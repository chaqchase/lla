# lla Categorizer Plugin

File categorization plugin for `lla` that organizes files based on extensions, with hierarchical categorization support.

## Features

- Automatic file categorization by extension with colored labels
- Hierarchical categories and subcategories
- Size-based rules and statistics tracking
- Configurable categories, colors, and rules

## Default Categories

### Documents

- Color: bright_blue
- Extensions: txt, md, doc, docx, pdf, rtf, odt
- Subcategories:
  - Text: txt, md
  - Office: doc, docx, xls, xlsx, ppt, pptx

### Code

- Color: bright_cyan
- Extensions: rs, py, js, ts, java, c, cpp, h, hpp, go, rb, php
- Subcategories:
  - Systems: rs, c, cpp, h, hpp
  - Web: js, ts, html, css, php
  - Scripts: py, rb, sh, bash

## Usage

```bash
# Add category
lla plugin run categorizer add-category -- "Images" "yellow" "jpg,png,gif"

# Add subcategory
lla plugin run categorizer add-subcategory -- "Images" "Raster" "jpg,png,gif"

# List categories
lla plugin run categorizer list-categories
```

## Configuration

Config file: `~/.config/lla/plugins/categorizer/config.toml`

- Category definitions and mappings
- Size rules and subcategories
- UI color schemes

## Display Formats

- **default**: `[Documents]`
- **long**: `[Documents] (Text)`
