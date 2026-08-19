mod bag_meta;
mod compress;
mod config;
mod discord;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::bag_meta::parse_bag_metadata;
use crate::compress::compress_bag_dir;
use crate::config::{load_config, save_config, Config};
use crate::discord::send_to_discord;

#[derive(Parser, Debug)]
#[command(
    name = "bagpipe",
    author = "lazytatzv",
    version = "0.1.0",
    about = "🚀 Record, zstd compress, summarize & ship ROS 2 bags to Discord",
    long_about = "bagpipe (bp / rosbag-pipe) - The ultimate ROS 2 bag pipeline tool.\n\nUltra-ergonomic usage:\n  bp                     # Auto-send the latest recorded rosbag in current directory\n  bp ./my_bag            # Auto-detect & send existing bag\n  bp -a                  # Auto-infer `record -a`\n  bp /topic1 /topic2     # Auto-infer `record /topic1 /topic2`\n  bp --init <URL>        # Quick Webhook setup"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Save Discord Webhook URL (e.g. `bp --init <WEBHOOK_URL>`)
    #[arg(long, value_name = "URL")]
    init: Option<String>,

    /// Show current configuration
    #[arg(long)]
    config: bool,

    /// Direct upload of an existing bag (shorthand for `bp send <PATH>`)
    #[arg(short = 'f', long = "file", value_name = "BAG_PATH")]
    file: Option<PathBuf>,

    /// Custom comment/note to include in Discord message
    #[arg(short = 'm', long = "message", value_name = "TEXT")]
    message: Option<String>,

    /// Keep compressed .tar.zst archive locally
    #[arg(short = 'k', long = "keep")]
    keep_archive: bool,

    /// Dry-run mode (parse & compress, but skip Discord upload)
    #[arg(long)]
    dry_run: bool,

    /// Direct path, topic list, or ros2 arguments (smart fallback)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    raw_args: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Record a ROS 2 bag and automatically compress & upload on stop (Ctrl+C)
    #[command(trailing_var_arg = true, allow_hyphen_values = true)]
    Record {
        /// Optional bag output directory name (-o/--output flag or custom path)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT_DIR")]
        output: Option<String>,

        /// Keep compressed .tar.zst archive in output directory
        #[arg(short = 'k', long = "keep")]
        keep_archive: bool,

        /// Custom message for Discord embed
        #[arg(short = 'm', long = "message", value_name = "TEXT")]
        message: Option<String>,

        /// Dry-run mode
        #[arg(long)]
        dry_run: bool,

        /// Arguments passed transparently to `ros2 bag record` (e.g. `-a`, `/topic1 /topic2`)
        #[arg(value_name = "ROS2_ARGS")]
        ros2_args: Vec<String>,
    },

    /// Compress and upload an existing ROS 2 bag directory or file
    Send {
        /// Path to ROS 2 bag folder or database file (defaults to latest in current directory)
        #[arg(value_name = "BAG_PATH")]
        path: Option<PathBuf>,

        /// Keep compressed .tar.zst archive after sending
        #[arg(short = 'k', long = "keep")]
        keep_archive: bool,

        /// Custom message for Discord embed
        #[arg(short = 'm', long = "message", value_name = "TEXT")]
        message: Option<String>,

        /// Dry-run mode (parse & compress, but do not send to Discord)
        #[arg(long)]
        dry_run: bool,
    },

    /// Initialize or update Discord Webhook configuration
    Init {
        /// Discord Webhook URL
        webhook_url: String,

        /// Max upload file size in MB (default: 25)
        #[arg(long, default_value = "25")]
        max_size_mb: u64,
    },

    /// Inspect and print ROS 2 bag summary without uploading
    Info {
        /// Path to ROS 2 bag directory (defaults to latest in current directory)
        path: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = load_config();

    // 1. Handle --init
    if let Some(webhook) = cli.init {
        cfg.webhook_url = Some(webhook.clone());
        save_config(&cfg)?;
        println!("{}", "✓ Configuration updated successfully!".green().bold());
        println!("  Webhook URL: {}", webhook);
        return Ok(());
    }

    // 2. Handle --config
    if cli.config {
        println!("{}", "⚙️  Current bagpipe configuration:".bold().cyan());
        println!("  Config Path : {}", config::get_config_path()?.display());
        println!("  Webhook URL : {}", cfg.webhook_url.as_deref().unwrap_or("(Not configured)"));
        println!("  Max Upload  : {} MB", cfg.max_file_size_mb.unwrap_or(25));
        println!("  Zstd Level  : {}", cfg.zstd_level.unwrap_or(3));
        return Ok(());
    }

    // 3. Handle explicit -f / --file
    if let Some(bag_path) = cli.file {
        return handle_send(bag_path, cli.keep_archive, cli.message, cli.dry_run, &cfg).await;
    }

    // 4. Subcommands
    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Init { webhook_url, max_size_mb } => {
                cfg.webhook_url = Some(webhook_url.clone());
                cfg.max_file_size_mb = Some(max_size_mb);
                save_config(&cfg)?;
                println!("{}", "✓ Configuration saved!".green().bold());
                println!("  Webhook URL : {}", webhook_url);
                println!("  Max Size    : {} MB", max_size_mb);
                return Ok(());
            }
            Commands::Info { path } => {
                let bag_path = resolve_bag_path(path)?;
                return handle_info(&bag_path);
            }
            Commands::Send { path, keep_archive, message, dry_run } => {
                let bag_path = resolve_bag_path(path)?;
                return handle_send(bag_path, keep_archive, message, dry_run, &cfg).await;
            }
            Commands::Record { output, keep_archive, message, dry_run, ros2_args } => {
                return handle_record(output, keep_archive, message, dry_run, ros2_args, &cfg).await;
            }
        }
    }

    // 5. Smart Raw Args Auto-Inference:
    // If no subcommand is specified, intelligently decide what the user wants:
    // Case A: `bp` (no args) -> Find and send latest bag in current dir
    // Case B: `bp path/to/bag` -> Send that bag
    // Case C: `bp -a` or `bp /topic1` -> Record and send
    if cli.raw_args.is_empty() {
        let latest = find_latest_bag_in_dir(Path::new("."))?;
        println!("{}", format!("🔍 Detected latest bag in current directory: {}", latest.display()).bold().cyan());
        return handle_send(latest, cli.keep_archive, cli.message, cli.dry_run, &cfg).await;
    }

    // Check if first arg is an existing directory/file or starts with '-' / '/'
    let first_arg = &cli.raw_args[0];
    let first_path = PathBuf::from(first_arg);

    if (first_path.exists() && (first_path.is_dir() || first_arg.ends_with(".db3") || first_arg.ends_with(".mcap")))
        && cli.raw_args.len() == 1
    {
        return handle_send(first_path, cli.keep_archive, cli.message, cli.dry_run, &cfg).await;
    }

    // Otherwise, treat all raw_args as arguments to `record` (e.g. `bp -a`, `bp /tf /scan`)
    handle_record(None, cli.keep_archive, cli.message, cli.dry_run, cli.raw_args, &cfg).await
}

