# lla YouTube Plugin

YouTube search plugin for `lla` with live autosuggestions, history management, and clipboard support.

## Features

- **Live Autocomplete**: Real-time search suggestions from YouTube API as you type
- **Loading States**: Visual spinner while fetching suggestions
- **Smart Search**: Multiple input options (live suggestions, history, clipboard)
- **Search Selected Text**: Instantly search using text from clipboard
- **History Management**: Persistent search history with statistics

## Usage

```bash
# Perform a YouTube search
lla plugin --name youtube --action search

# Search with selected/clipboard text
lla plugin --name youtube --action search-selected

# Manage search history
lla plugin --name youtube --action history

# Configure preferences
lla plugin --name youtube --action preferences

# Show help
lla plugin --name youtube --action help
```

## Configuration

Config location: `~/.config/lla/plugins/youtube/config.toml`

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
link = "bright_magenta"
```

## Display Examples

Live Autocomplete:

```
💡 Enter a search query to see live YouTube suggestions
Search query: rust tutorial

🔄 Fetching suggestions from YouTube...
⠋ Loading suggestions...

✨ 10 suggestions found:
Select a search query
> 🎬 rust tutorial (your input)
  💡 rust tutorial for beginners
  💡 rust tutorial 2025
  💡 rust tutorial game
  💡 rust tutorial programming
  ...
```

History Statistics:

```
📊 Search History Statistics:
──────────────────────────────────────────────────
 • Total searches: 42
 • Unique queries: 28
 • Oldest search: 2025-10-15 09:30:00
 • Most recent: 2025-10-20 16:15:30

🔥 Top 5 searches:
 • rust tutorial (8x)
 • golang explained (5x)
 • docker basics (3x)
```
