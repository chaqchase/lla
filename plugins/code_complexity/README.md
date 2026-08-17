# lla Code Complexity Plugin

A code analysis plugin for `lla` that performs real-time complexity analysis of source code.

## Features

- **Multi-Metric Analysis**
  - Cyclomatic & Cognitive Complexity
  - Maintainability Index (0-100)
  - Function/Class Analysis
  - Control Flow & Volume Metrics
- **Smart Thresholds**: Configurable with color-coding
- **Real-Time Statistics**: Continuous metric tracking
- **Detailed Reports**: File and language-level insights

## Default Configuration

### Complexity Thresholds

- Low: < 10.0
- Medium: < 20.0
- High: < 30.0
- Very High: ≥ 40.0

### Language Support (Default: Rust)

- Function: `fn`
- Class: `struct`, `impl`, `trait`
- Branch: `if`, `match`, `else`
- Loop: `for`, `while`, `loop`
- Comments: `//`, `/*`
- Max Line Length: 100
- Max Function Length: 50 lines

## Usage

```bash
# Set complexity thresholds
lla plugin run code_complexity set-thresholds -- 10 20 30 40

# Show report
lla plugin run code_complexity show-report
```

## Display Formats

### Default

```
[Complexity: 12 (MI: 85.3)]
```

### Long

```
[Complexity: 12 (MI: 85.3)]
├── Lines: 150
├── Functions: 5
├── Classes: 2
├── Branches: 8
├── Loops: 4
├── Comments: 20
└── Long functions:
    ├── process_data (55 lines)
    └── analyze_results (60 lines)
```

## Configuration

Config file: `~/.config/lla/code_complexity/config.toml`

### Language Settings

```toml
[languages.Rust]
extensions = ["rs"]
function_patterns = ["fn "]
class_patterns = ["struct ", "impl ", "trait "]
branch_patterns = ["if ", "match ", "else"]
loop_patterns = ["for ", "while ", "loop"]
comment_patterns = ["//", "/*"]
max_line_length = 100
max_function_lines = 50
```

### Thresholds

```toml
[thresholds]
low = 10.0
medium = 20.0
high = 30.0
very_high = 40.0
```

### Colors

```toml
[colors]
low = "bright_green"
medium = "bright_yellow"
high = "bright_red"
very_high = "red"
```
