# Hacker News Plugin

Browse Hacker News directly from your terminal - view top stories, best stories, new stories, Ask HN, Show HN, and job postings.

## Features

- **Multiple Topics**: Browse Top, Best, New, Ask HN, Show HN, and Jobs
- **Story Details**: View scores, comment counts, authors, and time posted
- **Interactive Browser**: Navigate and interact with stories using keyboard
- **Quick Actions**: Open articles, view comments, or copy URLs
- **Smart Caching**: Responses cached for 5 minutes to reduce API calls
- **Domain Extraction**: See which domain each story links to

## Installation

```bash
lla install
# Then enable the plugin
lla use  # Select hackernews
```

## Usage

### View Stories by Topic

```bash
lla plugin hackernews top      # Top stories (front page)
lla plugin hackernews best     # Best stories
lla plugin hackernews new      # Newest stories
lla plugin hackernews ask      # Ask HN
lla plugin hackernews show     # Show HN
lla plugin hackernews jobs     # Job postings
```

### Open a Story

```bash
# After viewing a list, open story #1
lla plugin hackernews open 1
```

### View Comments

```bash
# Open HN comments for story #1
lla plugin hackernews comments 1
```

### Copy Story URL

```bash
# Copy URL of story #1 to clipboard
lla plugin hackernews copy 1
```

### Interactive Browser

```bash
lla plugin hackernews browse       # Browse default topic
lla plugin hackernews browse top   # Browse specific topic
```

### Interactive Menu

```bash
lla plugin hackernews menu
```

### Clear Cache

```bash
lla plugin hackernews clear-cache
```

### Show Help

```bash
lla plugin hackernews help
```

## Configuration

The plugin stores its configuration in `~/.config/lla/hackernews/config.toml`:

```toml
default_topic = "Top"           # Default topic to show
story_count = 30                # Number of stories to fetch
cache_duration_secs = 300       # Cache duration (5 minutes)

[colors]
success = "bright_green"
info = "bright_cyan"
title = "bright_white"
score = "bright_yellow"
comments = "bright_cyan"
domain = "bright_blue"
time = "bright_black"
author = "bright_magenta"
```

## Output Example

```
🔥 Top Stories (30 stories)
────────────────────────────────────────────────────────────────────────────────
  1. Show HN: I built a terminal file manager in Rust (github.com)
     ▲342 💬89 • by rustdev 3h ago

  2. The decline of the American mall (nytimes.com)
     ▲256 💬145 • by journalist 5h ago

  3. PostgreSQL 17 Released (postgresql.org)
     ▲198 💬67 • by pgfan 2h ago

  4. Ask HN: What's your favorite CLI tool? (news.ycombinator.com)
     ▲167 💬234 • by curious_dev 4h ago

  5. Why I switched from VS Code to Neovim (medium.com)
     ▲134 💬89 • by vimuser 6h ago
────────────────────────────────────────────────────────────────────────────────
   ℹ️  Use 'open <number>' to open a story in your browser
```

## Interactive Browser

The interactive browser (`browse` action) provides a keyboard-driven interface:

1. Use arrow keys to navigate stories
2. Press Enter to select a story
3. Choose to:
   - 🔗 Open article in browser
   - 💬 Open HN comments
   - 📋 Copy URL to clipboard
4. Use menu options to:
   - 🔄 Refresh stories
   - 📋 Change topic
   - ← Go back

## API Information

This plugin uses the official [Hacker News API](https://github.com/HackerNews/API):
- Stories are fetched in batches for better performance
- Results are cached locally for 5 minutes
- No authentication required

## Tips

1. **Stay Updated**: Use `top` for trending stories, `new` for the latest
2. **Job Hunting**: The `jobs` topic shows YC startup job postings
3. **Quick Browse**: Use `browse` for a keyboard-driven experience
4. **Clear Cache**: Use `clear-cache` if stories seem stale

