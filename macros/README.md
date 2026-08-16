# `lla_plugin_sdk_macros`

Export macros used by `lla_plugin_sdk` for Plugin Platform v3 native libraries
and WebAssembly components.

Plugin authors should depend on `lla_plugin_sdk`, which re-exports these macros,
rather than depending on this implementation crate directly. See the
[plugin development guide](https://github.com/chaqchase/lla/blob/main/docs/plugins/developing.md).
