# bagpipe (`bp`)

Record, zstd-compress, and ship ROS 2 bags via **rsync** and/or **Discord** in one breath.

## Install

```bash
cargo install bagpipe-ros
```

## Quick Start

### 1. Set destinations (run once)

```bash
bp rsync=user@server:/path/to/bags
bp webhook="https://discord.com/api/webhooks/..."
```

### 2. Record & auto-ship on Ctrl+C

```bash
bp -a
bp /camera/image_raw /cmd_vel -m "field test"
```

### 3. Ship existing bags

```bash
bp                    # auto-detect and ship latest bag
bp ./my_rosbag_dir    # ship specific bag
```

### 4. Inspect bag metadata

```bash
bp info               # print topics, messages, duration
```

## Toggle & Configuration

```bash
# Enable / disable destinations
bp discord=on / off
bp rsync=on / off

# Ad-hoc disable for a single run
bp -a --no-discord
bp -a --no-rsync

# Check / customize settings
bp config
bp zstd=19            # manual compression level (1-22 or auto)
bp config reset
```

## License

MIT OR Apache-2.0