fn resolve_bag_path(path_opt: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = path_opt {
        Ok(p)
    } else {
        find_latest_bag_in_dir(Path::new("."))
    }
}

fn find_latest_bag_in_dir(dir: &Path) -> Result<PathBuf> {
    let mut bags = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("metadata.yaml").exists() {
                if let Ok(meta) = p.metadata() {
                    if let Ok(modified) = meta.modified() {
                        bags.push((p, modified));
                    }
                }
            }
        }
    }
    bags.sort_by(|a, b| b.1.cmp(&a.1));
    if let Some((latest, _)) = bags.into_iter().next() {
        Ok(latest)
    } else {
        anyhow::bail!("No ROS 2 bag with metadata.yaml found in current directory. Specify path with `bp <PATH>` or record with `bp -a`.");
    }
}


fn handle_info(path: &Path) -> Result<()> {
    let summary = parse_bag_metadata(path)?;
    println!("\n{}", "📦 ROS 2 Bag Metadata Summary".bold().cyan());
    println!("  Name        : {}", summary.bag_name.bold());
    println!("  Path        : {}", summary.bag_path.display());
    println!("  Raw Size    : {}", human_bytes::human_bytes(summary.total_raw_size_bytes as f64));
    println!("  Storage     : {}", summary.storage_format);
    println!("  Duration    : {:.2}s", summary.duration_sec);
    println!("  Start Time  : {}", summary.start_time_str);
    println!("  Total Msgs  : {}", summary.message_count);
    println!("\n{}", "📊 Recorded Topics:".bold());
    for (name, type_name, count) in &summary.topics {
        println!("  • {:<35} {:<25} {:>6} msgs", name.cyan(), type_name.dimmed(), count);
    }
    println!();
    Ok(())
}

