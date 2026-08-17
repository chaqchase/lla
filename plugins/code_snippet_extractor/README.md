# lla Code Snippet Extractor Plugin

A plugin for `lla` that extracts, organizes, and manages code snippets with metadata and search capabilities.

## Features

- **Smart Extraction**: Automatic language detection, contextual extraction
- **Organization**: Categories, tags, metadata tracking
- **Search**: Fuzzy search, multi-select operations
- **Interface**: Syntax highlighting, interactive CLI menus
- **Import/Export**: JSON-based snippet management

## Configuration

Config file: `~/.config/lla/code_snippets/config.toml`

```toml
[colors]
success = "bright_green"
info = "bright_blue"
error = "bright_red"
name = "bright_yellow"
language = "bright_cyan"
tag = "bright_magenta"

[syntax_themes]
default = "Solarized (dark)"
```

## Usage

### Basic Operations

```bash
# Extract snippet with context
lla plugin run code_snippet_extractor extract -- "file.rs" "function_name" 10 20 3

# List snippets
lla plugin run code_snippet_extractor list

# View snippet
lla plugin run code_snippet_extractor get -- "snippet_id"
```

### Organization

```bash
# Add/remove tags
lla plugin run code_snippet_extractor add-tags -- "snippet_id" "tag1" "tag2"
lla plugin run code_snippet_extractor remove-tags -- "snippet_id" "tag1"

# Category management
lla plugin run code_snippet_extractor set-category -- "snippet_id" "category_name"
```

### Import/Export

```bash
# Export/Import snippets
lla plugin run code_snippet_extractor export -- "snippets.json"
lla plugin run code_snippet_extractor import -- "snippets.json"
```

## Display Format

```
─────────────────────────────────────
 Example Function
 ID: abc123  •  Language: rust  •  Version: v1
─────────────────────────────────────
 📂 Source: src/example.rs
 🏷️  Tags: #rust #function #example
 📁 Category: Algorithms
 🕒 Created: 2024-01-20 10:30:00 UTC
─────────────────────────────────────
 ◀ Context (3 lines)
    1 │ // Helper functions
 ▶ Code (5 lines)
    4 │ fn parse_input<T: FromStr>(input: &str) -> Option<T> {
    5 │     input.trim().parse().ok()
    6 │ }
 ▼ Context (2 lines)
   10 │ // Example usage
─────────────────────────────────────
```

## Language Support

Supports common languages: Rust, Python, JavaScript, TypeScript, Go, C/C++, Java, Ruby, PHP, Shell, HTML, CSS, Markdown, JSON, YAML, XML, SQL
