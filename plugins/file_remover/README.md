# lla File Remover Plugin

A plugin for `lla` that provides an interactive interface for recoverable deletion. The
default `remove` action moves entries into the shared `trash` store. Permanent deletion
is available only through the explicit `purge` action and a second confirmation.

## Features

- **Interactive Selection**: Multi-select interface for choosing files to remove
- **Path Flexibility**: Support for both current and specified directories
- **Recoverable by Default**: `remove` records original paths and moves entries to trash
- **Explicit Permanent Deletion**: `purge` is deliberately separate and irreversible
- **Directory Support**: Files, symlinks, and complete directory trees are supported
- **User Interface**: Colored output and interactive menus

## Configuration

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
# Move files/directories from the current directory to recoverable trash
lla plugin --name file_remover --action remove

# Move files/directories from a specified directory to recoverable trash
lla plugin --name file_remover --action remove --args /path/to/directory

# Permanently delete selected entries (cannot be restored)
lla plugin --name file_remover --action purge --args /path/to/directory

# Show help information
lla plugin --name file_remover --action help
```

## Common Workflows

### 1. Trashing Files from Current Directory

```bash
# In target directory
cd /path/to/directory
lla plugin --name file_remover --action remove
# Select files to trash using space, confirm with enter
```

### 2. Trashing Files from Specific Directory

```bash
# Trash files from a specific directory without changing location
lla plugin --name file_remover --action remove --args /path/to/directory
# Select files to trash using space, confirm with enter
```

## Display Format

```
─────────────────────────────────────
 File Remover
─────────────────────────────────────
 Select items to remove:
   → file1.txt
   → directory1
   → file2.rs
─────────────────────────────────────
 Use Space to select, Enter to confirm
─────────────────────────────────────
```
