# `lla_plugin_sdk`

The maintained Rust SDK for lla Plugin Platform v3.

Plugin authors implement the high-level `Plugin` trait and export either a
native dynamic library with `export_plugin!` or a WebAssembly component with
`export_component!`. The SDK provides entry decoration, overridable native
batching, typed actions, structured output, panic containment, and compile-time
manifest embedding.

Start with the repository's
[plugin development guide](https://github.com/chaqchase/lla/blob/main/docs/plugins/developing.md).
The complete schema is documented in the
[manifest reference](https://github.com/chaqchase/lla/blob/main/docs/plugins/manifest.md).

```rust
use lla_plugin_sdk::{interface::proto, Plugin};

#[derive(Default)]
struct Example;

impl Plugin for Example {
    fn decorate_entry(
        &mut self,
        mut entry: proto::DecoratedEntry,
    ) -> proto::DecoratedEntry {
        entry.custom_fields.insert("example".into(), "yes".into());
        entry
    }
}

lla_plugin_sdk::export_plugin!(Example);
```

Every plugin crate must contain a valid schema-3 `plugin.toml` at its crate
root. The export macro validates and embeds that file in the compiled artifact.
