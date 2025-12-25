<h1>
<p align="center">
  <img src="https://github.com/user-attachments/assets/f7d26ac0-6d4c-4d66-9a4c-046158b20d24" alt="Logo" width="128">
  <br>lla
</h1>

<p align="center">
    Modern, customizable, feature-rich and extensible `ls` replacement.
    <br />
    <a href="https://lla.chaqchase.com">Documentation</a>
    ·
    <a href="#features">Features</a>
    ·
    <a href="#installation">Installation</a>
    ·
    <a href="#display-formats">Display Formats</a>
    ·
    <a href="#command-reference">Command Reference</a>
  </p>
</p>

## Overview

lla is a modern `ls` replacement that transforms how developers interact with their filesystem. Built with Rust's performance capabilities and designed with user experience in mind, lla combines the familiarity of `ls` with powerful features like specialized views, Git integration, and a robust plugin system with an extensible list of plugins to add more functionality.

## Features

- Multiple Views: Default clean view, long format, tree structure, table layout, grid display
- Diff Command: Human-friendly directory or file comparisons (local ↔ local or git) with size/line deltas and unified diff output
- Git Integration: Built-in status visualization and repository insights
- Advanced Organization: Timeline view, storage analysis, recursive exploration
- Smart Navigation: Interactive directory jumper with bookmarks and history
- Smart Search: complex filtering patterns (OR, AND, NOT, XOR), regex support, content search
- Customization: Plugin system, theme manager, custom shortcuts, configurable display
- High Performance: Built with Rust, modern UI, clean listings
- Smart Sorting: Multiple criteria, directory-first option, natural sorting
- Flexible Config: Easy initialization, plugin management, configuration tools
- Rich Plugin Ecosystem: File ops and metadata enhancements, code analysis, git tools, and more
- Smart Filters: Range-based size/time filters, reusable presets, cache-aware refinements
- Git-aware Filtering: Optional `.gitignore` support via `--respect-gitignore` or config defaults

## Installation

### Using Installation Script

The easiest way to install lla is using our installation script:

```bash
curl -sSL https://raw.githubusercontent.com/chaqchase/lla/main/install.sh | bash
```

This script will automatically:

- Detect your OS and architecture
- Download the appropriate binary
- Verify the checksum
- Install lla to `/usr/local/bin`

### Using Package Manager

| Package Manager / Platform | Command             |
| -------------------------- | ------------------- |
| Cargo                      | `cargo install lla` |
| macOS (Homebrew)           | `brew install lla`  |
| Arch Linux (paru)          | `paru -S lla`       |
| NetBSD (pkgin)             | `pkgin install lla` |
| X-CMD                      | `x install lla`     |

### Manual Installation

```bash
# Manual - Example is for amd64 GNU, replaces the file names if downloading for a different arch.
wget -c https://github.com/triyanox/lla/releases/download/<version>/<lla-<os>-<arch>> -O lla # Example /v0.3.9/lla-linux-amd64
sudo chmod +x lla
sudo chown root:root lla
sudo mv lla /usr/local/bin/lla
```

### Upgrading

Upgrade to the latest (or a specific) release without leaving the terminal:

```bash
# Upgrade to the latest release (auto-detects OS/arch, verifies checksums)
lla upgrade

# Target a specific tag or custom installation path
lla upgrade --version v0.5.1 --path /usr/local/bin/lla
```

The upgrade command reuses the official install script logic, renders animated progress indicators, verifies the `SHA256SUMS` manifest, and atomically swaps the `lla` binary in place (defaults to the path of the running executable—override with `--path` if needed).

### Post Installation

After installation, initialize your setup:

```bash
# Guided setup with theme/format wizard (default)
lla init

# Write the default config without prompts
lla init --default

# View your config
lla config
```

The revamped wizard (now the default) walks you through:

- Look & feel choices (icons, theme, default view, permission format)
- Listing defaults (sort order, directory inclusion, depth limits)
- Sorting & filtering toggles (dirs-first, natural sort, dotfile visibility, case sensitivity)
- Long-view behavior (relative dates, hide group column, column selection)
- Plugin directory selection, plugin enablement, and recursion safety limits

