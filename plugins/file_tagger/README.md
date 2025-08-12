# lla File Tagger Plugin

A file tagging plugin for `lla` that provides persistent tag management.

## Features

- Add, remove, and list file tags
- Persistent storage with efficient lookup
- Color-coded tag display
- Interactive commands
- List all tags across files
- Query files by tag

## Configuration

Config file: `~/.config/lla/file_tagger/config.toml`

```toml
[colors]
tag = "bright_cyan"        # Tag text
tag_label = "bright_green" # Tag label
success = "bright_green"   # Success messages
info = "bright_blue"      # Info messages
name = "bright_yellow"    # Name highlighting
```

## Storage

Persistent tag data is stored at: `~/.config/lla/file_tags.txt`

## Usage

```bash
# Add tag
lla plugin --name file_tagger --action add-tag --args "/path/to/file" "important"

# Remove tag
lla plugin --name file_tagger --action remove-tag --args "/path/to/file" "important"

# List tags
lla plugin --name file_tagger --action list-tags --args "/path/to/file"

# List all tags
lla plugin --name file_tagger --action all-tags

# List files by tag
lla plugin --name file_tagger --action files-by-tag --args "important"

# Help
lla plugin --name file_tagger --action help
```

### Display Examples

Default format:

```
document.pdf
Tags: [important] [work] [urgent]
```

Long format:

```
document.pdf
Tag: important
Tag: work
Tag: urgent
```
