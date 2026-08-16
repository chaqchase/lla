# lla Directory Metadata Plugin

Real-time directory statistics with intelligent caching.

## Features

- **Analysis**: Parallel scanning, caching, configurable depth
- **Statistics**: File counts, sizes, subdirectories, modification times
- **Performance**: Multi-threaded, cache-optimized

## Configuration

`~/.config/lla/dirs_meta/config.toml`:

```toml
cache_size = 1000           # Max cached directories
max_scan_depth = 100        # Max scan depth
parallel_threshold = 1000    # Min entries for parallel

[colors]
files = "bright_cyan"
dirs = "bright_green"
size = "bright_yellow"
time = "bright_magenta"
```

## Usage

```bash
# Show stats
lla plugin run dirs_meta stats -- "/path/to/directory"

# Clear cache
lla plugin run dirs_meta clear-cache
```

## Display Formats

Default: `Documents (15 files, 2.5 GB)`

Long:

```
Documents
Files:       15
Directories: 3
Total Size:  2.5 GB
Modified:    5 mins ago
```

Units: B/KB/MB/GB, seconds/minutes/hours/days ago
