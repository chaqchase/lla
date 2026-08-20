# Plugin Platform v3 architecture

For the full host execution architecture, plugin lifecycle, creation tutorial,
security model, and release flow, see the
[architecture and Plugin Platform handbook](../handbook.md).

Plugin Platform v3 separates the authoring API, wire contract, runtime, and
packaging responsibilities.

```text
plugin source
    │
    ├── lla_plugin_sdk ── high-level Rust trait and exports
    │       └── lla_plugin_sdk_macros ── native/component exports
    │
    ├── plugin.toml ── identity, handlers, schemas, permissions
    │
    └── native library or WASM component
            │
            └── lla_plugin_interface ── protobuf + ABI v3 wire contract
                    │
                    └── lla host ── validation, grants, limits, rendering
```

## Crate responsibilities

Repository directories are intentionally short; published crate names remain
stable:

| Directory | Published crate | Responsibility |
| --- | --- | --- |
| `interface/` | `lla_plugin_interface` | Protobuf messages, manifests, constants, and the native ABI |
| `macros/` | `lla_plugin_sdk_macros` | Native/component exports and manifest embedding |
| `sdk/` | `lla_plugin_sdk` | Public high-level Rust authoring API and WIT world |
| `utils/` | `lla_plugin_utils` | Optional UI, configuration, formatting, and compatibility helpers |

Plugin authors use the macros through the SDK re-exports rather than depending
on the macro implementation crate directly. The interface crate is
intentionally low-level.

All four crates are workspace members and share the lla product version.

## Native ABI

Native plugins export only `_plugin_create_v3`, which returns `PluginApiV3`.
Requests and responses are protobuf byte buffers. The plugin frees the response
memory it allocated and destroys its own API context. Rust-owned layout does not
cross the dynamic-library boundary.

The loader scans v1/v2 symbols without calling their constructors. Older
libraries are reported as disabled and never executed by lla 0.6.0.

## Component Model runtime

When the host is compiled with the non-default `wasm-plugins` feature,
WebAssembly packages run through embedded Wasmtime with WASI Preview 2 and the
Component Model. Official Linux and macOS release binaries enable this feature;
the NetBSD release binary uses the default native-only runtime. The host exposes
scoped filesystem preopens, exact-domain WASI HTTP, and permission-gated
clipboard and URL calls. It does not expose raw sockets or subprocess execution.

The host limits each instance to 128 MiB of memory, responses to 16 MiB, batches
to 512 entries, decoration calls to 5 seconds, and actions to 60 seconds. Traps,
timeouts, permission failures, and limit violations become structured plugin
errors rather than process crashes.

## Contract verification

The package manifest is the source of truth. At installation and during
`plugin doctor`, the host verifies:

1. Package checksums.
2. Schema and API compatibility.
3. Packaged versus embedded manifest equality.
4. Registered action handlers.
5. Declared fields and action output schemas.
6. Runtime and permission constraints.