## Display Formats

### Core Views

#### Default View

Clean, distraction-free listing for quick directory scans:

```bash
lla
```

<img src="https://github.com/user-attachments/assets/3517c63c-f4ec-4a51-ab6d-46a0ed7918f8" className="rounded-2xl" alt="default" />

#### Long Format

Rich metadata display for detailed file analysis:

```bash
lla -l
```

<img src="https://github.com/user-attachments/assets/2a8d95e4-efd2-4bff-a905-9d9a892dc794" className="rounded-2xl" alt="long" />

Options and tweaks:

- Hide the group column (useful on single-user systems):

  ```bash
  lla -l --hide-group
  ```

- Show relative dates (e.g., "2h ago"):

  ```bash
  lla -l --relative-dates
  ```

To make these defaults, add to your config (`~/.config/lla/config.toml`):

```toml
[formatters.long]
hide_group = true
relative_dates = true
columns = ["permissions", "size", "modified", "user", "group", "name", "field:git_status", "field:tags"]
```

The `columns` array lets you control the precise column order. Use built-in keys (`permissions`, `size`, `modified`, `user`, `group`, `name`, `path`, `plugins`) or reference any plugin-provided field with the `field:<name>` prefix (e.g., `field:git_status`, `field:complexity_score`).

#### Tree Structure

Hierarchical exploration of directory relationships:

```bash
lla -t -d 3  # Navigate up to 3 levels deep
```

<img src="https://github.com/user-attachments/assets/cb32bfbb-eeb1-4701-889d-f3d42c7d4896" className="rounded-2xl" alt="tree" />

#### Archive Introspection

List archive contents as a virtual directory (no extraction). Supported: `.zip`, `.tar`, `.tar.gz`, `.tgz`.

```
lla my_archive.zip -t    # tree view
lla project.tar.gz -l    # long view
lla my_archive.tgz --json
lla my_archive.zip -l -f ".rs"  # filter by extension on internal paths
```

#### Single-file Listing

You can pass a single file path to list it directly with any view or machine output:

```
lla README.md          # default view
lla Cargo.toml -l      # long view
lla src/main.rs --json # machine output
```

### Enhanced Organization

#### Table Layout

Structured view optimized for data comparison:

```bash
lla -T
```

<img src="https://github.com/user-attachments/assets/9f1d6d97-4074-4480-b242-a6a2eace4b38" className="rounded-2xl" alt="table" />

The `[formatters.table].columns` setting mirrors the long-view syntax, so you can mix built-in fields with plugin-provided metadata:

```toml
[formatters.table]
columns = ["permissions", "size", "modified", "name", "field:git_branch", "field:git_commit"]
```

#### Diff View (Files & Directories)

Compare directories or individual files (including against git references) with compact, colorized summaries. Directory comparisons still show the status table with size deltas, while file comparisons add a unified diff plus size/line-change stats:

```bash
lla diff src ../backup/src
lla diff apps/api --git              # compare working tree vs HEAD
lla diff src --git --git-ref HEAD~1  # compare against another commit
lla diff Cargo.lock ../backup/Cargo.lock
lla diff Cargo.lock --git --git-ref HEAD~1
```

For directories, the diff view groups entries by status, colors the change indicator, and shows left/right sizes plus the net delta so you can spot growth at a glance. For files, lla prints a size summary, line-count delta, and a git-style unified diff (with automatic binary detection so unreadable data isn’t dumped to your terminal).

#### Grid Display

Space-efficient layout for dense directories:

```bash
lla -g                  # Basic grid view
lla -g --grid-ignore    # Grid view ignoring terminal width (Warning: may extend beyond screen)
```

<img src="https://github.com/user-attachments/assets/b81d01ea-b830-4833-8791-7b62ff9137df" className="rounded-2xl" alt="grid" />

Note: Grid output no longer appends a trailing blank newline.

### Specialized Views

#### Git Integration

Smart repository insights:

```bash
lla -G
```

<img src="https://github.com/user-attachments/assets/b0654b20-c37d-45c2-9fd0-f3399fce385e" className="rounded-2xl" alt="git" />

