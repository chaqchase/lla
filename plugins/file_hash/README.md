# lla File Hash Plugin

A high-performance file hashing plugin for `lla` that calculates secure cryptographic hashes (SHA-1 and SHA-256).

## Features

- SHA-1 and SHA-256 hash calculation
- One native decoration and formatting call per listing batch
- Persistent metadata-keyed digest cache
- Efficient 64 KiB buffered reads on cache misses
- Rich display formatting
- Sandboxed WASI Preview 2 execution
- Structured text output for plugin actions

## Configuration

Located at `~/.config/lla/plugins/file_hash/config.toml`:

```toml
[colors]
sha1 = "bright_green"     # SHA-1 hash color
sha256 = "bright_yellow"  # SHA-256 hash color
success = "bright_green"  # Success messages
info = "bright_blue"     # Information messages
name = "bright_yellow"   # Name highlighting

[cache]
enabled = true            # Reuse hashes when size and modification time match
max_entries = 10000       # Retain the most recently used entries
```

The cache is stored at `~/.config/lla/plugins/file_hash/cache.toml`. A file is
hashed again when its size or nanosecond modification timestamp changes. Delete
that cache file to force a complete rebuild; it is recreated automatically.

## Usage

```bash
# View help information
lla plugin run file_hash help

# First run populates the digest and Wasmtime compilation caches
lla -l ./directory

# The same listing should reuse both caches
lla -l ./directory
```

## Display Formats

### Default Format

```
document.pdf
SHA1:   a1b2c3d4
SHA256: e5f6g7h8
```

### Long Format

```
document.pdf
SHA1:   a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t0
SHA256: u1v2w3x4y5z6a7b8c9d0e1f2g3h4i5j6k7l8m9n0p1q2r3s4t5u6v7w8x9y0
```
