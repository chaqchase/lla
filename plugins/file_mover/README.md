# lla File Mover Plugin

A plugin for `lla` that provides an intuitive clipboard-based interface for moving files and directories.

## Features

- **Clipboard Management**: Persistent clipboard for files and directories
- **Interactive Selection**: Multi-select interface for files and operations
- **Flexible Moving**: Move all or selected items from clipboard
- **Path Flexibility**: Support for both current and specified directories
- **Safe Operations**: Validation and error handling for move operations
- **User Interface**: Colored output and interactive menus

## Configuration

Config file: `~/.config/lla/mv_clipboard.json`

```toml
[colors]
success = "bright_green"
info = "bright_blue"
error = "bright_red"
path = "bright_yellow"
```

## Usage

### Basic Operations

```bash
# Add files from current directory to clipboard
lla plugin run file_mover add

# Add files from a specific directory to clipboard
lla plugin run file_mover add -- /path/to/source

# Move all files from clipboard to current directory
lla plugin run file_mover move-all

# Move all files from clipboard to specific directory
lla plugin run file_mover move-all -- /path/to/destination

# Move selected files from clipboard to current directory
lla plugin run file_mover move-partial

# Move selected files from clipboard to specific directory
lla plugin run file_mover move-partial -- /path/to/destination
```

### Clipboard Management

```bash
# View clipboard contents with option to remove items
lla plugin run file_mover show

# Clear the clipboard
lla plugin run file_mover clear

# Show help information
lla plugin run file_mover help
```

## Common Workflows

### 1. Moving Files Between Directories (Using Explicit Paths)

```bash
# Add files from source directory
lla plugin run file_mover add -- /path/to/source
# Select files to move using space, confirm with enter

# Move all files to target directory
lla plugin run file_mover move-all -- /path/to/target
```

### 2. Moving Files Using Current Directory Navigation

```bash
# In source directory
cd /path/to/source
lla plugin run file_mover add
# Select files to add to clipboard

# Navigate to first target
cd /path/to/target1
lla plugin run file_mover move-partial
# Select subset of files to move here

# Navigate to second target
cd /path/to/target2
lla plugin run file_mover move-partial
# Select another subset of files to move here
```

### 3. Mixed Workflow (Current and Explicit Paths)

```bash
# Add files from current directory
lla plugin run file_mover add
# Select files to add to clipboard

# Move selected files to a specific directory without changing location
lla plugin run file_mover move-partial -- /path/to/target
```

## Display Format

```
─────────────────────────────────────
 File Mover Clipboard
─────────────────────────────────────
 Current Items:
   → /path/to/file1.txt
   → /path/to/directory1
   → /path/to/file2.rs
─────────────────────────────────────
 Use Space to select, Enter to confirm
─────────────────────────────────────
```
