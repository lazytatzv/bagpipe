# bagpipe (`bp`)

A fast, lightweight CLI tool to record, zstd-compress, summarize, and ship ROS 2 bags via **rsync** and/or **Discord**.

## Features

- **Transparent Recording**: Wraps `ros2 bag record`. Automatically compresses and ships the bag when stopped (`Ctrl+C`).
- **Flexible Transmission**: Ship directly to remote workstations / GPU servers via `rsync`, upload to Discord Webhooks, or do both simultaneously.
- **Adaptive zstd Compression**: High-efficiency multi-threaded compression (.tar.zst) tuned for fast network transmission.
- **Metadata Summary**: Extracts duration, message counts, storage format, and topic breakdown from `metadata.yaml`.
- **Zero Friction CLI**: Infers recording or upload actions automatically.
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

### 1. Configuration (Run once)

Configure default destinations:

```bash
# Set default remote server via rsync
bp --init-rsync user@server:/data/rosbags

# And/or configure Discord Webhook
bp --init "https://discord.com/api/webhooks/your/webhook/url"
```

### 2. Record & Auto-Ship

```bash
# Record all topics -> compress -> auto-rsync / upload on Ctrl+C
bp -a

# Record and ship to a specific remote server
bp -a --to user@gpu-server:/data/bags

# Record specific topics with a custom note
bp /camera/image_raw /cmd_vel -m "Obstacle avoidance test run"
```

### 3. Ship Existing Bags

```bash
# Auto-detect latest recorded bag in current directory and ship
bp

# Ship to a remote machine via rsync
bp -t user@server:/bags

# Ship a specific bag directory
bp ./rosbag2_2026_08_19
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
  record  Record a ROS 2 bag and automatically compress & ship on stop (Ctrl+C)
  send    Compress and ship an existing ROS 2 bag
  info    Inspect and print ROS 2 bag summary without uploading
  init    Initialize or update default Webhook / rsync target configuration
  help    Print help information

Options:
      --init <URL>              Save Discord Webhook URL
      --init-rsync <TARGET>     Save default rsync remote target
      --config                  Show current configuration
  -t, --to <REMOTE_TARGET>      Send to remote machine via rsync (e.g. user@host:/dir)
  -f, --file <BAG_PATH>         Direct send of an existing bag
  -m, --message <TEXT>          Custom comment to include in notifications
  -k, --keep                    Keep compressed .tar.zst archive locally
      --dry-run                 Parse and compress without transmitting
  -h, --help                    Print help
  -V, --version                 Print version
```

## Configuration

Manage settings dynamically via CLI or editor:

```bash
# View current configuration
bp config

# Set individual keys (webhook, rsync, max_size, zstd)
bp config set rsync user@gpu-server:/data/bags
bp config set webhook "https://discord.com/api/webhooks/..."
bp config set max_size 50
bp config set zstd 19          # Set manual level (1-22) or 'auto'

# Get specific configuration value
bp config get rsync

# Open config file directly in your default $EDITOR (e.g. nano/vim)
bp config edit

# Reset configuration to default values
bp config reset
```

Configuration file is stored at `~/.config/bagpipe/config.json`:

```json
{
  "webhook_url": "https://discord.com/api/webhooks/...",
  "rsync_target": "user@server:/path/to/bags",
  "max_file_size_mb": 25,
  "zstd_level": null
}
```

- `webhook_url`: Discord Webhook URL.
- `rsync_target`: Default remote server destination (`user@host:/path`).
- `max_file_size_mb`: File upload size threshold in MB for Discord (default: 25).
- `zstd_level`: `null` for smart adaptive auto-tuning, or an integer from 1 to 22.

## License

MIT OR Apache-2.0
