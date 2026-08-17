# lla documentation

This directory is the documentation home for the `lla` workspace. The root
README remains the product overview; detailed guides and maintainer procedures
live here so that the repository, crates.io packages, and release workflow can
link to one canonical source.

## Complete handbook

- [lla architecture and Plugin Platform handbook](handbook.md) — complete host
  architecture, plugin lifecycle, native and WebAssembly creation, manifest and
  SDK references, usage, packaging, security, testing, troubleshooting, and
  releases.

## Plugin users

- [Installing and managing plugins](plugins/README.md)
- [Bundled plugin catalog](plugins/catalog.md)
- [Migrating installed plugins to API v3](plugins/migration-v3.md)

## Plugin authors

- [Developing plugins with the Rust SDK](plugins/developing.md)
- [Plugin manifest reference](plugins/manifest.md)
- [Plugin Platform v3 architecture](plugins/architecture.md)

## Maintainers

- [Coordinated workspace releases](maintainers/releasing.md)

Crate-specific API documentation remains beside each crate:

- [`lla_plugin_sdk`](../sdk/README.md)
- [`lla_plugin_interface`](../interface/README.md)
- [`lla_plugin_utils`](../utils/README.md)