#### Timeline Organization

Chronological file tracking:

```bash
lla --timeline
```

<img src="https://github.com/user-attachments/assets/06a156a7-628a-4948-b75c-a0da584c9224" className="rounded-2xl" alt="timeline" />

#### Storage Analysis

Visual disk usage insights:

```bash
lla -S # use --include-dirs to calculate directories sizes
```

<img src="https://github.com/user-attachments/assets/dad703ec-ef23-460b-9b9c-b5c5d6595300" className="rounded-2xl" alt="sizemap" />

### Advanced Navigation

#### Jump-to-Directory (Interactive Directory Jumper)

Quickly navigate to bookmarked or recently visited directories with an interactive keyboard-driven prompt:

```bash
# One-time setup for seamless directory jumping
lla jump --setup

# Interactive directory selection
lla jump

# Add directory to bookmarks
lla jump --add ~/projects/my-app

# Remove bookmark
lla jump --remove ~/projects/my-app

# List all bookmarks and recent history
lla jump --list

# Clear history
lla jump --clear-history
```

**Features:**

- **Interactive Selection**: Arrow keys and Enter to select from bookmarked and recent directories
- **Smart History**: Automatically records directory visits (respects `exclude_paths`)
- **Bookmarks**: Add favorite directories for quick access
- **Deduplication**: No duplicate entries in history
- **Shell Integration**: Works seamlessly with shell commands like `cd`

**How it works:**

1. Bookmarks (marked with ★) appear first in the selection list
2. Recent directories follow, showing directory name and full path
3. Use arrow keys to navigate, Enter to select
4. The selected path is printed to stdout for shell integration
5. History is automatically maintained as you navigate directories

**Shell Integration Setup:**

Since `lla` runs as a child process, it cannot directly change your shell's working directory. To enable seamless directory jumping, run the automatic setup:

```bash
lla jump --setup
```

This will automatically detect your shell (bash, zsh, or fish) and add the necessary function to your shell configuration file. After setup, restart your terminal or run `source ~/.zshrc` (or equivalent for your shell).

Then use `j` to jump to directories interactively!

**Examples:**

```bash
# One-time setup (auto-detects your shell)
lla jump --setup

# shell override
lla jump --setup --shell fish

# Navigate to a frequently used directory (after setup)
j

# Add your project directories to bookmarks
lla jump --add ~/dev/my-project
lla jump --add ~/work/client-app
lla jump --add ~/personal/blog

# View all your saved locations
lla jump --list
```

#### Fuzzy Search (Experimental)

Interactive file discovery with multi-select and batch actions:

```bash
lla --fuzzy
```

Keyboard shortcuts:

- Ctrl+J / Ctrl+K: move down / up
- Ctrl+N / Ctrl+P: move down / up
- Ctrl+D / Ctrl+U: half-page down / up
- Ctrl+G: jump to end
- Ctrl+Shift+G: jump to start
- Ctrl+C / Esc: exit
- Space: toggle select
- Enter: confirm (returns the highlighted file or all selected files)
- Ctrl+E: open selected file(s) in your editor
- Ctrl+Y: copy selected path(s) to clipboard
- Ctrl+O: open selected file(s) with the system opener (open/xdg-open/start)
- F2: rename the highlighted file (Enter: confirm, Esc/Ctrl+C: cancel)

Search input shortcuts:

- Ctrl+W: delete word backward
- Ctrl+H: delete character (backspace)
- Ctrl+A: jump to start of input (`Home` also works)
- End: jump to end of input

Editor selection (for `Ctrl+E`):

- Config override: `listers.fuzzy.editor` (takes priority)
- Environment: `$EDITOR`, then `$VISUAL`
- Fallback: `nano` (macOS/Linux) or `notepad` (Windows)

Example config:

```toml
[listers.fuzzy]
editor = "nvim" # or: "code --wait"
```

Tip: for GUI editors, configure a “wait” flag so lla returns after you close the file, e.g. `code --wait` or `subl -w`.

