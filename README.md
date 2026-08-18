<h1>
<p align="center">
  <img src="https://github.com/user-attachments/assets/f7d26ac0-6d4c-4d66-9a4c-046158b20d24" alt="Logo" width="128">
  <br>lla
</h1>

<p align="center">
    Modern, customizable, feature-rich and extensible <code>ls</code> replacement.
    <br />
    <a href="https://lla.chaqchase.com">Website</a>
    ·
    <a href="docs/README.md">Documentation</a>
    ·
    <a href="docs/getting-started.md">Get started</a>
    ·
    <a href="docs/command-reference.md">CLI reference</a>
  </p>
</p>

`lla` combines familiar directory listings with multiple views, Git-aware
output, filtering and content search, interactive navigation, machine-readable
formats, themes, shortcuts, and an extensible plugin platform.

<p align="center">
  <img src="https://github.com/user-attachments/assets/3517c63c-f4ec-4a51-ab6d-46a0ed7918f8" className="rounded-2xl" alt="lla default view" />
</p>

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

- Default, long, tree, table, grid, Git, timeline, sizemap, recursive, and
  interactive fuzzy views
- Name, type, size, date, pattern, `.gitignore`, and content filtering
- Directory and file comparisons, including comparisons against Git references
- Interactive directory jumping with bookmarks and history
- Configurable themes, icons, columns, sorting, shortcuts, and project profiles
- JSON, NDJSON, and CSV output for scripts and automation
- Native and WebAssembly plugins with typed fields and actions

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
