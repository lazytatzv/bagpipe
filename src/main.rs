mod bag_meta;
mod compress;
mod config;
mod discord;
mod rsync;

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
use crate::rsync::sync_to_remote;

#[derive(Parser, Debug)]
#[command(
    name = "bagpipe",
    author = "lazytatzv",
    version = "0.1.0",
    about = "Record, zstd compress, summarize & ship ROS 2 bags via Discord or rsync",
    long_about = "bagpipe (bp / rosbag-pipe) - The ultimate ROS 2 bag pipeline tool.\n\nUltra-ergonomic usage:\n  bp                     # Auto-ship latest recorded rosbag in current directory\n  bp ./my_bag            # Auto-detect & ship existing bag\n  bp -a                  # Auto-infer `record -a`\n  bp /topic1 /topic2     # Auto-infer `record /topic1 /topic2`\n  bp --to user@host:/dir # Ship via rsync directly to remote machine\n  bp --init <URL>        # Quick Webhook setup"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Save Discord Webhook URL (e.g. `bp --init <WEBHOOK_URL>`)
    #[arg(long, value_name = "URL")]
    init: Option<String>,

    /// Set default remote rsync target (e.g. `bp --init-rsync user@host:/path/to/bags`)
    #[arg(long, value_name = "TARGET")]
    init_rsync: Option<String>,

    /// Show current configuration
    #[arg(long)]
    config: bool,

    /// Direct upload of an existing bag (shorthand for `bp send <PATH>`)
    #[arg(short = 'f', long = "file", value_name = "BAG_PATH")]
    file: Option<PathBuf>,

    /// Send to remote machine using rsync (e.g. `bp --to user@server:/bags` or `bp -t server:/bags`)
    #[arg(short = 't', long = "to", value_name = "REMOTE_TARGET")]
    to: Option<String>,

    /// Disable Discord notification/upload for this run
    #[arg(long)]
    no_discord: bool,

    /// Disable rsync transfer for this run
    #[arg(long)]
    no_rsync: bool,

    /// Custom comment/note to include in Discord message
    #[arg(short = 'm', long = "message", value_name = "TEXT")]
    message: Option<String>,

    /// Keep compressed .tar.zst archive locally
    #[arg(short = 'k', long = "keep")]
    keep_archive: bool,

    /// Dry-run mode (parse & compress, but skip network upload/sync)
    #[arg(long)]
    dry_run: bool,

    /// Direct path, topic list, or ros2 arguments (smart fallback)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    raw_args: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Record a ROS 2 bag and automatically compress & upload/sync on stop (Ctrl+C)
    #[command(trailing_var_arg = true, allow_hyphen_values = true)]
    Record {
        /// Optional bag output directory name (-o/--output flag or custom path)
        #[arg(short = 'o', long = "output", value_name = "OUTPUT_DIR")]
        output: Option<String>,

        /// Remote rsync destination (e.g. `user@host:/path/to/bags`)
        #[arg(short = 't', long = "to", value_name = "REMOTE_TARGET")]
        to: Option<String>,

        /// Disable Discord notification/upload for this run
        #[arg(long)]
        no_discord: bool,

        /// Disable rsync transfer for this run
        #[arg(long)]
        no_rsync: bool,

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

    /// Compress and upload/sync an existing ROS 2 bag directory or file
    Send {
        /// Path to ROS 2 bag folder or database file (defaults to latest in current directory)
        #[arg(value_name = "BAG_PATH")]
        path: Option<PathBuf>,

        /// Remote rsync destination (e.g. `user@host:/path/to/bags`)
        #[arg(short = 't', long = "to", value_name = "REMOTE_TARGET")]
        to: Option<String>,

        /// Disable Discord notification/upload for this run
        #[arg(long)]
        no_discord: bool,

        /// Disable rsync transfer for this run
        #[arg(long)]
        no_rsync: bool,

        /// Keep compressed .tar.zst archive after sending
        #[arg(short = 'k', long = "keep")]
        keep_archive: bool,

        /// Custom message for Discord embed
        #[arg(short = 'm', long = "message", value_name = "TEXT")]
        message: Option<String>,

        /// Dry-run mode (parse & compress, but do not send)
        #[arg(long)]
        dry_run: bool,
    },

    /// Unpack / extract a compressed bag archive (.tar.zst)
    Unpack {
        /// Path to .tar.zst archive (defaults to latest in current directory)
        archive: Option<PathBuf>,

        /// Output directory to unpack into (defaults to current directory)
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
    },

    /// Unpack and immediately play the bag using `ros2 bag play`
    Play {
        /// Path to .tar.zst archive or extracted bag folder (defaults to latest in current directory)
        target: Option<PathBuf>,

        /// Arguments passed transparently to `ros2 bag play` (e.g. `--loop`, `-r 2.0`, `/topic`)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        ros2_args: Vec<String>,
    },

    /// Manage configuration (view, set, get, or interactive edit)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },

    /// Inspect and print ROS 2 bag summary without uploading
    Info {
        /// Path to ROS 2 bag directory (defaults to latest in current directory)
        path: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Get a specific configuration key
    Get {
        /// Key name (webhook, rsync, max_size, zstd)
        key: String,
    },
    /// Set a specific configuration key
    Set {
        /// Key name (webhook, rsync, max_size, zstd)
        key: String,
        /// Value to set (use "null" or "auto" for zstd)
        value: String,
    },
    /// Open the config file in your default $EDITOR
    Edit,
    /// Reset configuration to default
    Reset,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = load_config();

    // 1. Handle --init
    if let Some(webhook) = cli.init {
        cfg.webhook_url = Some(webhook.clone());
        save_config(&cfg)?;
        println!("{}", "✓ Discord Webhook configured!".green().bold());
        println!("  Webhook URL: {}", webhook);
        return Ok(());
    }

    if let Some(rsync_target) = cli.init_rsync {
        cfg.rsync_target = Some(rsync_target.clone());
        save_config(&cfg)?;
        println!("{}", "✓ Remote rsync target configured!".green().bold());
        println!("  rsync target: {}", rsync_target);
        return Ok(());
    }

    // 2. Handle --config
    if cli.config {
        print_config(&cfg)?;
        return Ok(());
    }

    let rsync_target = if cli.no_rsync {
        None
    } else {
        cli.to.or_else(|| {
            if cfg.rsync_enabled.unwrap_or(true) { cfg.rsync_target.clone() } else { None }
        })
    };

    let discord_active = !cli.no_discord && cfg.discord_enabled.unwrap_or(true);

    // 3. Handle explicit -f / --file
    if let Some(bag_path) = cli.file {
        return handle_send(bag_path, cli.keep_archive, cli.message, cli.dry_run, rsync_target.as_deref(), discord_active, &cfg).await;
    }

    // 4. Subcommands
    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Unpack { archive, output } => {
                let archive_path = resolve_archive_path(archive)?;
                let out_dir = output.unwrap_or_else(|| PathBuf::from("."));
                return handle_unpack(&archive_path, &out_dir);
            }
            Commands::Play { target, ros2_args } => {
                return handle_play(target, ros2_args);
            }
            Commands::Config { action } => {
                return handle_config_action(action, &mut cfg);
            }
            Commands::Info { path } => {
                let bag_path = resolve_bag_path(path)?;
                return handle_info(&bag_path);
            }
            Commands::Send { path, to, no_discord, no_rsync, keep_archive, message, dry_run } => {
                let bag_path = resolve_bag_path(path)?;
                let target = if no_rsync { None } else { to.or(rsync_target) };
                let send_discord = !no_discord && discord_active;
                return handle_send(bag_path, keep_archive, message, dry_run, target.as_deref(), send_discord, &cfg).await;
            }
            Commands::Record { output, to, no_discord, no_rsync, keep_archive, message, dry_run, ros2_args } => {
                let target = if no_rsync { None } else { to.or(rsync_target) };
                let send_discord = !no_discord && discord_active;
                return handle_record(output, keep_archive, message, dry_run, target.as_deref(), send_discord, ros2_args, &cfg).await;
            }
        }
    }

    // 5. Smart Raw Args Auto-Inference:
    if cli.raw_args.is_empty() {
        let latest = find_latest_bag_in_dir(Path::new("."))?;
        println!("{}", format!("🔍 Detected latest bag in current directory: {}", latest.display()).bold().cyan());
        return handle_send(latest, cli.keep_archive, cli.message, cli.dry_run, rsync_target.as_deref(), discord_active, &cfg).await;
    }

    // Check if raw_arg is a direct key=value config setting (e.g. `bp rsync=user@host:/bags` or `bp discord=off`)
    if cli.raw_args.len() == 1 && cli.raw_args[0].contains('=') && !cli.raw_args[0].starts_with('/') && !cli.raw_args[0].starts_with('.') {
        let parts: Vec<&str> = cli.raw_args[0].splitn(2, '=').collect();
        let key = parts[0];
        let val = parts[1];
        if matches!(key.to_lowercase().as_str(), "webhook" | "discord" | "rsync" | "to" | "max_size" | "zstd") {
            return handle_config_action(Some(ConfigAction::Set { key: key.to_string(), value: val.to_string() }), &mut cfg);
        }
    }

    let first_arg = &cli.raw_args[0];
    let first_path = PathBuf::from(first_arg);

    // If first_arg is a .tar.zst archive file -> auto unpack / inspect
    if first_path.exists() && (first_arg.ends_with(".tar.zst") || first_arg.ends_with(".tar.zstd") || first_arg.ends_with(".zst")) && cli.raw_args.len() == 1 {
        return handle_unpack(&first_path, Path::new("."));
    }

    if (first_path.exists() && (first_path.is_dir() || first_arg.ends_with(".db3") || first_arg.ends_with(".mcap")))
        && cli.raw_args.len() == 1
    {
        return handle_send(first_path, cli.keep_archive, cli.message, cli.dry_run, rsync_target.as_deref(), discord_active, &cfg).await;
    }

    handle_record(None, cli.keep_archive, cli.message, cli.dry_run, rsync_target.as_deref(), discord_active, cli.raw_args, &cfg).await
}

