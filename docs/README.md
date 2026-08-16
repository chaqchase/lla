# lla documentation

This directory is the documentation home for the `lla` workspace. The root
README remains the product overview; detailed guides and maintainer procedures
live here so that the repository, crates.io packages, and release workflow can
link to one canonical source.

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

- [`lla_plugin_sdk`](../lla_plugin_sdk/README.md)
- [`lla_plugin_interface`](../lla_plugin_interface/README.md)
- [`lla_plugin_utils`](../lla_plugin_utils/README.md)
