# lla Keyword Search Plugin

High-performance keyword search plugin for `lla` with interactive search and rich display features.

## Features

- **Smart Search**: Multi-keyword, case-sensitive, regex support
- **Interactive**: File selection, filtering, action menu
- **Rich Display**: Syntax highlighting, context visualization
- **Analysis**: Match statistics and pattern detection

## Usage

```bash
# Search in current directory
lla plugin run keyword_search search

# Available actions after finding matches:
1. View detailed matches
2. Copy to clipboard
3. Save to file
4. Show statistics
5. Filter matches
6. Advanced analysis
```

## Configuration

Config location: `~/.config/lla/keyword_search/config.toml`

```toml
keywords = []               # Keywords to search for
case_sensitive = false      # Case sensitivity
use_regex = false          # Regular expression support
context_lines = 2          # Number of context lines
max_matches = 5            # Maximum matches per file

[colors]
keyword = "bright_red"
line_number = "bright_yellow"
context = "bright_black"
file = "bright_blue"
success = "bright_green"
info = "bright_cyan"
```

## Display Examples

Match View:

```
─────────────────────────────────
 📂 src/main.rs
─────────────────────────────────
 123 │ function process() {
 124 │ let data = analyze();
►125 │ // TODO: Implement error handling
 126 │ return data;
 127 │ }
─────────────────────────────────
```

Statistics View:

```
📊 Match Statistics:
─────────────────────────────────
 • Total matches: 5
 • Unique keywords: 2
 • Average context: 2.5 lines
 • File: src/main.rs
─────────────────────────────────
```
