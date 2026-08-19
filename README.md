# bagpipe (`bp`)

A fast, lightweight CLI tool to record, zstd-compress, summarize, and upload ROS 2 bags directly to Discord.

## Features

- **Transparent Recording**: Wraps `ros2 bag record`. Automatically compresses and ships the bag when stopped (`Ctrl+C`).
- **Smart Argument Inference**: Zero friction CLI. Infer recording or upload actions automatically.
- **Adaptive zstd Compression**: Automatically tunes compression levels (Level 1-9) based on topic types and Discord file size limits.
- **Metadata Summary**: Extracts duration, message counts, storage format, and topic breakdown from `metadata.yaml` for Discord embeds.
- **Size Limit Protection**: Handles Discord upload limits (default 25 MB) gracefully without crashing.
- **Single Binary**: Built with pure Rust. Fast and dependency-free.

## Installation

```bash
cargo install --git https://github.com/lazytatzv/bagpipe
```

Or from source:

```bash
git clone https://github.com/lazytatzv/bagpipe.git
cd bagpipe
cargo install --path .
```

## Quick Start

### 1. Configure Webhook (Run once)

```bash
bp --init "https://discord.com/api/webhooks/your/webhook/url"
```

### 2. Record & Auto-Send

Run `bp` followed by any standard `ros2 bag record` arguments:

```bash
# Record all topics and auto-ship on Ctrl+C
bp -a

# Record specific topics with a message
bp /camera/image_raw /cmd_vel -m "Obstacle avoidance test run"
```

### 3. Send Existing Bags

```bash
# Auto-detect and send the latest recorded bag in the current directory
bp

# Send a specific bag directory
bp ./rosbag2_2026_08_19

# Keep the compressed .tar.zst archive locally
bp ./rosbag2_2026_08_19 -k
```

### 4. Inspect Bag Metadata

```bash
# Show summary of the latest bag in current directory
bp info

# Show summary of a specific bag
bp info ./rosbag2_2026_08_19
```

## Command Reference

```text
Usage: bp [OPTIONS] [COMMAND] [RAW_ARGS]...

Arguments:
  [RAW_ARGS]...  Direct bag path (send) or ros2 record arguments (-a, /topic, etc.)

Commands:
  record  Record a ROS 2 bag and automatically compress & upload on stop (Ctrl+C)
  send    Compress and upload an existing ROS 2 bag
  info    Inspect and print ROS 2 bag summary without uploading
  init    Initialize or update Discord Webhook configuration
  help    Print help information

Options:
      --init <URL>       Save Discord Webhook URL
      --config           Show current configuration
  -f, --file <BAG_PATH>  Direct upload of an existing bag
  -m, --message <TEXT>   Custom comment to include in the Discord message
  -k, --keep             Keep compressed .tar.zst archive locally
      --dry-run          Parse and compress without uploading to Discord
  -h, --help             Print help
  -V, --version          Print version
```

## Configuration

View current configuration:

```bash
bp --config
```

Configuration file is stored at `~/.config/bagpipe/config.json`:

```json
{
  "webhook_url": "https://discord.com/api/webhooks/...",
  "max_file_size_mb": 25,
  "zstd_level": null
}
```

- `webhook_url`: Discord Webhook URL.
- `max_file_size_mb`: File upload size threshold in MB (default: 25).
- `zstd_level`: `null` for smart adaptive auto-tuning, or an integer from 1 to 22.

## License

MIT OR Apache-2.0
