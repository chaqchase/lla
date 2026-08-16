# Homebrew Plugin

Homebrew package manager plugin for lla - install, uninstall, upgrade, and search packages directly from your terminal.

## Features

- **List Installed Packages**: View all installed formulae and casks
- **Search Packages**: Fuzzy search through all available Homebrew packages
- **Check for Updates**: See which packages have available updates
- **Install/Uninstall**: Install or remove packages with a single command
- **Upgrade**: Upgrade individual packages or all outdated packages
- **Package Info**: View detailed information about any package
- **Cleanup**: Remove old versions and clear Homebrew cache
- **Doctor**: Run Homebrew diagnostics

## Installation

```bash
lla install
# Then enable the plugin
lla use  # Select brew
```

## Requirements

- Homebrew must be installed on your system
- Works on macOS and Linux

## Usage

### List Installed Packages
```bash
lla plugin run brew list
```

### Search for Packages
```bash
lla plugin run brew search -- git
lla plugin run brew search -- firefox
```

### Check for Updates
```bash
lla plugin run brew outdated
```

### Install a Package
```bash
lla plugin run brew install -- wget
lla plugin run brew install -- firefox --cask   # For cask applications
```

### Uninstall a Package
```bash
lla plugin run brew uninstall -- wget
```

### Upgrade Packages
```bash
lla plugin run brew upgrade --           # Upgrade all packages
lla plugin run brew upgrade -- wget      # Upgrade specific package
```

### View Package Info
```bash
lla plugin run brew info -- git
lla plugin run brew info -- visual-studio-code
```

### Cleanup Old Versions
```bash
lla plugin run brew cleanup
```

### Run Diagnostics
```bash
lla plugin run brew doctor
```

### Interactive Menu
```bash
lla plugin run brew menu
```

### Show Help
```bash
lla plugin run brew help
```

## Configuration

The plugin stores its configuration in `~/.config/lla/brew/config.toml`:

```toml
greedy_upgrades = true    # Include auto-update casks in upgrade
show_caveats = true       # Show package caveats after info

[colors]
success = "bright_green"
info = "bright_cyan"
warning = "bright_yellow"
error = "bright_red"
formula = "bright_blue"
cask = "bright_magenta"
version = "bright_yellow"
outdated = "bright_red"
```

## Output Example

### List Installed
```
📦 Installed Packages
──────────────────────────────────────────────────────────────────────

🍺 Formulae (42)
   • git 2.43.0
   • node 21.5.0 [outdated]
   • python@3.12 3.12.1

🖥️  Casks (15)
   • firefox (Firefox) 121.0
   • visual-studio-code (Visual Studio Code) 1.85.1

──────────────────────────────────────────────────────────────────────
```

### Search Results
```
🔍 Search Results for 'git'
──────────────────────────────────────────────────────────────────────

🍺 Formulae (25)
   • git 2.43.0 - Distributed revision control system
   • git-lfs 3.4.0 - Git extension for versioning large files
   • gitui 0.24.3 - Blazing fast terminal-ui for git

🖥️  Casks (8)
   • github (GitHub Desktop) 3.3.6 - Desktop client for GitHub
   • gitkraken (GitKraken) 9.11.0 - Git client focusing on productivity

──────────────────────────────────────────────────────────────────────
```