async fn handle_record(
    output_opt: Option<String>,
    keep_archive: bool,
    custom_message: Option<String>,
    dry_run: bool,
    ros2_args: Vec<String>,
    cfg: &Config,
) -> Result<()> {
    let bag_output_dir = if let Some(o) = output_opt {
        o
    } else {
        // Find if -o or --output is inside ros2_args
        let mut found = None;
        let mut iter = ros2_args.iter().peekable();
        while let Some(arg) = iter.next() {
            if (arg == "-o" || arg == "--output") && iter.peek().is_some() {
                found = Some(iter.next().unwrap().to_string());
                break;
            }
        }
        found.unwrap_or_else(|| {
            let timestamp = chrono::Local::now().format("rosbag2_%Y_%m_%d-%H_%M_%S").to_string();
            timestamp
        })
    };

    println!("{}", "🎥 Starting ROS 2 bag recording...".green().bold());
    println!("   Output Directory : {}", bag_output_dir.bold());
    println!("   (Press {} to stop recording and start compression/upload)", "Ctrl+C".yellow().bold());

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }).ok();

    let mut cmd = Command::new("ros2");
    cmd.arg("bag").arg("record");

    // Check if -o / --output already in args
    let has_output_flag = ros2_args.iter().any(|a| a == "-o" || a == "--output");
    if !has_output_flag {
        cmd.arg("-o").arg(&bag_output_dir);
    }

    cmd.args(&ros2_args);
    cmd.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());

    let mut child = cmd.spawn().context("Failed to execute 'ros2 bag record'. Is ROS 2 sourced?")?;

    let _status = child.wait().context("Failed to wait on ros2 process")?;
    println!("\n{}", "🛑 Recording finished.".yellow().bold());

    let bag_path = PathBuf::from(&bag_output_dir);
    if !bag_path.exists() {
        anyhow::bail!("Expected bag directory '{}' was not created.", bag_output_dir);
    }

    // Process pipeline
    handle_send(bag_path, keep_archive, custom_message, dry_run, cfg).await
}

async fn handle_send(
    bag_path: PathBuf,
    keep_archive: bool,
    custom_message: Option<String>,
    dry_run: bool,
    cfg: &Config,
) -> Result<()> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ")
            .template("{spinner:.green} {msg}")?,
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));

    pb.set_message("Parsing ROS 2 bag metadata...");
    let summary = parse_bag_metadata(&bag_path)
        .with_context(|| format!("Failed to parse ROS 2 bag at {}", bag_path.display()))?;

    let raw_size = summary.total_raw_size_bytes;
    let archive_name = format!("{}.tar.zst", summary.bag_name);
    let archive_path = if keep_archive {
        summary.bag_path.parent().unwrap_or_else(|| Path::new(".")).join(&archive_name)
    } else {
        std::env::temp_dir().join(&archive_name)
    };

    let max_mb = cfg.max_file_size_mb.unwrap_or(25);
    let (zstd_level, mode_desc) = match cfg.zstd_level {
        Some(lvl) if lvl > 0 => (lvl, "User-configured"),
        _ => crate::compress::determine_optimal_zstd_level(&summary, max_mb),
    };

    pb.set_message(format!("Compressing with zstd (Level {}, {})...", zstd_level, mode_desc));

    let comp_start = std::time::Instant::now();
    let compressed_size = compress_bag_dir(&summary.bag_path, &archive_path, zstd_level)
        .with_context(|| format!("Failed to compress bag directory to {}", archive_path.display()))?;
    let comp_elapsed = comp_start.elapsed();

    let ratio = if raw_size > 0 {
        (compressed_size as f64 / raw_size as f64) * 100.0
    } else {
        100.0
    };

    pb.finish_and_clear();

    println!("{}", "✨ Compression Complete!".green().bold());
    println!("  Original Size : {}", human_bytes::human_bytes(raw_size as f64).cyan());
    println!("  Compressed    : {}", human_bytes::human_bytes(compressed_size as f64).green().bold());
    println!("  Ratio         : {:.1}% (in {:.2}s, zstd: L{} {})", ratio, comp_elapsed.as_secs_f64(), zstd_level, mode_desc.dimmed());
    if keep_archive {
        println!("  Archive Saved : {}", archive_path.display());
    }

    if dry_run {
        println!("{}", "⚡ Dry-run enabled: skipped Discord upload.".yellow());
        return Ok(());
    }

    let webhook_url = match &cfg.webhook_url {
        Some(url) if !url.trim().is_empty() => url.trim(),
        _ => {
            println!("\n{}", "⚠️  No Discord Webhook URL configured!".yellow().bold());
            println!("Run {} to configure Webhook, or set WEBHOOK_URL.", "bp init <WEBHOOK_URL>".cyan());
            return Ok(());
        }
    };

    let max_mb = cfg.max_file_size_mb.unwrap_or(25);
    let upload_pb = ProgressBar::new_spinner();
    upload_pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ")
            .template("{spinner:.cyan} {msg}")?,
    );
    upload_pb.enable_steady_tick(std::time::Duration::from_millis(80));
    upload_pb.set_message("Uploading report to Discord...");

    let res = send_to_discord(
        webhook_url,
        &summary,
        Some(&archive_path),
        raw_size,
        Some(compressed_size),
        max_mb,
        custom_message.as_deref(),
    ).await;

    if !keep_archive && archive_path.exists() {
        let _ = std::fs::remove_file(&archive_path);
    }

    upload_pb.finish_and_clear();

    match res {
        Ok(()) => {
            println!("{}", "🚀 Successfully sent to Discord!".green().bold());
        }
        Err(e) => {
            eprintln!("{} Failed to send to Discord: {:?}", "✗".red().bold(), e);
        }
    }

    Ok(())
}
