<div align="center">
  <img src="https://github.com/user-attachments/assets/f7d26ac0-6d4c-4d66-9a4c-046158b20d24" alt="lla logo" width="128" />
  <h1>lla CLI</h1>
  <p>Blazing-fast and highly customizable <code>ls</code> replacement with an extensible plugin platform.</p>
  <p>
    <a href="https://github.com/chaqchase/lla">Repository</a> ·
    <a href="https://github.com/chaqchase/lla/tree/main/docs">Documentation</a> ·
    <a href="https://lla.chaqchase.com">Website</a>
  </p>
</div>

This directory contains the executable `lla` crate in the workspace. It owns
the command-line interface, configuration loading, filesystem listing pipeline,
formatters, search and navigation commands, theme management, upgrades, and the
native and WebAssembly plugin host.

<div align="center">
  <img src="https://github.com/user-attachments/assets/ba5fa273-c2c4-4143-b199-ab5bff1bb608" alt="lla default view" />
</div>

## Install

Install the published crate with Cargo:

```bash
cargo install lla
```

For the installation script, prebuilt binaries, package-manager commands, musl
notes, and upgrades, see the repository's
[installation guide](https://github.com/chaqchase/lla/blob/main/docs/getting-started.md).

## Try it

```bash
lla                     # compact default listing
lla -l                  # detailed metadata
lla -t -d 3             # tree view with a depth limit
lla -G                   # Git-aware view
lla --search "TODO"     # content search
lla --json --pretty     # machine-readable listing
```

## Capabilities

- Default, long, tree, table, grid, Git, timeline, sizemap, recursive, and fuzzy
  views, plus archive and single-file listings
- Name, glob, regular-expression, type, size, time, visibility, and Git-aware
  filtering with configurable sorting
- Ripgrep-backed content search with human and machine output modes
- Directory jumping, bookmarks, visit history, themes, project profiles,
  shortcuts, completion generation, diffing, and in-place upgrades
- JSON, NDJSON, and CSV listing output
- Native dynamic plugins and WebAssembly Component Model plugins with typed
  fields, actions, manifests, integrity checks, and permissions

## Documentation

The repository documentation is canonical:

- [Documentation index](https://github.com/chaqchase/lla/blob/main/docs/README.md)
- [Views and display](https://github.com/chaqchase/lla/blob/main/docs/views.md)
- [Filtering and search](https://github.com/chaqchase/lla/blob/main/docs/filtering-and-search.md)
- [Navigation](https://github.com/chaqchase/lla/blob/main/docs/navigation.md)
- [Configuration](https://github.com/chaqchase/lla/blob/main/docs/configuration.md)
- [Machine output](https://github.com/chaqchase/lla/blob/main/docs/machine-output.md)
- [Command reference](https://github.com/chaqchase/lla/blob/main/docs/command-reference.md)
- [Plugin user guide](https://github.com/chaqchase/lla/blob/main/docs/plugins/README.md)
- [Architecture and Plugin Platform handbook](https://github.com/chaqchase/lla/blob/main/docs/handbook.md)

## Workspace development

From the repository root:

```bash
cargo build -p lla
cargo test -p lla
cargo run -p lla -- --help
```

The default feature set enables dynamic plugins. Builds without the
`dynamic-plugins` feature retain the core CLI but do not load or manage dynamic
plugins:

```bash
cargo build -p lla --no-default-features
```

## License

Licensed under the workspace's
[MIT license](https://github.com/chaqchase/lla/blob/main/LICENSE).
