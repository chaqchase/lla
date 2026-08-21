# Releasing the lla workspace

The CLI, plugin SDK crates, bundled plugin manifests, binaries, and documentation
are released from this repository under one product version and Git tag.

## Release units

The Cargo workspace contains:

1. `lla_plugin_interface`
2. `lla_plugin_sdk_macros`
3. `lla_plugin_sdk`
4. `lla_plugin_utils`
5. `lla`
6. The bundled plugin crates

The first five crates are published to crates.io. Bundled native plugin packages
are built for each release target and attached to the GitHub release. The WIT
world is included in the SDK crate.

## Prepare a version

Use the release preparation workflow or run its script with a semantic version:

```bash
RELEASE_VERSION=0.6.0 .github/scripts/prepare_release.sh
```

The script updates the shared workspace version, all internal dependency
versions, bundled `Cargo.toml` files, bundled `plugin.toml` files, and the
changelog. Release validation fails if any of these versions diverge.

## Required verification

Before tagging a release, run:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo test -p lla --features wasm-plugins
cargo clippy -p lla --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p lla --no-default-features --all-targets -- -D warnings
```

CI also runs `cargo check -p lla` natively inside NetBSD and verifies that its
default dependency graph does not contain Wasmtime.

The release workflow separately builds `lla-netbsd-amd64` inside NetBSD 10.1,
checks that Wasmtime remains absent, exercises the binary's help path, and adds
it to the verified release assets and checksum manifest. Linux, macOS, and
Windows binaries build with `--features wasm-plugins`. Windows AMD64 and ARM64
builds run natively, use the static MSVC runtime, and verify their corresponding
plugin `.zip` archives.

Build the SDK fixtures, including a real Component Model target:

```bash
cargo check --manifest-path sdk/tests/fixtures/minimal_native/Cargo.toml
cargo check --manifest-path sdk/tests/fixtures/custom_batch/Cargo.toml
rustup target add wasm32-wasip2
cargo build --manifest-path sdk/tests/fixtures/wasm_component/Cargo.toml \
  --target wasm32-wasip2
```

Build and verify the bundled plugins:

```bash
./scripts/build_plugins.sh --target "$(rustc -vV | sed -n 's/^host: //p')"
./scripts/verify_plugins_v3.sh dist/plugins-<os>-<arch>
```

## Publication order

The release workflow publishes crates in dependency order:

```text
lla_plugin_interface
        ↓
lla_plugin_sdk_macros
        ↓
lla_plugin_sdk
        ↓
lla_plugin_utils
        ↓
lla
```

After each dependency is published, the workflow waits until crates.io exposes
that exact version before continuing. A resumed workflow skips versions already
published and rejects an inconsistent partial publication state.

The GitHub release is published only after validation, binary/plugin artifact
collection (including NetBSD and both Windows architectures), and crates.io
publication succeed.
