<div align="center">
  <img src="https://github.com/user-attachments/assets/f7d26ac0-6d4c-4d66-9a4c-046158b20d24" alt="lla logo" width="128" />
  <h1>lla</h1>
  <p>Modern, customizable, feature-rich and extensible <code>ls</code> replacement.</p>
  <p>
    <a href="https://lla.chaqchase.com">Website</a> ·
    <a href="docs/README.md">Documentation</a> ·
    <a href="docs/getting-started.md">Get started</a> ·
    <a href="docs/command-reference.md">CLI reference</a>
  </p>
</div>

`lla` combines familiar directory listings with multiple views, Git-aware
output, filtering and content search, interactive navigation, machine-readable
formats, themes, shortcuts, and an extensible plugin platform.

<div align="center">
  <img src="https://github.com/user-attachments/assets/3517c63c-f4ec-4a51-ab6d-46a0ed7918f8" alt="lla default view" />
</div>

## Install

```bash
curl -sSL https://raw.githubusercontent.com/chaqchase/lla/main/install.sh | bash
```

Or use a package manager:

| Platform | Command |
| --- | --- |
| Cargo | `cargo install lla` |
| macOS with Homebrew | `brew install lla` |
| Arch Linux with paru | `paru -S lla` |
| NetBSD with pkgin | `pkgin install lla` |
| X-CMD | `x install lla` |

Initialize the configuration after installation:

```bash
lla init
```

See [Installation and first run](docs/getting-started.md) for manual downloads,
musl limitations, upgrades, and initialization options.

Official releases include `lla-netbsd-amd64`; the install script selects it on
NetBSD amd64. This binary supports native plugins but omits the unsupported
embedded WASM runtime.

## Try it

```bash
lla                     # default view
lla -l                  # long view with metadata
lla -t -d 3             # tree view, three levels deep
lla -G                   # Git-aware view
lla --timeline          # group entries by time
lla --search "TODO"     # search file contents
lla --json --pretty     # machine-readable output
```

## Highlights

### Views for different workflows

- **Everyday listings:** start with the compact default view, switch to long
  view for permissions and metadata, or use table and grid layouts when column
  comparison or screen density matters.
- **Directory exploration:** tree and recursive views reveal nested structure,
  with configurable depth and safety limits for large directory trees.
- **Repository and time awareness:** Git view surfaces repository status,
  timeline groups entries by age, and sizemap compares storage usage. Archives
  can be browsed as virtual directories without extracting them first.
- **Interactive discovery:** fuzzy view supports searching, multi-selection,
  opening files, copying paths, editing, and renaming from the terminal.

See [Views and display](docs/views.md) for screenshots, examples, and guidance
on choosing a view.

### Precise filtering and content search

- Sort by name, size, or date; reverse the result; place directories first; and
  choose natural or case-sensitive ordering.
- Select entries by name, extension, glob, regular expression, file type, size,
  creation time, modification time, dotfile visibility, or Git ignore rules.
- Combine compatible name filters with AND, OR, and NOT expressions, save common
  combinations as presets, and apply sequential refinements.
- Search file contents with ripgrep-backed literal or regular-expression
  matching, syntax-highlighted context, and optional plugin action pipelines.

The exact listing and content-search compatibility rules are documented in
[Filtering and search](docs/filtering-and-search.md).

### Rich metadata and configurable presentation

- Long and table views can show permissions, ownership, timestamps, inode and
  hard-link counts, allocated size, extended attributes, security context, mount
  information, symlink targets, and plugin-provided fields.
- Choose symbolic, octal, binary, compact, or verbose permissions; absolute or
  relative dates; icons; color; hyperlinks; and an explicit column order.
- Install, preview, and select themes, then keep global defaults in
  `~/.config/lla/config.toml` and repository-specific overrides in `.lla.toml`.

See [Configuration](docs/configuration.md) for persistent settings, profiles,
themes, exclusions, recursion limits, and shell completion.

### Navigation and repeatable workflows

- The directory jumper combines bookmarks and deduplicated visit history with a
  generated `j` shell function for Bash, Zsh, and Fish.
- Command shortcuts turn frequently used plugin actions into concise top-level
  `lla` commands and can be exported or imported as TOML.
- File and directory diffing supports local-to-local comparison and comparison
  against Git references, including unified text diffs and size or line deltas.

See [Navigation](docs/navigation.md) for jump and fuzzy controls.

### Automation-friendly output

- Emit listing data as a JSON array, newline-delimited JSON, or fixed-schema CSV
  while retaining the selected path, sort, filter, depth, and archive behavior.
- JSON and NDJSON include typed plugin fields. Content search and plugin actions
  provide their own documented machine-output contracts.
- Generate completion scripts for Bash, Zsh, Fish, PowerShell, and Elvish.

See [Machine output](docs/machine-output.md) for schemas and search-output
differences, or the [command reference](docs/command-reference.md) for flags.

### Extensible Plugin Platform

- Install official prebuilt plugins, build from a local directory, or install
  from a Git repository; inspect packages and permissions before running them.
- Native plugins provide trusted in-process extensions. On builds with the
  optional `wasm-plugins` feature, WebAssembly Component Model plugins run with
  declared, persisted grants and scoped host capabilities. Official Linux and
  macOS release binaries enable this feature; the NetBSD binary does not.
- Plugins can add typed listing fields, formatting, and actions with human,
  JSON, NDJSON, or CSV output. The bundled catalog covers metadata, Git context,
  code analysis, storage insights, file operations, and other focused workflows.

Start with [Installing and managing plugins](docs/plugins/README.md), browse the
[bundled catalog](docs/plugins/catalog.md), or read the
[plugin development guide](docs/plugins/developing.md).

## Documentation

Start at the [documentation index](docs/README.md), or go directly to a guide:

| Topic | Guide |
| --- | --- |
| Install, upgrade, and initialize | [Getting started](docs/getting-started.md) |
| Listing formats and screenshots | [Views and display](docs/views.md) |
| Sort, filter, and search | [Filtering and search](docs/filtering-and-search.md) |
| Jump and fuzzy workflows | [Navigation](docs/navigation.md) |
| Global config, project profiles, themes, and shortcuts | [Configuration](docs/configuration.md) |
| JSON, NDJSON, CSV, and schemas | [Machine output](docs/machine-output.md) |
| Flags and subcommands | [Command reference](docs/command-reference.md) |
| Install and use plugins | [Plugins](docs/plugins/README.md) |
| Build plugins | [Plugin development](docs/plugins/developing.md) |
| Architecture and internals | [Architecture handbook](docs/handbook.md) |

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).
