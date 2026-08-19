use anyhow::{Context, Result};
use colored::Colorize;
use human_bytes::human_bytes;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const DEFAULT_PORT: u16 = 8990;
const PROTOCOL_MAGIC: &[u8; 8] = b"BAGPIPE1";

/// Sends an archive file at full line rate directly to a receiver via high-speed zero-overhead TCP stream.
pub async fn send_direct_stream(file_path: &Path, target_addr: &str) -> Result<()> {
    let connect_addr = if target_addr.contains(':') {
        target_addr.to_string()
    } else {
        format!("{}:{}", target_addr, DEFAULT_PORT)
    };

    // Resolves standard IP, IPv6, or Tailscale MagicDNS hostname (e.g. my-desktop:8990)
    let addr = tokio::net::lookup_host(&connect_addr).await
        .with_context(|| format!("Failed to resolve target address '{}'", connect_addr))?
        .next()
        .context("No address resolved for target")?;

    let filename = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let file_size = file_path.metadata()?.len();

    let mut stream = TcpStream::connect(addr).await
        .with_context(|| format!("Failed to connect to receiver at {}", addr))?;

    // Disable Nagle's algorithm for minimum latency
    stream.set_nodelay(true)?;

    // 1. Send Magic Header
    stream.write_all(PROTOCOL_MAGIC).await?;

    // 2. Send Filename length + Filename
    let name_bytes = filename.as_bytes();
    stream.write_u32(name_bytes.len() as u32).await?;
    stream.write_all(name_bytes).await?;

    // 3. Send File Size
    stream.write_u64(file_size).await?;

    // 4. Stream data directly with large buffer
    let mut file = tokio::fs::File::open(file_path).await?;
    let pb = ProgressBar::new(file_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})")?
            .progress_chars("#>-"),
    );

    let mut buffer = vec![0u8; 2 * 1024 * 1024]; // 2MB stream buffer
    let mut sent = 0;

    let start_time = std::time::Instant::now();
    while sent < file_size {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        stream.write_all(&buffer[..n]).await?;
        sent += n as u64;
        pb.set_position(sent);
    }
    stream.flush().await?;
    pb.finish_and_clear();

    let elapsed = start_time.elapsed().as_secs_f64();
    let mb_per_sec = (file_size as f64 / (1024.0 * 1024.0)) / elapsed.max(0.001);

    println!("{}", "✓ Direct High-Speed Stream Finished!".green().bold());
    println!("  Total Sent  : {}", human_bytes(file_size as f64));
    println!("  Throughput  : {:.2} MB/s (in {:.2}s)", mb_per_sec, elapsed);

    Ok(())
}

/// Receives a stream and automatically unpacks it.
pub async fn receive_stream(listen_port: u16, auto_unpack: bool, auto_play: bool) -> Result<PathBuf> {
    let bind_addr = format!("0.0.0.0:{}", listen_port);
    let listener = TcpListener::bind(&bind_addr).await
        .with_context(|| format!("Failed to bind listener on port {}", listen_port))?;

    println!("{}", "🚀 bagpipe Direct Receiver Online".bold().cyan());
    println!("  Listening on   : {}", bind_addr.yellow().bold());
    println!("  Status         : Ready for incoming high-speed stream...");

    let (mut stream, peer_addr) = listener.accept().await?;
    stream.set_nodelay(true)?;
    println!("\n{}", format!("⚡ Connected from {}", peer_addr).green().bold());

    // 1. Verify Magic
    let mut magic = [0u8; 8];
    stream.read_exact(&mut magic).await?;
    if &magic != PROTOCOL_MAGIC {
        anyhow::bail!("Invalid stream protocol received");
    }

    // 2. Read Filename
    let name_len = stream.read_u32().await? as usize;
    let mut name_buf = vec![0u8; name_len];
    stream.read_exact(&mut name_buf).await?;
    let filename = String::from_utf8(name_buf)?;

    // 3. Read File Size
    let file_size = stream.read_u64().await?;

    let output_path = PathBuf::from(&filename);
    let mut file = tokio::fs::File::create(&output_path).await?;

    let pb = ProgressBar::new(file_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.green/blue}] {bytes}/{total_bytes} ({bytes_per_sec})")?
            .progress_chars("#>-"),
    );

    let mut buffer = vec![0u8; 2 * 1024 * 1024]; // 2MB stream buffer
    let mut received = 0;
    let start_time = std::time::Instant::now();

    while received < file_size {
        let to_read = std::cmp::min(buffer.len(), (file_size - received) as usize);
        let n = stream.read(&mut buffer[..to_read]).await?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n]).await?;
        received += n as u64;
        pb.set_position(received);
    }
    file.flush().await?;
    pb.finish_and_clear();

    let elapsed = start_time.elapsed().as_secs_f64();
    let mb_per_sec = (file_size as f64 / (1024.0 * 1024.0)) / elapsed.max(0.001);

    println!("{}", "✓ File Received Successfully!".green().bold());
    println!("  Saved File  : {}", output_path.display().to_string().bold());
    println!("  Speed       : {:.2} MB/s", mb_per_sec);

    if auto_unpack || auto_play {
        println!("{}", "Unpacking archive...".cyan());
        let extracted = crate::compress::decompress_archive(&output_path, Path::new("."))?;
        println!("{}", format!("✓ Extracted to {}", extracted.display()).green().bold());

        if auto_play {
            println!("{}", "Starting ros2 bag play...".green().bold());
            let mut cmd = std::process::Command::new("ros2");
            cmd.arg("bag").arg("play").arg(&extracted);
            cmd.stdin(std::process::Stdio::inherit()).stdout(std::process::Stdio::inherit()).stderr(std::process::Stdio::inherit());
            let mut child = cmd.spawn()?;
            let _ = child.wait()?;
        }
    }

    Ok(output_path)
}
