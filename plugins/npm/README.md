# lla NPM Plugin

NPM package search plugin for `lla` with bundlephobia integration and favorites management.

## Features

- **Package Search**: Search npm registry for package information
- **Bundle Size**: View bundlephobia.com size metrics (minified & gzipped)
- **Favorites**: Manage favorite packages
- **Clipboard Integration**: Copy install commands

## Usage

```bash
# Search for npm packages
lla plugin run npm search

# View favorite packages
lla plugin run npm favorites

# Configure preferences (package manager)
lla plugin run npm preferences

# Show help
lla plugin run npm help
```

## Configuration

Config location: `~/.config/lla/plugins/npm/config.toml`

```toml
favorites = []                          # List of favorite packages
registry = "https://registry.npmjs.org" # NPM registry URL
package_manager = "npm"                 # Package manager (npm, yarn, pnpm, bun)

[colors]
success = "bright_green"
info = "bright_cyan"
warning = "bright_yellow"
error = "bright_red"
package = "bright_blue"
version = "bright_magenta"
```

## Display Examples

Package Details:

```
────────────────────────────────────────────────────────────
📦 react v18.2.0
────────────────────────────────────────────────────────────
Description: React is a JavaScript library for building UIs
Author: Meta
License: MIT
Homepage: https://reactjs.org/

Bundle Size:
  Minified: 42.5 KB
  Gzipped: 13.2 KB
────────────────────────────────────────────────────────────
```

Favorites List:

```
⚡ Choose action
> 📦 View package details
  📋 Copy install commands
  🗑️  Remove from favorites
  ← Back
```
