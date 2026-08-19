# bagpipe (`bp`)

Record, compress (multi-threaded zstd), ship at wire-speed, and auto-play ROS 2 bags.

## Install

```bash
cargo install bagpipe-ros
```

## Quick Start

### 1. Send & Stream (Sender / Robot)

Set default host once:
```bash
bp to=192.168.1.50     # or hostname (e.g. workstation)
```

Record & stream directly on `Ctrl+C`:
```bash
bp -a
bp /camera/image_raw /cmd_vel -m "test run 1"
```

Stream an existing bag:
```bash
bp                     # detect & stream latest bag
bp ./my_bag            # stream specific bag
```

### 2. Receive & Play (Receiver / Server)

Start receiver server:
```bash
bp server start        # run in background (daemon)
bp server stop         # stop background server
bp server status       # check server status
bp server              # run in foreground
```

Unpack or play received bags manually:
```bash
bp ./my_bag.tar.zst    # unpack & show summary
bp play                # unpack & immediately play with `ros2 bag play`
bp play --loop -r 2.0  # pass transparent args to `ros2 bag play`
```

### 3. Discord Notifications (Optional)

```bash
bp webhook="https://discord.com/api/webhooks/..."
bp discord=off / on    # toggle alerts
```

## Toggle & Configuration

```bash
# Enable / disable streaming and Discord independently
bp stream=off / on     # toggle direct network streaming
bp discord=off / on    # toggle Discord webhook upload

# Ad-hoc disable for a single run
bp -a --no-stream      # record & compress, but don't stream
bp -a --no-discord     # record & compress, but don't notify Discord

# Check / customize settings
bp config              # view all settings
bp zstd=19             # set compression level (1-22 or auto)
bp config reset        # reset to defaults
```


## License

MIT OR Apache-2.0
