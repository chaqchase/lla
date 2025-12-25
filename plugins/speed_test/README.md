# Speed Test Plugin

Internet speed test plugin for lla - test download speeds and latency from multiple servers.

## Features

- **Full Speed Test**: Test download speed from multiple CDN servers (Cloudflare, Google, GitHub)
- **Latency Testing**: Measure ping latency to multiple DNS providers
- **Quick Test**: Fast single-server speed test
- **History Tracking**: Keep track of past speed test results
- **Speed Rating**: Get a rating based on your connection speed

## Installation

```bash
lla install
# Then enable the plugin
lla use  # Select speed_test
```

## Usage

### Run a Full Speed Test
```bash
lla plugin speed_test test
```

### Run a Quick Speed Test
```bash
lla plugin speed_test quick
```

### View Speed Test History
```bash
lla plugin speed_test history
```

### Clear History
```bash
lla plugin speed_test clear-history
```

### Interactive Menu
```bash
lla plugin speed_test menu
```

### Show Help
```bash
lla plugin speed_test help
```

## Configuration

The plugin stores its configuration in `~/.config/lla/speed_test/config.toml`:

```toml
remember_history = true      # Save test results to history
max_history_size = 50        # Maximum history entries to keep
test_size_mb = 10            # Test file size in MB

[colors]
success = "bright_green"
info = "bright_cyan"
warning = "bright_yellow"
error = "bright_red"
speed_fast = "bright_green"
speed_medium = "bright_yellow"
speed_slow = "bright_red"
```

## Output Example

```
🚀 Running Internet Speed Test...
──────────────────────────────────────────────────

📡 Testing latency...
   • Cloudflare DNS: 12ms
   • Google DNS: 15ms
   • OpenDNS: 18ms

⬇️  Testing download speed...
   • Cloudflare: 85.42 Mbps
   • Google: 72.31 Mbps
   • GitHub: 68.15 Mbps

──────────────────────────────────────────────────
📊 Results Summary
──────────────────────────────────────────────────
   Download Speed: 85.42 Mbps
   Best Server: Cloudflare
   Latency: 12ms
   Fastest DNS: Cloudflare DNS

   Rating: ✨ Very Good

✓ Test completed!
```

