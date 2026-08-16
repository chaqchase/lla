# lla Duplicate File Detector Plugin

A plugin for `lla` that identifies identical files using secure hash comparison.

## Features

- SHA-256 content hashing with intelligent caching
- Original file and duplicate chain tracking
- Color-coded status display
- Performance optimized with chunk-based processing

## Configuration

Located at `~/.config/lla/duplicate_file_detector/config.toml`:

```toml
[colors]
duplicate = "bright_red"        # Duplicate file indicator
has_duplicates = "bright_yellow"# Original file with duplicates
path = "bright_cyan"           # File path display
success = "bright_green"       # Success messages
info = "bright_blue"          # Information messages
name = "bright_yellow"        # Name highlighting
```

## Usage

```bash
# Clear the detection cache
lla plugin run duplicate_file_detector clear-cache

# View help information
lla plugin run duplicate_file_detector help
```

## Display Formats

### Default Format

```
file.txt
Status: DUPLICATE of /path/to/original.txt

other.txt
Status: HAS DUPLICATES: /path/to/copy1.txt, /path/to/copy2.txt
```

### Long Format

```
file.txt
Status:       DUPLICATE
Original File: /path/to/original.txt

other.txt
Status:          HAS DUPLICATES
Duplicate Copies: /path/to/copy1.txt
                 /path/to/copy2.txt
```

## Technical Details

- Uses SHA-256 hashing with 8KB chunk-based reading
- Implements efficient caching with automatic invalidation
- Thread-safe operations
- Color-coded status indicators for duplicates and originals