<img src="https://github.com/user-attachments/assets/ec946fd2-34d7-40b7-b951-ffd9c4009ad6" className="rounded-2xl" alt="fuzzy" />

#### Deep Directory Exploration (Recursive)

Comprehensive subdirectory listing:

```bash
lla -R
lla -R -d 3  # Set exploration depth
```

<img src="https://github.com/user-attachments/assets/f8fa0901-8866-4b92-a76e-3b7fd307f04e" className="rounded-2xl" alt="recursive" />

The `-R` option can be integrated with other options to create a more specific view. For example, `lla -R -l`
will show a detailed listing of all files and directories in the current directory.

### Content Search

Powerful ripgrep-backed content search with syntax highlighting and theme integration:

```bash
lla --search "TODO"
```

**Features:**

- **Syntax Highlighting**: Code snippets are automatically highlighted based on file extension
- **Match Indicators**: Bright yellow carets (^^^) point to exact match locations
- **Context Control**: Configurable context lines around matches (`--search-context`)
- **Theme Integration**: Colors adapt to your current lla theme
- **Safe by Default**: Uses literal string matching; add `regex:` prefix for regex patterns
- **Filter Integration**: Honors all existing filters, exclude paths, and dotfile settings

**Examples:**

```bash
# Basic content search
lla --search "main()"

# Search with more context
lla --search "TODO" --search-context 5

# Regex search
lla --search "regex:^func.*\("

# Search in specific file types
lla --search "Error" --filter ".rs"

# Case-sensitive search
lla --search "Error" --case-sensitive

# Machine output formats
lla --search "FIXME" --json
lla --search "TODO" --csv
```

**Output Format:**

Each match shows:

- File path with themed colors
- Syntax-highlighted code snippet with line numbers
- Bright yellow carets (^^^) marking exact match positions
- Configurable context lines before and after matches

**Integration:**

- Works with all existing filters (`--filter`, `--files-only`, etc.)
- Respects `exclude_paths` configuration
- Honors dotfile settings (`--no-dotfiles`, `--almost-all`)
- Supports machine output formats (`--json`, `--ndjson`, `--csv`)

### Machine Output

Stable, streaming machine-readable formats are available. These modes keep existing listing filters/sorts/depth behavior; only the output changes.

- **--json**: Output a single JSON array (streamed). Use **--pretty** to pretty print.
- **--ndjson**: Output newline-delimited JSON, one object per line.
- **--csv**: Output CSV with a header row.

Flags are mutually exclusive. **--pretty** only affects **--json**.

JSON/NDJSON schema (stable fields):

```
{
  "path": "src/main.rs",
  "name": "main.rs",
  "extension": "rs" | null,
  "file_type": "file" | "dir" | "symlink" | "other",
  "size_bytes": 1234,
  "modified": "2024-05-01T12:34:56Z",
  "created": "..." | null,
  "accessed": "..." | null,
  "mode_octal": "0644",
  "owner_user": "mohamed" | null,
  "owner_group": "staff" | null,
  "inode": 1234567 | null,
  "hard_links": 1 | null,
  "symlink_target": "..." | null,
  "is_hidden": false,
  "git_status": "M." | null,
  "plugin": { /* plugin-provided fields, if any */ }
}
```

CSV columns (v1):

```
path,name,extension,file_type,size_bytes,modified,created,accessed,mode_octal,owner_user,owner_group,inode,hard_links,symlink_target,is_hidden,git_status
```

Examples:

```bash
lla --json --pretty
lla --ndjson
lla --csv
```

## Command Reference

### Display Options

#### Basic Views

| Command   | Short | Description                                                                                                       | Example  |
| --------- | ----- | ----------------------------------------------------------------------------------------------------------------- | -------- |
| (default) |       | List current directory                                                                                            | `lla`    |
| `--long`  | `-l`  | Detailed file information with metadata                                                                           | `lla -l` |
| `--tree`  | `-t`  | Hierarchical directory visualization                                                                              | `lla -t` |
| `--table` | `-T`  | Structured data display                                                                                           | `lla -T` |
| `--grid`  | `-g`  | Organized grid layout you can use `-g --grid-ignore` to ignore terminal width (Warning: may extend beyond screen) | `lla -g` |