fn print_config(cfg: &Config) -> Result<()> {
    let discord_status = match (&cfg.webhook_url, cfg.discord_enabled.unwrap_or(true)) {
        (Some(url), true) => format!("Enabled ({})", url),
        (Some(url), false) => format!("Disabled (URL: {})", url),
        (None, _) => "(Not configured)".to_string(),
    };

    let rsync_status = match (&cfg.rsync_target, cfg.rsync_enabled.unwrap_or(true)) {
        (Some(target), true) => format!("Enabled ({})", target),
        (Some(target), false) => format!("Disabled (Target: {})", target),
        (None, _) => "(Not configured)".to_string(),
    };

    println!("{}", "Current bagpipe configuration:".bold().cyan());
    println!("  Config Path : {}", config::get_config_path()?.display());
    println!("  Discord     : {}", discord_status);
    println!("  rsync       : {}", rsync_status);
    println!("  Max Upload  : {} MB", cfg.max_file_size_mb.unwrap_or(25));
    println!("  Zstd Level  : {}", cfg.zstd_level.map(|l| format!("Level {} (Manual)", l)).unwrap_or_else(|| "Auto-Tuned (Smart Adaptive)".to_string()));
    Ok(())
}

fn handle_config_action(action: Option<ConfigAction>, cfg: &mut Config) -> Result<()> {
    match action {
        None => {
            print_config(cfg)?;
        }
        Some(ConfigAction::Get { key }) => match key.to_lowercase().as_str() {
            "webhook" | "webhook_url" => println!("{}", cfg.webhook_url.as_deref().unwrap_or("")),
            "discord" => println!("{}", if cfg.discord_enabled.unwrap_or(true) { "on" } else { "off" }),
            "rsync" | "rsync_target" | "to" => println!("{}", cfg.rsync_target.as_deref().unwrap_or("")),
            "rsync_enabled" => println!("{}", if cfg.rsync_enabled.unwrap_or(true) { "on" } else { "off" }),
            "max_size" | "max_size_mb" => println!("{}", cfg.max_file_size_mb.unwrap_or(25)),
            "zstd" | "zstd_level" => println!("{}", cfg.zstd_level.map(|l| l.to_string()).unwrap_or_else(|| "auto".to_string())),
            other => anyhow::bail!("Unknown config key '{}'. Available keys: webhook, discord, rsync, max_size, zstd", other),
        },
        Some(ConfigAction::Set { key, value }) => {
            let val_lower = value.to_lowercase();
            match key.to_lowercase().as_str() {
                "discord" => {
                    cfg.discord_enabled = Some(matches!(val_lower.as_str(), "true" | "1" | "on" | "enable" | "yes"));
                }
                "webhook" | "webhook_url" => {
                    if val_lower == "off" || val_lower == "disable" || val_lower == "false" {
                        cfg.discord_enabled = Some(false);
                    } else if val_lower == "on" || val_lower == "enable" || val_lower == "true" {
                        cfg.discord_enabled = Some(true);
                    } else if value.is_empty() || value == "null" || value == "none" {
                        cfg.webhook_url = None;
                    } else {
                        cfg.webhook_url = Some(value.clone());
                        cfg.discord_enabled = Some(true);
                    }
                }
                "rsync" | "rsync_target" | "to" => {
                    if val_lower == "off" || val_lower == "disable" || val_lower == "false" {
                        cfg.rsync_enabled = Some(false);
                    } else if val_lower == "on" || val_lower == "enable" || val_lower == "true" {
                        cfg.rsync_enabled = Some(true);
                    } else if value.is_empty() || value == "null" || value == "none" {
                        cfg.rsync_target = None;
                    } else {
                        cfg.rsync_target = Some(value.clone());
                        cfg.rsync_enabled = Some(true);
                    }
                }
                "max_size" | "max_size_mb" => {
                    let mb: u64 = value.parse().context("max_size must be a positive integer (MB)")?;
                    cfg.max_file_size_mb = Some(mb);
                }
                "zstd" | "zstd_level" => {
                    if value == "auto" || value == "null" || value == "none" {
                        cfg.zstd_level = None;
                    } else {
                        let lvl: i32 = value.parse().context("zstd_level must be between 1 and 22, or 'auto'")?;
                        if !(1..=22).contains(&lvl) {
                            anyhow::bail!("zstd level must be between 1 and 22");
                        }
                        cfg.zstd_level = Some(lvl);
                    }
                }
                other => anyhow::bail!("Unknown config key '{}'. Available keys: webhook, discord, rsync, max_size, zstd", other),
            }
            save_config(cfg)?;
            println!("{}", format!("✓ Set {} = {}", key, value).green().bold());
        }
        Some(ConfigAction::Edit) => {
            let path = config::get_config_path()?;
            if !path.exists() {
                save_config(cfg)?;
            }
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
            Command::new(editor)
                .arg(&path)
                .status()
                .context("Failed to open config in editor")?;
        }
        Some(ConfigAction::Reset) => {
            *cfg = Config::default();
            save_config(cfg)?;
            println!("{}", "✓ Configuration reset to defaults.".green().bold());
        }
    }
    Ok(())
}


