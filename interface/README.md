# `lla_plugin_interface`

`lla_plugin_interface` is the low-level wire-contract crate for Plugin Platform
v3. Most plugin authors should depend on `lla_plugin_sdk`; this crate exists for
generated bindings, alternative-language SDKs, hosts, and tooling.

Plugin authors should start with the
[high-level SDK guide](https://github.com/chaqchase/lla/blob/main/docs/plugins/developing.md).

## API v3 boundary

Native plugins export only `_plugin_create_v3`, returning `PluginApiV3`.
Requests and responses are protobuf bytes, response memory is released by the
plugin that allocated it, the API object has a plugin-owned destructor, and no
Rust-owned type crosses the dynamic-library boundary.

The v3 API embeds the exact `plugin.toml` bytes in the compiled plugin. The host
compares this contract with the packaged manifest before enabling a plugin.
API v1 and v2 exports are not loaded by lla 0.6.0.

## SDK and Component Model

Rust plugins use `lla_plugin_sdk::Plugin` and
`lla_plugin_sdk::export_plugin!`. The trait exposes entry decoration, true batch
decoration, and typed actions; its default batch implementation processes each
entry individually and can be overridden with one native operation.

The maintained WIT world is published at
`sdk/wit/lla-plugin.wit`. Other languages can generate Component
Model bindings from that world and package a `.wasm` component.
Rust Component Model plugins enable the SDK's `component` feature and use
`lla_plugin_sdk::export_component!` instead of the native export macro.

Every plugin crate must place a valid schema-3 `plugin.toml` at its crate root;
the export macro validates and embeds it at compile time.

See the
[architecture guide](https://github.com/chaqchase/lla/blob/main/docs/plugins/architecture.md)
for crate boundaries, runtime constraints, and contract verification.