#### Advanced Views

| Command       | Short | Description                             | Example                               |
| ------------- | ----- | --------------------------------------- | ------------------------------------- |
| `--sizemap`   | `-S`  | Visual representation of file sizes     | `lla -S` <br> `lla -S --include-dirs` |
| `--timeline`  |       | Group files by time periods             | `lla --timeline`                      |
| `--git`       | `-G`  | Show git status and information         | `lla -G`                              |
| `--fuzzy`     | `-F`  | Interactive fuzzy finder (Experimental) | `lla --fuzzy`                         |
| `--recursive` | `-R`  | Recursive listing format                | `lla -R` <br> `lla -R -d 3`           |

#### Comparison & Diff

| Command | Description                                                    | Example                                                               |
| ------- | -------------------------------------------------------------- | --------------------------------------------------------------------- |
| `diff`  | Compare two directories or a directory vs git with size deltas | `lla diff src ../backup/src`<br>`lla diff src --git --git-ref HEAD~1` |

#### Navigation Commands

| Command                    | Description                       | Example                               |
| -------------------------- | --------------------------------- | ------------------------------------- |
| `lla jump --setup`         | Auto-setup shell integration      | `lla jump --setup`                    |
| `lla jump`                 | Interactive directory jumper      | `j` (after setup)                     |
| `lla jump --add`           | Add directory to bookmarks        | `lla jump --add ~/projects/my-app`    |
| `lla jump --remove`        | Remove bookmark                   | `lla jump --remove ~/projects/my-app` |
| `lla jump --list`          | List bookmarks and recent history | `lla jump --list`                     |
| `lla jump --clear-history` | Clear directory history           | `lla jump --clear-history`            |

#### Display Modifiers

| Command               | Description                                                                           | Example                         |
| --------------------- | ------------------------------------------------------------------------------------- | ------------------------------- |
| `--icons`             | Show icons for files and directories                                                  | `lla --icons`                   |
| `--no-icons`          | Hide icons for files and directories                                                  | `lla --no-icons`                |
| `--no-color`          | Disable all colors in the output                                                      | `lla --no-color`                |
| `--permission-format` | Set the format for displaying permissions (symbolic, octal, binary, verbose, compact) | `lla --permission-format octal` |

### Sort & Filter Options

#### Sorting

| Command                 | Short | Description                                  | Example                                             |
| ----------------------- | ----- | -------------------------------------------- | --------------------------------------------------- |
| `--sort`                | `-s`  | Sort files by criteria                       | `lla -s name` <br> `lla -s size` <br> `lla -s date` |
| `--sort-reverse`        | `-r`  | Reverse the sort order                       | `lla -s size -r`                                    |
| `--sort-dirs-first`     |       | List directories before files                | `lla --sort-dirs-first`                             |
| `--sort-case-sensitive` |       | Enable case-sensitive sorting                | `lla --sort-case-sensitive`                         |
| `--sort-natural`        |       | Natural number sorting (2.txt before 10.txt) | `lla --sort-natural`                                |

#### Basic Filtering

| Command            | Short | Description                     | Example                             |
| ------------------ | ----- | ------------------------------- | ----------------------------------- |
| `--filter`         | `-f`  | Filter files by pattern         | `lla -f "test"` <br> `lla -f ".rs"` |
| `--preset`         |       | Apply named filter preset       | `lla --preset rust_sources`         |
| `--size`           |       | Range filter on file size       | `lla --size 10M..1G`                |
| `--modified`       |       | Range filter on modified time   | `lla --modified <7d`                |
| `--created`        |       | Range filter on creation time   | `lla --created 2023-01-01..`        |
| `--case-sensitive` | `-c`  | Enable case-sensitive filtering | `lla -f "test" -c`                  |
| `--refine`         |       | Reuse cached listing w/ filter  | `lla --refine "regex:foo"`          |
| `--depth`          | `-d`  | Set the depth for tree listing  | `lla -t -d 3` <br> `lla -d 2`       |

##### Range Filters & Presets

