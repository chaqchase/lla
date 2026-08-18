# Command reference

This page is a compact map of the public CLI. `lla --help` and
`lla <subcommand> --help` are authoritative for the installed version.

```text
lla [OPTIONS] [directory] [SUBCOMMAND]
```

The directory defaults to `.` and may also be a supported archive or one file.

## Views

| Option | Short | Purpose |
| --- | --- | --- |
| `--long` | `-l` | Detailed metadata. |
| `--tree` | `-t` | Hierarchical tree. |
| `--table` | `-T` | Column-oriented table. |
| `--grid` | `-g` | Terminal-width grid. |
| `--grid-ignore` | | Ignore terminal width in grid view. |
| `--sizemap` | `-S` | Visual size map. |
| `--timeline` | | Group entries by time period. |
| `--git` | `-G` | Git status and repository information. |
| `--fuzzy` | `-F` | Interactive fuzzy finder. |
| `--recursive` | `-R` | Recursive listing. |
| `--depth <n>` | `-d` | Tree or recursive depth. |

See [Views and display](views.md) for screenshots and examples.

## Sorting and selection

| Option | Short | Purpose |
| --- | --- | --- |
| `--sort name\|size\|date` | `-s` | Select the sort key. |
| `--sort-reverse` | `-r` | Reverse ordering. |
| `--sort-dirs-first` | | Put directories first. |
| `--sort-case-sensitive` | | Use case-sensitive sorting. |
| `--sort-natural` | | Sort embedded numbers naturally. |
| `--filter <pattern>` | `-f` | Filter names or extensions. |
| `--preset <name>` | | Apply a configured preset; repeatable. |
| `--size <range>` | | Filter by file size. |
| `--modified <range>` | | Filter by modification time. |
| `--created <range>` | | Filter by creation time. |
| `--case-sensitive` | `-c` | Use case-sensitive filtering. |
| `--refine <pattern>` | | Apply sequential name/path refinements; repeatable. |
| `--respect-gitignore` | | Apply Git ignore rules. |
| `--no-gitignore` | | Override configured Git-ignore behavior. |

## Entry types and symlinks

| Option | Purpose |
| --- | --- |
| `--dirs-only`, `--files-only`, `--symlinks-only` | Show only one entry type. |
| `--no-dirs`, `--no-files`, `--no-symlinks` | Hide one entry type. |
| `--show-symlinks` | Include links whose targets match file/directory-only filters. |
| `--dereference` (`-X`) | Use symlink-target metadata. |
| `--no-symlink-target` | Hide the rendered target suffix. |
| `--all` (`-a`) | Show dotfiles. |
| `--almost-all` (`-A`) | Show dotfiles except `.` and `..`. |
| `--no-dotfiles` | Hide dotfiles. |
| `--dotfiles-only` | Show only dotfiles. |

See [Filtering and search](filtering-and-search.md) for pattern and range syntax.

## Metadata and presentation

| Option | Short | Purpose |
| --- | --- | --- |
| `--icons`, `--no-icons` | | Override icon display. |
| `--hyperlink [always\|auto\|never]` | | Control OSC 8 links. |
| `--no-color` | | Disable colors. |
| `--include-dirs` | | Calculate recursive directory sizes. |
| `--permission-format <format>` | | Choose symbolic, octal, binary, verbose, or compact. |
| `--hide-group` | | Hide the long-view group column. |
| `--relative-dates` | | Use relative long-view timestamps. |
| `--date-format <format>` | | Set the Chrono date format. |
| `--inode` | `-i` | Show inode numbers. |
| `--links` | `-H` | Show hard-link counts. |
| `--allocated-size` | | Show allocated bytes. |
| `--extended` | `-@` | Show extended attributes. |
| `--context` | `-Z` | Show ACL or SELinux context. |
| `--mounts` | `-M` | Show mount information. |

## Search and machine output

| Option | Purpose |
| --- | --- |
| `--search <pattern>` | Search file contents with ripgrep. |
| `--search-context <n>` | Set surrounding context lines. |
| `--search-pipe <plugin:action[:arg...]>` | Send matching paths to a plugin; repeatable. |
| `--json` | Emit one JSON array. |
| `--ndjson` | Emit one object per line. |
| `--csv` | Emit a CSV header and rows. |
| `--pretty` | Pretty-print `--json`. |

See [Machine output](machine-output.md) for schemas.

## Plugin overrides

| Option | Purpose |
| --- | --- |
| `--enable-plugin <name>` | Enable a plugin for the invocation; repeatable. |
| `--disable-plugin <name>` | Disable a plugin for the invocation; repeatable. |
| `--plugins-dir <path>` | Override the writable plugin directory. |

## Subcommands

| Command | Purpose | Detailed guide |
| --- | --- | --- |
| `clean` | Remove invalid plugins. | [Plugins](plugins/README.md) |
| `completion` | Generate shell completion scripts. | [Configuration](configuration.md#shell-completion) |
| `config` | View or modify configuration. | [Configuration](configuration.md) |
| `diff` | Compare paths or a path against Git. | [Views](views.md#compare-files-and-directories) |
| `init` | Create configuration interactively or from defaults. | [Getting started](getting-started.md#initialize-lla) |
| `install` | Install prebuilt, Git, or local plugins. | [Plugins](plugins/README.md#install-plugins) |
| `jump` | Manage bookmarks/history and select a directory. | [Navigation](navigation.md#directory-jumping) |
| `list-plugins` | List discovered plugins. | [Plugins](plugins/README.md#inspect-an-installation) |
| `plugin` | Run actions or inspect, validate, and migrate packages. | [Plugins](plugins/README.md) |
| `shortcut` | Manage plugin-action shortcuts. | [Configuration](configuration.md#shortcuts) |
| `theme` | Manage, install, and preview themes. | [Configuration](configuration.md#themes) |
| `update` | Update all plugins or a named plugin. | [Plugins](plugins/README.md) |
| `upgrade` | Upgrade the lla executable. | [Getting started](getting-started.md#upgrade) |
| `use` | Open the interactive plugin manager. | [Plugins](plugins/README.md) |

## General options

| Option | Short | Purpose |
| --- | --- | --- |
| `--help` | `-h` | Print help. |
| `--version` | `-V` | Print the version. |
