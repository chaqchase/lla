# lla documentation

The root [README](../README.md) is the project overview. This index routes users,
plugin authors, contributors, and maintainers to focused documentation.

## Start here

| Goal | Guide |
| --- | --- |
| Install lla, initialize it, or upgrade | [Installation and first run](getting-started.md) |
| Choose a listing format | [Views and display](views.md) |
| Sort entries or narrow a listing | [Filtering and search](filtering-and-search.md) |
| Search file contents | [Filtering and search](filtering-and-search.md#search-file-contents) |
| Jump between directories or find files interactively | [Navigation](navigation.md) |
| Set persistent defaults or a project profile | [Configuration](configuration.md) |
| Create or manage command shortcuts | [Configuration](configuration.md#shortcuts) |
| Use lla from scripts | [Machine output](machine-output.md) |
| Look up a flag or subcommand | [Command reference](command-reference.md) |

## Plugin users

- [Install and manage plugins](plugins/README.md)
- [Browse the bundled plugin catalog](plugins/catalog.md)
- [Migrate installed plugins to API v3](plugins/migration-v3.md)

## Plugin authors

- [Develop plugins with the Rust SDK](plugins/developing.md)
- [Plugin manifest reference](plugins/manifest.md)
- [Plugin Platform v3 architecture](plugins/architecture.md)
- [Complete architecture and Plugin Platform handbook](handbook.md)

The handbook is the deep reference for the host pipeline, plugin lifecycle,
native and WebAssembly runtimes, manifests, SDK APIs, permissions, packaging,
testing, troubleshooting, and workspace architecture.

Crate-specific API documentation remains beside each crate:

- [`lla_plugin_sdk`](../sdk/README.md)
- [`lla_plugin_interface`](../interface/README.md)
- [`lla_plugin_utils`](../utils/README.md)

## Contributors and maintainers

- [Workspace architecture and development reference](handbook.md)
- [Coordinated workspace releases](maintainers/releasing.md)
