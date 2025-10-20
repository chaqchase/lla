# lla Google Search Plugin

Google search plugin for `lla` with live autosuggestions, history management, and clipboard fallback.

## Features

- **Live Autocomplete**: Real-time search suggestions from Google API with loading states
- **Smart Search**: Multiple input options (live suggestions, history, clipboard)
- **History Management**: Persistent search history with statistics and analytics
- **Interactive Interface**: Rich TUI with visual feedback

## Usage

```bash
# Perform a Google search
lla plugin --name google_search --action search

# Manage search history
lla plugin --name google_search --action history

# Configure preferences
lla plugin --name google_search --action preferences

# Show help
lla plugin --name google_search --action help
```

## Configuration

Config location: `~/.config/lla/plugins/google_search/config.toml`

```toml
remember_search_history = true    # Enable/disable history persistence
use_clipboard_fallback = true     # Enable/disable clipboard fallback
max_history_size = 100           # Maximum history entries

[colors]
success = "bright_green"
info = "bright_cyan"
warning = "bright_yellow"
error = "bright_red"
prompt = "bright_blue"
```

## Display Examples

Live Autocomplete:

```
💡 Enter a search query to see live Google suggestions
Search query: rust programming

🔄 Fetching suggestions from YouTube...
⠋ Loading suggestions...

✨ 10 suggestions found:
Select a search query
> 🔍 rust programming (your input)
  💡 rust programming tutorial
  💡 rust programming language
  💡 rust programming for beginners
  💡 rust programming projects
  ...
```

History Statistics:

```
📊 Search History Statistics:
──────────────────────────────────────────────────
 • Total searches: 25
 • Unique queries: 18
 • Oldest search: 2025-10-15 09:30:00
 • Most recent: 2025-10-20 14:30:45

🔥 Top 5 searches:
 • rust programming tutorial (5x)
 • golang best practices (3x)
 • python async await (2x)
```
