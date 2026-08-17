# lla Google Search Plugin

Google search plugin for `lla` with live autosuggestions, history management, and clipboard fallback.

## Features

- **Live Autocomplete**: Real-time search suggestions from Google API with loading states
- **Smart Search**: Multiple input options (live suggestions, history, clipboard)
- **History Management**: Persistent search history with statistics and analytics
- **Interactive Interface**: Rich TUI with visual feedback

## Usage

```bash
# Perform a Google search (live suggestions)
lla plugin run google_search search

# Search with selected/clipboard text (prefills input; live suggestions)
lla plugin run google_search search-selected

# Manage search history
lla plugin run google_search history

# Configure preferences
lla plugin run google_search preferences

# Show help
lla plugin run google_search help
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

Live Autocomplete (use ↑/↓ to select, Enter to confirm):

```
💡 Enter a search query to see live Google suggestions
Search query: rust programming

🔄 Fetching suggestions from Google...
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
