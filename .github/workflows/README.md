# GitHub Workflows Overview

This repository uses regular CI for pull requests and a short two-action release process:

1. Open a generated release-prep PR that updates every version surface.
2. Merge it; the release workflow creates the matching `vX.Y.Z` tag and publishes.

## CI (`ci.yml`)

- Triggered on pushes and pull requests to `main` when Rust sources, manifests, proto files, scripts, workflow files, package metadata, or toolchain files change.
- Runs formatting, Clippy, tests, and release-mode build checks across Linux and macOS.
- Builds GNU/Linux artifacts against an explicit glibc 2.28 baseline and rejects newer symbol requirements.
- Clippy stays in regular CI; release publishing is gated by formatting, tests, validation, asset verification, and publish dry-runs.

## Prepare Release (`prepare-release.yml`)

- Started manually from **Run workflow**.
- Inputs:
  - `version`: target version with or without a `v` prefix, for example `v0.5.5`.
  - `changelog`: optional fallback markdown if `CHANGELOG.md` has no `## [Unreleased]` section.
- Opens a PR with a conventional commit such as `chore: prepare release v0.5.5`.
- Updates:
  - root workspace version in `Cargo.toml`
  - explicit internal dependency versions in `lla/Cargo.toml` and `lla_plugin_utils/Cargo.toml`
  - every `plugins/*/Cargo.toml` package version
  - `Cargo.lock`
  - `CHANGELOG.md`, by promoting `## [Unreleased]` into `## [X.Y.Z] - YYYY-MM-DD` and leaving a fresh empty `## [Unreleased]` section

## Release (`release.yml`)

- Triggered when a release-prep PR with the `release` label is merged into `main`.
- Also triggered by pushing a tag that matches `v*.*.*`.
- Can be rerun manually with `workflow_dispatch` and an existing `release_tag`.
- Validates before doing release work:
  - tag is semver with a `v` prefix
  - tag commit is reachable from `main`
  - tag version matches the workspace, internal dependency, and plugin versions
  - `CHANGELOG.md` has a section for the version
  - crates.io state can be resumed safely
- Runs release gates:
  - `cargo fmt --all -- --check`
  - `cargo test --workspace`
- Builds and verifies all release assets before publishing:
  - CLI binaries: `lla-linux-*`, `lla-macos-*`
  - plugin archives: `plugins-*.tar.gz` and `plugins-*.zip`
  - Linux packages: `.deb`, `.rpm`, `.apk`, `.pkg.tar.zst`
  - `themes.zip`
  - final `SHA256SUMS`
- GNU/Linux CLI and plugin artifacts are built with pinned Zig tooling and must not require glibc newer than 2.28.
- Publishes crates.io packages in dependency order:
  - `lla_plugin_interface`
  - `lla_plugin_utils`
  - `lla`
- Runs `cargo publish --dry-run` immediately before each crate publish; dependent crates are dry-run only after their internal dependencies are visible on crates.io.
- Creates or updates the GitHub release as a draft, uploads verified assets, verifies uploaded assets, then publishes the release.

## Manual Release Checklist

1. Add release notes under `## [Unreleased]` in `CHANGELOG.md`.
2. Run **Prepare Release** with the target version.
3. Review and merge the generated `chore: prepare release vX.Y.Z` PR.
4. Watch the **Release** workflow; it creates the matching tag automatically.
5. If the workflow fails after partial publishing, rerun **Release** manually with the same tag. Existing crates and release assets are skipped when safe.

## Required Secrets

- `GITHUB_TOKEN`: provided by GitHub Actions.
- `CRATES_IO_TOKEN`: required for publishing `lla_plugin_interface`, `lla_plugin_utils`, and `lla`.

## Helper Scripts

- `.github/scripts/prepare_release.sh` updates release versions and changelog content for the generated PR.
- `.github/scripts/release_helpers.sh` contains shared validation, expected asset, checksum, crates.io, and GitHub release helpers.
- `scripts/build_plugins.sh` builds plugin dynamic libraries and produces both `.tar.gz` and `.zip` archives for each release target.
- `.github/scripts/check_glibc_baseline.sh` enforces the documented GNU/Linux compatibility baseline.