fn resolve_archive_path(path_opt: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = path_opt {
        Ok(p)
    } else {
        let mut archives = Vec::new();
        if let Ok(entries) = std::fs::read_dir(".") {
            for entry in entries.flatten() {
                let p = entry.path();
                let name = p.file_name().unwrap_or_default().to_string_lossy();
                if p.is_file() && (name.ends_with(".tar.zst") || name.ends_with(".tar.zstd") || name.ends_with(".zst")) {
                    if let Ok(meta) = p.metadata() {
                        if let Ok(mod_time) = meta.modified() {
                            archives.push((p, mod_time));
                        }
                    }
                }
            }
        }
        archives.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some((latest, _)) = archives.into_iter().next() {
            Ok(latest)
        } else {
            anyhow::bail!("No .tar.zst archive found in current directory. Specify path with `bp unpack <FILE>`.");
        }
    }
}

fn handle_unpack(archive_path: &Path, output_dir: &Path) -> Result<()> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ")
            .template("{spinner:.green} {msg}")?,
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb.set_message(format!("Decompressing {} with zstd...", archive_path.display()));

    let start = std::time::Instant::now();
    let extracted_path = crate::compress::decompress_archive(archive_path, output_dir)?;
    let elapsed = start.elapsed();

    pb.finish_and_clear();

    println!("{}", "✓ Decompression Complete!".green().bold());
    println!("  Extracted to : {}", extracted_path.display().to_string().bold());
    println!("  Time Taken   : {:.2}s", elapsed.as_secs_f64());

    if extracted_path.join("metadata.yaml").exists() {
        let _ = handle_info(&extracted_path);
    }

    Ok(())
}

