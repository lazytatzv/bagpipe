# bagpipe (`bp`)

Record, zstd-compress, ship, and unpack ROS 2 bags via **Direct Stream (LAN / Tailscale)**, **rsync**, and **Discord** in one breath.

## Install

```bash
cargo install bagpipe-ros
```

## Quick Start

### 1. High-Speed Direct Stream (Tailscale / LAN — Zero SSH Overhead)

On Receiver (Development PC / Server):
```bash
bp listen              # listen for incoming streams & auto-extract
bp listen --play       # listen & immediately start `ros2 bag play` on arrival
```

On Sender (Robot):
```bash
# Record & stream directly to Tailscale MagicDNS name or IP at wire speed
bp -a -t my-desktop
bp -a -t 100.64.0.12

# Stream existing bag
bp -t my-desktop
```

### 2. Set default destinations (run once)

```bash
bp rsync=user@server:/path/to/bags   # or bp to=my-desktop
bp webhook="https://discord.com/api/webhooks/..."
```

### 3. Record & auto-ship on Ctrl+C

```bash
bp -a
bp /camera/image_raw /cmd_vel -m "field test"
```

### 4. Ship existing bags

```bash
bp                    # auto-detect and ship latest bag
bp ./my_rosbag_dir    # ship specific bag
```

### 5. Unpack & Play (Receiving side)

```bash
bp ./my_bag.tar.zst   # auto-extract archive & print summary
bp unpack             # unpack latest .tar.zst in current directory
bp play               # extract (if compressed) & play via `ros2 bag play`
bp play --loop -r 2.0 # pass transparent args to `ros2 bag play`
```

### 6. Inspect bag metadata

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
