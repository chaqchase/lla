# lla File Metadata Plugin

A file metadata plugin for `lla` that provides comprehensive file information with rich formatting.

## Features

- Timestamp tracking (access, modify, create)
- Ownership and permission details
- Size statistics
- Color-coded information display

## Configuration

Located at `~/.config/lla/file_meta/config.toml`:

```toml
[colors]
accessed = "bright_blue"      # Access time color
modified = "bright_green"     # Modification time color
created = "bright_yellow"     # Creation time color
ownership = "bright_magenta"  # UID/GID information color
size = "bright_cyan"         # File size color
permissions = "bright_red"    # Permissions color
success = "bright_green"     # Success messages
info = "bright_blue"        # Information messages
name = "bright_yellow"      # Name highlighting
```

## Usage

```bash
# View help information
lla plugin run file_meta help
```

## Display Format

```
document.pdf
Accessed:    2024-03-15 14:30:22
Modified:    2024-03-15 14:30:20
Created:     2024-03-15 14:30:18
UID/GID:     1000/1000
Size:        1.0 MB
Permissions: 644
```