fn handle_play(target_opt: Option<PathBuf>, ros2_args: Vec<String>) -> Result<()> {
    let target = if let Some(t) = target_opt {
        t
    } else {
        // Find latest archive or bag
        if let Ok(archive) = resolve_archive_path(None) {
            archive
        } else {
            resolve_bag_path(None)?
        }
    };

    let bag_dir = if target.is_file() {
        let name = target.file_name().unwrap_or_default().to_string_lossy();
        if name.ends_with(".tar.zst") || name.ends_with(".tar.zstd") || name.ends_with(".zst") {
            println!("{}", format!("Decompressing {} for immediate playback...", target.display()).cyan());
            crate::compress::decompress_archive(&target, Path::new("."))?
        } else {
            target
        }
    } else {
        target
    };

    println!("{}", format!("▶️ Playing bag: {}", bag_dir.display()).green().bold());

    let mut cmd = Command::new("ros2");
    cmd.arg("bag").arg("play").arg(&bag_dir);
    cmd.args(&ros2_args);
    cmd.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());

    let mut child = cmd.spawn().context("Failed to execute 'ros2 bag play'. Is ROS 2 sourced?")?;
    let _status = child.wait().context("Failed to wait on ros2 bag play")?;

    Ok(())
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
    rsync_target: Option<&str>,
    send_discord: bool,
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

    println!("{}", "Starting ROS 2 bag recording...".green().bold());
    println!("   Output Directory : {}", bag_output_dir.bold());
    println!("   (Press {} to stop recording and start compression/pipeline)", "Ctrl+C".yellow().bold());

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
    println!("\n{}", "Recording finished.".yellow().bold());

    let bag_path = PathBuf::from(&bag_output_dir);
    if !bag_path.exists() {
        anyhow::bail!("Expected bag directory '{}' was not created.", bag_output_dir);
    }

    // Process pipeline
    handle_send(bag_path, keep_archive, custom_message, dry_run, rsync_target, send_discord, cfg).await
}