- Size filters understand human units and comparisons: `--size >10M`, `--size 512K..2G`, `--size ..100K`.
- Time filters support ISO dates and relative durations: `--modified <7d`, `--created 2023-01-01..2023-12-31`.
- Define reusable presets in `[filter.presets.<name>]` within your config, then reuse with `--preset <name>`.
- `--refine` replays the last cached listing and applies new filters instantly—great for chaining explorations without touching the disk again.

#### Content Search

| Command            | Description                                | Example                                                   |
| ------------------ | ------------------------------------------ | --------------------------------------------------------- |
| `--search`         | Search file contents for pattern (ripgrep) | `lla --search "TODO"`                                     |
| `--search-context` | Number of context lines (default: 2)       | `lla --search "TODO" --search-context 3`                  |
| `--search-pipe`    | Pipe matches into plugins                  | `lla --search "TODO" --search-pipe file_tagger:list-tags` |

Content search uses literal string matching by default (safe for special characters). Use `regex:` prefix for regex patterns:

```bash
# Literal search (default) - safe for any characters
lla --search "main()"
lla --search "TODO: fix bug"

# Regex search - use regex: prefix
lla --search "regex:^func.*\("

# Search in specific file types
lla --search "TODO" --filter ".rs"

# Case-sensitive search
lla src/ --search "Error" --case-sensitive

# Search with machine output
lla --search "FIXME" --json
```

- Automations: chain `--search` with `--search-pipe plugin:action[:arg...]` to send every matching file directly into plugins (e.g., `--search-pipe file_tagger:list-tags` or `--search-pipe file_organizer:organize:type`). The matched file paths are appended to the plugin arguments automatically.

#### Advanced Filtering Patterns

| Filter Type        | Example                       | Description                                    |
| ------------------ | ----------------------------- | ---------------------------------------------- |
| OR Operation       | `lla -f "test,spec"`          | Match files containing either "test" or "spec" |
| AND Operation      | `lla -f "+test,api"`          | Match files containing both "test" and "api"   |
| Regular Expression | `lla -f "regex:^test.*\.rs$"` | Rust files starting with "test"                |
| Glob Pattern       | `lla -f "glob:*.{rs,toml}"`   | Match .rs or .toml files                       |
| Composite AND      | `lla -f "test AND .rs"`       | Logical AND operation                          |
| Composite OR       | `lla -f "test OR spec"`       | Logical OR operation                           |
| Composite NOT      | `lla -f "NOT test"`           | Logical NOT operation                          |
| Composite XOR      | `lla -f "test XOR spec"`      | Logical XOR operation                          |

### View Filters

#### Show Only Filters

| Command           | Description                         | Example               |
| ----------------- | ----------------------------------- | --------------------- |
| `--dirs-only`     | Show only directories               | `lla --dirs-only`     |
| `--files-only`    | Show only regular files             | `lla --files-only`    |
| `--symlinks-only` | Show only symbolic links            | `lla --symlinks-only` |
| `--dotfiles-only` | Show only dot files and directories | `lla --dotfiles-only` |

#### Hide Filters

| Command               | Description                                     | Example                   |
| --------------------- | ----------------------------------------------- | ------------------------- |
| `--no-dirs`           | Hide directories                                | `lla --no-dirs`           |
| `--no-files`          | Hide regular files                              | `lla --no-files`          |
| `--no-symlinks`       | Hide symbolic links                             | `lla --no-symlinks`       |
| `--no-dotfiles`       | Hide dot files and directories                  | `lla --no-dotfiles`       |
| `--respect-gitignore` | Skip files excluded by .gitignore / git exclude | `lla --respect-gitignore` |

> Tip: Set `filter.respect_gitignore = true` in your config to make this the default. Use `--no-gitignore` to override in a single run.

```toml
[filter]
respect_gitignore = true
```

#### Combined Filters

| Description                                  | Example                           |
| -------------------------------------------- | --------------------------------- |
| Show only dot directories                    | `lla --dirs-only --dotfiles-only` |
| Show only regular files, excluding dot files | `lla --files-only --no-dotfiles`  |

