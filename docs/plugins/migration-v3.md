# Migrating to Plugin Platform v3

lla 0.6.0 supports API v3 plugins only. API v1 and v2 native libraries are
detected without loading their code and are reported as disabled.

## Installed official plugins

After installing lla 0.6.0, migrate the official prebuilt bundle:

```bash
lla plugin migrate --prebuilt
lla plugin doctor
```

Migration replaces matching official packages and preserves the previous
artifacts in a timestamped legacy backup. Unmatched third-party plugins are not
modified and are listed in the report. If replacement fails, the installer
restores the previous artifacts, metadata, and permission grants.

## Third-party plugin source

There is no binary compatibility bridge. Rebuild the plugin against
`lla_plugin_sdk` 0.6 and replace its manifest with schema 3.

For a native Rust plugin:

1. Add `lla_plugin_sdk = "0.6"` and build a `cdylib`.
2. Implement `lla_plugin_sdk::Plugin`.
3. Replace the old export with `lla_plugin_sdk::export_plugin!(PluginType)`.
4. Add stable identity, API range, runtime, and entrypoint fields to
   `plugin.toml`.
5. Declare every field, action, argument, output schema, and permission.
6. Install from source and run `lla plugin doctor`.

Existing protobuf-oriented source can temporarily implement the SDK's
compatibility request adapter, but new code should implement `decorate_entry`,
`decorate_batch`, `format_field`, `registered_actions`, and `run_action`
directly. The compatibility adapter is not part of the native ABI and may be
removed in a later SDK major version.

## Behavioral changes

- `plugin.toml` is now a compiled and packaged contract, not passive metadata.
- Actions receive typed, host-validated arguments.
- Plugins return structured output for host rendering.
- Interactive actions must be explicitly declared.
- WebAssembly permissions are enforced and persisted as grants.
- Native and WebAssembly calls are subject to response, batch, and timeout
  limits.
