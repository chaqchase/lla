#!/usr/bin/env bash

set -euo pipefail

# Change to repo root (script is in scripts/)
cd "$(dirname "$0")/.."

usage() {
  echo "Usage: $0 [--target <triple>]" 1>&2
  echo "Example: $0 --target x86_64-unknown-linux-gnu" 1>&2
}

TARGET_TRIPLE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      shift
      TARGET_TRIPLE="${1:-}"
      [[ -z "$TARGET_TRIPLE" ]] && { echo "--target requires an argument" 1>&2; usage; exit 2; }
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" 1>&2
      usage
      exit 2
      ;;
  esac
done

if [[ -z "$TARGET_TRIPLE" ]]; then
  # Detect host triple
  TARGET_TRIPLE=$(rustc -vV | sed -n 's/^host: \(.*\)$/\1/p')
fi

case "$TARGET_TRIPLE" in
  *apple-darwin)
    DL_EXT="dylib"
    OS_LABEL="macos"
    ;;
  *windows*)
    DL_EXT="dll"
    OS_LABEL="windows"
    ;;
  *)
    DL_EXT="so"
    OS_LABEL="linux"
    ;;
esac

case "$TARGET_TRIPLE" in
  aarch64-*) ARCH_LABEL="arm64" ;;
  x86_64-*)  ARCH_LABEL="amd64" ;;
  *)
    echo "Unsupported architecture in target triple: $TARGET_TRIPLE" 1>&2
    exit 1
    ;;
esac

STAGING_DIR="dist/plugins-${OS_LABEL}-${ARCH_LABEL}"
ARCHIVE_TGZ="dist/plugins-${OS_LABEL}-${ARCH_LABEL}.tar.gz"
ARCHIVE_ZIP="dist/plugins-${OS_LABEL}-${ARCH_LABEL}.zip"

echo "Building all plugins for target: ${TARGET_TRIPLE}"
echo "Output staging directory: ${STAGING_DIR}"

rm -rf "$STAGING_DIR"
mkdir -p "$STAGING_DIR"

# Collect plugin crate names from plugins/*/Cargo.toml
mapfile -t PLUGIN_CRATES < <(for f in plugins/*/Cargo.toml; do \
  [[ -f "$f" ]] || continue; \
  awk -F ' = ' '/^name *=/ {gsub(/"/, "", $2); print $2; exit}' "$f"; \
done)

if [[ ${#PLUGIN_CRATES[@]} -eq 0 ]]; then
  echo "No plugins found under plugins/*" 1>&2
  exit 1
fi

echo "Found plugins: ${PLUGIN_CRATES[*]}"

# Ensure target toolchain installed (no-op if already present)
rustup target add "$TARGET_TRIPLE" >/dev/null 2>&1 || true

# Build all plugin crates in one cargo invocation to leverage workspace caching
BUILD_PKGS=( )
for crate in "${PLUGIN_CRATES[@]}"; do
  BUILD_PKGS+=( -p "$crate" )
done

echo "Running: cargo build --release --target $TARGET_TRIPLE ${BUILD_PKGS[*]}"
cargo build --release --target "$TARGET_TRIPLE" "${BUILD_PKGS[@]}"

# Copy resulting dynamic libraries to staging
for crate in "${PLUGIN_CRATES[@]}"; do
  if [[ "$DL_EXT" == "dll" ]]; then
    SRC="target/${TARGET_TRIPLE}/release/${crate}.dll"
  else
    SRC="target/${TARGET_TRIPLE}/release/lib${crate}.${DL_EXT}"
  fi

  if [[ ! -f "$SRC" ]]; then
    echo "Expected plugin artifact not found: $SRC" 1>&2
    exit 1
  fi

  cp "$SRC" "$STAGING_DIR/"
done

# Create archive per-OS format
if [[ "$OS_LABEL" == "windows" ]]; then
  # Prefer zip on Windows; fallback to 7z if zip is unavailable
  rm -f "$ARCHIVE_ZIP"
  if command -v zip >/dev/null 2>&1; then
    (cd "$STAGING_DIR" && zip -9 -r "../$(basename "$ARCHIVE_ZIP")" . >/dev/null)
  elif command -v 7z >/dev/null 2>&1; then
    (cd "$STAGING_DIR" && 7z a -tzip -mx=9 "../$(basename "$ARCHIVE_ZIP")" . >/dev/null)
  else
    echo "Neither zip nor 7z found on PATH for Windows packaging" 1>&2
    exit 1
  fi
  echo "Created archive: $ARCHIVE_ZIP"
else
  rm -f "$ARCHIVE_TGZ"
  tar -C "$STAGING_DIR" -czf "$ARCHIVE_TGZ" .
  echo "Created archive: $ARCHIVE_TGZ"
fi

echo "Done."


