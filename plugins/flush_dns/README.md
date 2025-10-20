# lla Flush DNS Plugin

DNS cache flushing plugin for `lla` with history tracking and cross-platform support.

## Features

- **Cross-Platform**: Works on macOS, Linux, and Windows
- **History Tracking**: Track all flush operations with timestamps
- **Confirmation Prompts**: Optional confirmation before flushing
- **Statistics**: View flush history and success rates

## Usage

```bash
# Flush DNS cache
lla plugin --name flush_dns --action flush

# View flush history
lla plugin --name flush_dns --action history

# Clear history
lla plugin --name flush_dns --action clear-history

# Configure preferences
lla plugin --name flush_dns --action preferences

# Show help
lla plugin --name flush_dns --action help
```

## Configuration

Config location: `~/.config/lla/plugins/flush_dns/config.toml`

```toml
confirm_before_flush = true      # Show confirmation prompt
show_verbose_output = true       # Display command output
max_history_size = 50           # Maximum history entries

[colors]
success = "bright_green"
info = "bright_cyan"
warning = "bright_yellow"
error = "bright_red"
```

## Display Examples

Flushing DNS:

```
ℹ Info: Flushing DNS cache on macOS...
✓ Success: DNS cache flushed successfully!
```

History View:

```
📜 DNS Flush History:
──────────────────────────────────────────────────────────────────────
 ✓ 2025-10-20 16:30:45 [macOS] Success
 ✓ 2025-10-20 15:20:30 [macOS] Success
 ✗ 2025-10-20 14:15:22 [macOS] Failed
──────────────────────────────────────────────────────────────────────

📊 Statistics:
 • Total flushes: 25
 • Successful: 24
 • Failed: 1
```

## Platform Commands

- **macOS**: `sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder`
- **Linux**: `sudo systemd-resolve --flush-caches` or `sudo systemctl restart nscd`
- **Windows**: `ipconfig /flushdns`
