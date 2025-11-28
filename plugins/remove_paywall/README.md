# Remove Paywall Plugin

Remove paywalls from URLs using various bypass services like 12ft.io, archive.is, and more.

## Features

- **Multiple Services**: Support for 12ft.io, archive.is, RemovePaywall.com, Freedium, and Google Cache
- **Clipboard Integration**: Automatically read URLs from your clipboard
- **Auto-open Browser**: Optionally open bypassed URLs directly in your browser
- **History Tracking**: Keep track of URLs you've processed
- **Service Selection**: Choose the best service for each article type

## Supported Services

| Service | Best For |
|---------|----------|
| **12ft.io** | News sites (NYT, WaPo, WSJ, etc.) |
| **archive.is** | General articles, creating permanent archives |
| **RemovePaywall.com** | News articles, subscription sites |
| **Freedium** | Medium articles only |
| **Google Cache** | Recently indexed pages |

## Installation

```bash
lla install
# Then enable the plugin
lla use  # Select remove_paywall
```

## Usage

### Remove Paywall from URL
```bash
lla plugin remove_paywall remove https://example.com/paywalled-article
```

### Remove Paywall from Clipboard URL
```bash
# Copy a URL first, then:
lla plugin remove_paywall clipboard
```

### Choose Service Interactively
```bash
lla plugin remove_paywall choose
lla plugin remove_paywall choose https://example.com/article
```

### Generate Links for All Services
```bash
lla plugin remove_paywall try-all
lla plugin remove_paywall try-all https://example.com/article
```

### Use Specific Services
```bash
lla plugin remove_paywall 12ft https://nytimes.com/article
lla plugin remove_paywall archive https://wsj.com/article
lla plugin remove_paywall freedium https://medium.com/article
```

### List Available Services
```bash
lla plugin remove_paywall services
```

### View History
```bash
lla plugin remove_paywall history
```

### Configure Preferences
```bash
lla plugin remove_paywall preferences
```

### Interactive Menu
```bash
lla plugin remove_paywall menu
```

### Show Help
```bash
lla plugin remove_paywall help
```

## Configuration

The plugin stores its configuration in `~/.config/lla/remove_paywall/config.toml`:

```toml
default_service = "TwelveFt"    # Default bypass service
auto_open_browser = true        # Open URL in browser automatically
copy_to_clipboard = true        # Copy bypass URL to clipboard
remember_history = true         # Save usage history
max_history_size = 50           # Maximum history entries

[colors]
success = "bright_green"
info = "bright_cyan"
warning = "bright_yellow"
error = "bright_red"
url = "bright_blue"
service = "bright_magenta"
```

## Output Example

```
🔓 Removing Paywall
────────────────────────────────────────────────────────────────

   Original URL:  https://nytimes.com/2024/01/article.html
   Service:       12ft.io
   Bypass URL:    https://12ft.io/https://nytimes.com/2024/01/article.html

   📋 Bypass URL copied to clipboard!
   🌐 Opened in browser!

────────────────────────────────────────────────────────────────
```

## Tips

1. **For News Sites**: Use 12ft.io or RemovePaywall.com
2. **For Medium Articles**: Use Freedium (designed specifically for Medium)
3. **For Creating Archives**: Use archive.is to create a permanent snapshot
4. **When Others Fail**: Try Google Cache for recently indexed pages

## Disclaimer

This plugin is provided for educational purposes. Please respect content creators and consider subscribing to publications you regularly read.

