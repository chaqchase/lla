# GitHub Workflows Overview

This repository uses a staged workflow strategy so CI runs only when it matters and releases append assets incrementally without rebuilding what already exists.

## CI (`ci.yml`)
- Triggered on pushes and pull requests to `main` **only** when Rust sources, manifests, proto files, scripts, or the CI definition itself change.
- Runs formatting, Clippy (non-blocking), tests, and a release-mode build matrix (Linux + macOS, multiple targets).
- Eliminating docs/completions-only triggers keeps CI fast and avoids redundant runners.

## Release Chain
The release process is split into four independent workflows that communicate via `repository_dispatch` events. Each stage checks the GitHub Release for previously uploaded assets and skips work when everything is already present.

1. **`release.yml` – Release Core Bins**
   - Starts when a release PR (with the `release` label) merges into `main`, or via manual `workflow_dispatch`.
   - Ensures the version changed, builds the core binaries matrix, publishes crates to crates.io, and creates/updates the GitHub Release with notes + SHA256 sums.
   - Uploads only missing binaries/checksums and dispatches `release-plugins` when done.

2. **`release-plugins.yml` – Release Plugins**
   - Triggered by the dispatch from the core workflow (or manually).
   - Rebuilds plugin archives per target only when the release lacks the corresponding `.tar.gz`/`.zip`.
   - Uploads the missing archives and dispatches `release-packages`.

3. **`release-packages.yml` – Release Packages**
   - Downloads the Linux binaries directly from the release, runs nFPM to produce `.deb`, `.rpm`, `.apk`, `.pkg.tar.zst`, and uploads any missing package artifacts.
   - Dispatches `release-themes` once packaging succeeds.

4. **`release-themes.yml` – Release Themes**
   - Zips the `themes/` directory and uploads `themes.zip` only if it does not already exist on the release.

### Helper Script
`.github/scripts/release_helpers.sh` provides reusable functions:
- `ensure_release_exists` – create/edit releases safely.
- `asset_exists` – check if an asset is already attached.
- `dispatch_next_stage` – emit the next `repository_dispatch` event with tag + version payload.
Every workflow sources this script to guarantee idempotent uploads and clean chaining.

## Manual Dispatch Tips
- To resume a stage, open the desired workflow and use **Run workflow** supplying `release_tag` (e.g., `v1.2.3`). `release_version` defaults to the tag without `v` when omitted.
- Each workflow will validate the release exists and skip gracefully if all assets are already present.

## Runner Requirements
- GitHub-hosted runners already include `gh` and `jq`. For self-hosted runners, ensure both tools plus the Rust toolchain, protoc, and nFPM (for the packaging stage) are pre-installed.

