# Developing plugins

This is the short development guide. The canonical
[architecture and Plugin Platform handbook](../handbook.md) contains a complete
native plugin implementation, WebAssembly conversion, SDK reference, package
lifecycle, testing matrix, performance guidance, and troubleshooting.

Rust is the maintained SDK language for lla 0.6.0. Native plugins and Rust
WebAssembly components use the same high-level `Plugin` trait. Other languages
may generate Component Model bindings from the published WIT world.

## Choose a runtime

Use a native plugin when you need an existing native dependency, unrestricted
platform integration, or the smallest possible host overhead. Native libraries
are trusted code and must be built for every supported operating system and
architecture.

Use a WebAssembly component when portability and enforced permissions matter.
The embedded runtime is available on supported x86_64 and ARM64 Linux/macOS
builds. i686 builds report WebAssembly packages as unsupported.

## Create a native plugin

Create a library crate and configure it as a dynamic library:

```toml
[package]
name = "my-plugin"
version = "1.0.0"
edition = "2021"

[lib]
name = "my_plugin"
crate-type = ["cdylib"]

[dependencies]
lla_plugin_sdk = "0.6"
```

While developing inside this repository, use the workspace dependency instead:

```toml
[dependencies]
lla_plugin_sdk.workspace = true
```

Implement only the capabilities the plugin needs:

```rust
use lla_plugin_sdk::{interface::proto, Plugin};

#[derive(Default)]
struct MyPlugin;

impl Plugin for MyPlugin {
    fn decorate_entry(
        &mut self,
        mut entry: proto::DecoratedEntry,
    ) -> proto::DecoratedEntry {
        entry.custom_fields.insert("owner".into(), "example".into());
        entry
    }
}

lla_plugin_sdk::export_plugin!(MyPlugin);
```

The exported plugin type must implement `Default + Send + 'static`. The export
macro validates and embeds the crate-root `plugin.toml`, generates the API v3
entrypoint, contains panics, and ensures plugin-allocated responses are freed by
the plugin.

## Decoration and batching

`decorate_entry` handles one entry. The default `decorate_batch` implementation
calls it once for each entry. Override `decorate_batch` when the plugin can
share I/O or computation across a batch:

```rust
fn decorate_batch(
    &mut self,
    mut entries: Vec<proto::DecoratedEntry>,
    _format: &str,
) -> Vec<proto::DecoratedEntry> {
    let shared_state = load_shared_state_once();
    for entry in &mut entries {
        decorate_with_state(entry, &shared_state);
    }
    entries
}
```

The host sends at most 512 entries per batch.

## Typed actions

Declare each action in `plugin.toml`; the manifest is the public contract. The
host parses and validates arguments before calling the plugin.

```toml
[[actions]]
id = "inspect"
description = "Inspect a path"
examples = ["lla plugin run my_plugin inspect -- README.md --limit 10"]
interactive = false
arguments = [
  { name = "path", type = "path", position = 0, required = true },
  { name = "limit", type = "integer", option = "--limit", default = 10, min = 1, max = 100 },
]
output = { type = "value" }
```

Implement `registered_actions` with matching IDs and `run_action` to receive a
`HashMap<String, TypedValue>`. Return an `ActionResponse` containing `none`,
`text`, `value`, or typed `table` output. Do not print machine-readable output
directly; the host renders the declared output format.

Installation and `plugin doctor` reject missing handlers and manifest/output
contract mismatches.

## Create a Rust WebAssembly component

Enable the component feature and export with the component macro:

```toml
[lib]
name = "my_plugin"
crate-type = ["cdylib"]

[dependencies]
lla_plugin_sdk = { version = "0.6", features = ["component"] }
```

```rust
use lla_plugin_sdk::Plugin;

#[derive(Default)]
struct MyPlugin;

impl Plugin for MyPlugin {}

lla_plugin_sdk::export_component!(MyPlugin);
```

Set `runtime = "wasm-component"` and use the generated `.wasm` filename as the
manifest entrypoint. Then build it:

```bash
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
```

The maintained interface is
[`sdk/wit/lla-plugin.wit`](../../sdk/wit/lla-plugin.wit).

## Install and test locally

```bash
cargo build --release
lla install --dir /path/to/my-plugin
lla plugin doctor
lla plugin info my_plugin
lla plugin run my_plugin inspect -- README.md
lla plugin run my_plugin inspect --output json -- README.md
```

Workspace maintainers can validate an entire release bundle with:

```bash
./scripts/build_plugins.sh --target "$(rustc -vV | sed -n 's/^host: //p')"
./scripts/verify_plugins_v3.sh dist/plugins-<os>-<arch>
```

Use the SDK fixtures as executable examples:

- [Minimal native plugin](../../sdk/tests/fixtures/minimal_native)
- [Custom native batch](../../sdk/tests/fixtures/custom_batch)
- [Rust WebAssembly component](../../sdk/tests/fixtures/wasm_component)
