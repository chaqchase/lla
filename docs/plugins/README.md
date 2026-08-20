# Plugins

For the complete system design and end-to-end authoring tutorial, read the
[lla architecture and Plugin Platform handbook](../handbook.md). This page is a
concise user guide.

Plugin Platform v3 packages are self-contained directories containing a
schema-3 manifest, a native dynamic library or WebAssembly component, and a
SHA-256 inventory.

```text
my-plugin/
├── plugin.toml
├── libmy_plugin.so        # .dylib on macOS, .dll on Windows
├── checksums.toml
└── README.md              # optional
```

For a WebAssembly package, the native library is replaced by the `.wasm`
component named by `plugin.entrypoint`. Loading it requires an lla host built
with `--features wasm-plugins`; official Linux and macOS release binaries
include that feature, while the NetBSD release binary intentionally does not.

## Install plugins

Install the official bundle from the latest release:

```bash
lla install --prebuilt
```

Build and install plugins from a Git repository or a local source directory:

```bash
lla install --git https://github.com/chaqchase/lla
lla install --dir /path/to/plugin
```

Source installs build the entrypoint and create the checksum inventory. A
prebuilt package must already contain a complete `checksums.toml`; installation
fails if any covered file has changed.

## Inspect an installation

```bash
lla plugin doctor
lla plugin info file_hash
lla plugin permissions file_hash
```

`plugin doctor` compares the packaged and compiled manifests, checks registered
handlers and output schemas, validates checksums, and exercises the plugin
contract without using the normal plugin data directory.

## Run an action

```bash
lla plugin run <plugin> <action> -- <arguments>
lla plugin run <plugin> <action> --output json -- <arguments>
```

Supported output formats are `human`, `json`, `ndjson`, and `csv`. The older
`lla plugin <plugin> <action>` form is deprecated during the 0.6.x series.

Interactive actions require a terminal and cannot be requested with a machine
output format.

## Permissions

Native plugins are trusted code. Their permissions are declarations presented
to the user but are not an operating-system sandbox.

WebAssembly plugins are default-denied. Approved grants are persisted in
`plugin-grants.toml`, and an update that requests broader access prompts again.
WebAssembly packages can receive scoped filesystem preopens, exact-domain WASI
HTTP access, and gated clipboard or URL-opening host calls. Raw sockets and
subprocess execution are unavailable.

See the [catalog](catalog.md) for bundled plugins, the [development guide](developing.md)
to create one, or the [migration guide](migration-v3.md) when upgrading from an
older lla release.
