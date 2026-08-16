# lla File Copier Plugin

A plugin for `lla` that provides an intuitive clipboard-based interface for copying files and directories.

## Features

- **Clipboard Management**: Persistent clipboard for files and directories
- **Interactive Selection**: Multi-select interface for files and operations
- **Flexible Copying**: Copy all or selected items from clipboard
- **Path Flexibility**: Support for both current and specified directories
- **Safe Operations**: Validation and error handling for copy operations
- **User Interface**: Colored output and interactive menus

## Configuration

Config file: `~/.config/lla/cp_clipboard.json`

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
lla plugin run file_copier add

# Add files from a specific directory to clipboard
lla plugin run file_copier add -- /path/to/source

# Copy all files from clipboard to current directory
lla plugin run file_copier copy-all

# Copy all files from clipboard to specific directory
lla plugin run file_copier copy-all -- /path/to/destination

# Copy selected files from clipboard to current directory
lla plugin run file_copier copy-partial

# Copy selected files from clipboard to specific directory
lla plugin run file_copier copy-partial -- /path/to/destination
```

### Clipboard Management

```bash
# View clipboard contents with option to remove items
lla plugin run file_copier show

# Clear the clipboard
lla plugin run file_copier clear

# Show help information
lla plugin run file_copier help
```

## Common Workflows

### 1. Copying Files Between Directories (Using Explicit Paths)

```bash
# Add files from source directory
lla plugin run file_copier add -- /path/to/source
# Select files to copy using space, confirm with enter

# Copy all files to target directory
lla plugin run file_copier copy-all -- /path/to/target
```

### 2. Copying Files Using Current Directory Navigation

```bash
# In source directory
cd /path/to/source
lla plugin run file_copier add
# Select files to add to clipboard

# Navigate to first target
cd /path/to/target1
lla plugin run file_copier copy-partial
# Select subset of files to copy here

# Navigate to second target
cd /path/to/target2
lla plugin run file_copier copy-partial
# Select another subset of files to copy here
```

### 3. Mixed Workflow (Current and Explicit Paths)

```bash
# Add files from current directory
lla plugin run file_copier add
# Select files to add to clipboard

# Copy selected files to a specific directory without changing location
lla plugin run file_copier copy-partial -- /path/to/target
```

## Display Format

```
─────────────────────────────────────
 File Copier Clipboard
─────────────────────────────────────
 Current Items:
   → /path/to/file1.txt
   → /path/to/directory1
   → /path/to/file2.rs
─────────────────────────────────────
 Use Space to select, Enter to confirm
─────────────────────────────────────
```