### Plugin Management

#### Installation

| Command              | Description                                    | Example                                            |
| -------------------- | ---------------------------------------------- | -------------------------------------------------- |
| `install`            | Install from latest prebuilt release (default) | `lla install`                                      |
| `install --prebuilt` | Install from latest prebuilt release           | `lla install --prebuilt`                           |
| `install --git`      | Install from Git repository                    | `lla install --git https://github.com/user/plugin` |
| `install --dir`      | Install from local directory                   | `lla install --dir path/to/plugin`                 |

#### Plugin Controls

| Command            | Description                | Example                                                                       |
| ------------------ | -------------------------- | ----------------------------------------------------------------------------- |
| `use`              | Interactive plugin manager | `lla use`                                                                     |
| `--enable-plugin`  | Enable specific plugins    | `lla --enable-plugin name`                                                    |
| `--disable-plugin` | Disable specific plugins   | `lla --disable-plugin name`                                                   |
| `update`           | Update plugins             | `lla update` <br> `lla update file_tagger`                                    |
| `plugin`           | Run plugin actions         | `lla plugin --name file_tagger --action add-tag --args README.md "important"` |

#### Shortcut Management

| Command           | Description        | Example                                                           |
| ----------------- | ------------------ | ----------------------------------------------------------------- |
| `shortcut add`    | Add a new shortcut | `lla shortcut add find file_finder search -d "Quick file search"` |
| `shortcut remove` | Remove a shortcut  | `lla shortcut remove find`                                        |
| `shortcut list`   | List all shortcuts | `lla shortcut list`                                               |

### Configuration & Setup

| Command                 | Description                         | Example                                                                         |
| ----------------------- | ----------------------------------- | ------------------------------------------------------------------------------- |
| `init`                  | Initialize the configuration file   | `lla init`                                                                      |
| `init --default`        | Write defaults without the wizard   | `lla init --default`                                                            |
| `config`                | View or modify configuration        | `lla config`                                                                    |
| `config show-effective` | Show merged global + profile config | `lla config show-effective`                                                     |
| `config diff --default` | List overrides vs built-in defaults | `lla config diff --default`                                                     |
| `theme`                 | Interactive theme manager           | `lla theme`                                                                     |
| `theme pull`            | Pull the built-in themes            | `lla theme pull`                                                                |
| `theme install`         | Install theme from file/directory   | `lla theme install /path/to/theme.toml`<br>`lla theme install /path/to/themes/` |
| `theme preview`         | Render sample output for a theme    | `lla theme preview one_dark`                                                    |
| `completion`            | Generate shell completion scripts   | `lla completion bash`                                                           |
| `clean`                 | Clean up invalid plugins            | `lla clean`                                                                     |

### General Options

| Command     | Short | Description               |
| ----------- | ----- | ------------------------- |
| `--help`    | `-h`  | Print help information    |
| `--version` | `-V`  | Print version information |

> **Note**
> For detailed usage and examples of each command, visit the [lla documentation](https://lla.chaqchase.com).

### Excluding Paths (macOS iCloud and others)

You can exclude heavy or virtualized directories from listings via the config key `exclude_paths`.

Example (`~/.config/lla/config.toml`):

```toml
# Paths to exclude from listings (tilde is supported)
exclude_paths = [
  "~/Library/Mobile Documents", # iCloud Drive
  "~/Library/CloudStorage"      # Other cloud providers
]
```

Notes:

- Tilde `~` is expanded to your home directory.
- Exclusions are honored in recursive listings and top-level listings.

### Project Profiles (`.lla.toml`)

Keep repo-specific defaults local by dropping a `.lla.toml` file anywhere inside the project. lla walks up from your current working directory, finds the nearest profile, and overlays it on top of the global config without touching `~/.config/lla/config.toml`.

Example `.lla.toml`:

```toml
show_icons = true
default_format = "git"

[sort]
dirs_first = true
```

Use `lla config show-effective` to inspect the merged configuration (global + profile) and `lla config diff --default` to see exactly which keys diverge from the built-in defaults and whether the change came from the global config or the profile file.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