async fn handle_send(
    bag_path: PathBuf,
    keep_archive: bool,
    custom_message: Option<String>,
    dry_run: bool,
    rsync_target: Option<&str>,
    send_discord: bool,
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
    let archive_path = if keep_archive || rsync_target.is_some() {
        summary.bag_path.parent().unwrap_or_else(|| Path::new(".")).join(&archive_name)
    } else {
        std::env::temp_dir().join(&archive_name)
    };

    let max_mb = cfg.max_file_size_mb.unwrap_or(25);
    let max_bytes = max_mb * 1024 * 1024;
    let (mut zstd_level, mut mode_desc) = match cfg.zstd_level {
        Some(lvl) if lvl > 0 => (lvl, "User-configured"),
        _ => crate::compress::determine_optimal_zstd_level(&summary, max_mb),
    };

    pb.set_message(format!("Compressing with zstd (Level {}, {})...", zstd_level, mode_desc));

    let comp_start = std::time::Instant::now();
    let mut compressed_size = compress_bag_dir(&summary.bag_path, &archive_path, zstd_level)
        .with_context(|| format!("Failed to compress bag directory to {}", archive_path.display()))?;

    // If compressed size is slightly over the Discord limit (<= 1.35x limit), re-compress at max level (19)
    if compressed_size > max_bytes && compressed_size <= (max_bytes as f64 * 1.35) as u64 && zstd_level < 19 && cfg.zstd_level.is_none() {
        pb.set_message("Size close to limit: re-compressing with Ultra zstd (Level 19) to fit Discord...".to_string());
        zstd_level = 19;
        mode_desc = "Ultra max-compression (fit Discord limit)";
        if let Ok(new_size) = compress_bag_dir(&summary.bag_path, &archive_path, zstd_level) {
            compressed_size = new_size;
        }
    }

    let comp_elapsed = comp_start.elapsed();

    let ratio = if raw_size > 0 {
        (compressed_size as f64 / raw_size as f64) * 100.0
    } else {
        100.0
    };

    pb.finish_and_clear();

    println!("{}", "Compression Complete!".green().bold());
    println!("  Original Size : {}", human_bytes::human_bytes(raw_size as f64).cyan());
    println!("  Compressed    : {}", human_bytes::human_bytes(compressed_size as f64).green().bold());
    println!("  Ratio         : {:.1}% (in {:.2}s, zstd: L{} {})", ratio, comp_elapsed.as_secs_f64(), zstd_level, mode_desc.dimmed());
    if keep_archive {
        println!("  Archive Saved : {}", archive_path.display());
    }

    if dry_run {
        println!("{}", "Dry-run enabled: skipped network upload and rsync.".yellow());
        return Ok(());
    }

    // Handle rsync transfer if target specified
    let mut rsync_successful = false;
    if let Some(target) = rsync_target {
        println!("\n{}", format!("Transmitting archive via rsync to {}...", target).bold().cyan());
        match sync_to_remote(&archive_path, target) {
            Ok(()) => {
                println!("{}", "Successfully synced to remote target via rsync!".green().bold());
                rsync_successful = true;
            }
            Err(e) => {
                eprintln!("{} Failed to rsync: {:?}", "✗".red().bold(), e);
            }
        }
    }

    // Handle Discord Webhook if enabled and configured
    if send_discord {
        if let Some(webhook_url) = &cfg.webhook_url {
            if !webhook_url.trim().is_empty() {
                let upload_pb = ProgressBar::new_spinner();
                upload_pb.set_style(
                    ProgressStyle::default_spinner()
                        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ")
                        .template("{spinner:.cyan} {msg}")?,
                );
                upload_pb.enable_steady_tick(std::time::Duration::from_millis(80));
                upload_pb.set_message("Uploading report to Discord...");

                let synced_str = if rsync_successful { rsync_target } else { None };

                let res = send_to_discord(
                    webhook_url.trim(),
                    &summary,
                    Some(&archive_path),
                    raw_size,
                    Some(compressed_size),
                    max_mb,
                    custom_message.as_deref(),
                    synced_str,
                ).await;

                upload_pb.finish_and_clear();

                match res {
                    Ok(()) => {
                        println!("{}", "Successfully sent report to Discord!".green().bold());
                    }
                    Err(e) => {
                        eprintln!("{} Failed to send to Discord: {:?}", "✗".red().bold(), e);
                    }
                }
            }
        }
    } else if rsync_target.is_none() {
        println!("\n{}", "No remote destination active!".yellow().bold());
        println!("Enable Discord with: {}", "bp discord=on".cyan());
        println!("Or enable rsync with: {}", "bp rsync=on".cyan());
    }

    if !keep_archive && archive_path.exists() && rsync_target.is_none() {
        let _ = std::fs::remove_file(&archive_path);
    }

    Ok(())
}


