#!/usr/bin/env bash

set -euo pipefail

# Change to repo root (script is in scripts/)
cd "$(dirname "$0")/.."

usage() {
  echo "Usage: $0 [--target <triple>] [--glibc-version <version>]" 1>&2
  echo "Example: $0 --target x86_64-unknown-linux-gnu --glibc-version 2.28" 1>&2
}

TARGET_TRIPLE=""
GLIBC_VERSION=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      shift
      TARGET_TRIPLE="${1:-}"
      [[ -z "$TARGET_TRIPLE" ]] && { echo "--target requires an argument" 1>&2; usage; exit 2; }
      shift
      ;;
    --glibc-version)
      shift
      GLIBC_VERSION="${1:-}"
      [[ -z "$GLIBC_VERSION" ]] && { echo "--glibc-version requires an argument" 1>&2; usage; exit 2; }
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

if [[ -n "$GLIBC_VERSION" ]]; then
  if [[ ! "$GLIBC_VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
    echo "Invalid glibc version: $GLIBC_VERSION" 1>&2
    exit 2
  fi
  if [[ "$TARGET_TRIPLE" != *-unknown-linux-gnu ]]; then
    echo "--glibc-version is only supported for unknown-linux-gnu targets" 1>&2
    exit 2
  fi
  if ! command -v cargo-zigbuild >/dev/null 2>&1; then
    echo "cargo-zigbuild is required when --glibc-version is set" 1>&2
    exit 1
  fi
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
  i686-*)    ARCH_LABEL="i686" ;;
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

# Collect plugin crate names from plugins/*/Cargo.toml (Bash 3 compatible)
PLUGIN_CRATES=()
NATIVE_PLUGIN_CRATES=()
WASM_PLUGIN_CRATES=()
for f in plugins/*/Cargo.toml; do
  if [[ -f "$f" ]]; then
    name=$(awk -F ' = ' '/^name *=/ {gsub(/"/, "", $2); print $2; exit}' "$f" || true)
    if [[ -n "$name" ]]; then
      PLUGIN_CRATES+=("$name")
      runtime=$(awk -F ' = ' '/^runtime *=/ {gsub(/"/, "", $2); print $2; exit}' "plugins/$name/plugin.toml" || true)
      case "$runtime" in
        native|"") NATIVE_PLUGIN_CRATES+=("$name") ;;
        wasm-component) WASM_PLUGIN_CRATES+=("$name") ;;
        *) echo "Unsupported plugin runtime '$runtime' in plugins/$name/plugin.toml" 1>&2; exit 1 ;;
      esac
    fi
  fi
done

if [[ ${#PLUGIN_CRATES[@]} -eq 0 ]]; then
  echo "No plugins found under plugins/*" 1>&2
  exit 1
fi

echo "Found plugins: ${PLUGIN_CRATES[*]}"

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# Ensure target toolchain installed (no-op if already present)
rustup target add "$TARGET_TRIPLE" >/dev/null 2>&1 || true

# Build all plugin crates in one cargo invocation to leverage workspace caching
BUILD_PKGS=( )
for crate in "${NATIVE_PLUGIN_CRATES[@]}"; do
  BUILD_PKGS+=( -p "$crate" )
done

if [[ ${#NATIVE_PLUGIN_CRATES[@]} -eq 0 ]]; then
  echo "No native plugins to build"
elif [[ -n "$GLIBC_VERSION" ]]; then
  BUILD_TARGET="${TARGET_TRIPLE}.${GLIBC_VERSION}"
  echo "Running: cargo zigbuild --release --target $BUILD_TARGET ${BUILD_PKGS[*]}"
  cargo zigbuild --release --target "$BUILD_TARGET" "${BUILD_PKGS[@]}"
elif [[ "$OS_LABEL" == "macos" ]]; then
  # rustc's Mach-O stripping can produce malformed LINKEDIT string pools for
  # some cdylibs. Keep plugin symbols intact; the CLI binary remains stripped.
  echo "Running: CARGO_PROFILE_RELEASE_STRIP=none cargo build --release --target $TARGET_TRIPLE ${BUILD_PKGS[*]}"
  CARGO_PROFILE_RELEASE_STRIP=none cargo build --release --target "$TARGET_TRIPLE" "${BUILD_PKGS[@]}"
else
  echo "Running: cargo build --release --target $TARGET_TRIPLE ${BUILD_PKGS[*]}"
  cargo build --release --target "$TARGET_TRIPLE" "${BUILD_PKGS[@]}"
fi

if [[ ${#WASM_PLUGIN_CRATES[@]} -gt 0 ]]; then
  echo "Building WASM components: ${WASM_PLUGIN_CRATES[*]}"
  rustup target add wasm32-wasip2 --toolchain stable >/dev/null
  WASM_RUSTC=$(rustup which rustc --toolchain stable)
  WASM_BUILD_PKGS=( )
  for crate in "${WASM_PLUGIN_CRATES[@]}"; do
    WASM_BUILD_PKGS+=( -p "$crate" )
  done
  RUSTC="$WASM_RUSTC" rustup run stable cargo build --release --target wasm32-wasip2 "${WASM_BUILD_PKGS[@]}"
fi

# Package each plugin as a v3 directory containing its manifest and native entrypoint.
for crate in "${PLUGIN_CRATES[@]}"; do
  # Cargo turns '-' into '_' in library filenames (e.g. my-plugin -> libmy_plugin.so)
  artifact_name="${crate//-/_}"
  runtime=$(awk -F ' = ' '/^runtime *=/ {gsub(/"/, "", $2); print $2; exit}' "plugins/$crate/plugin.toml" || true)
  entrypoint=$(awk -F ' = ' '/^entrypoint *=/ {gsub(/"/, "", $2); print $2; exit}' "plugins/$crate/plugin.toml" || true)
  if [[ "$runtime" == "wasm-component" ]]; then
    SRC="target/wasm32-wasip2/release/$entrypoint"
  elif [[ "$DL_EXT" == "dll" ]]; then
    SRC="target/${TARGET_TRIPLE}/release/${artifact_name}.dll"
  else
    SRC="target/${TARGET_TRIPLE}/release/lib${artifact_name}.${DL_EXT}"
  fi

  if [[ ! -f "$SRC" ]]; then
    echo "Expected plugin artifact not found: $SRC" 1>&2
    exit 1
  fi

  PACKAGE_DIR="$STAGING_DIR/$crate"
  MANIFEST="plugins/$crate/plugin.toml"
  if [[ ! -f "$MANIFEST" ]]; then
    echo "Expected v3 plugin manifest not found: $MANIFEST" 1>&2
    exit 1
  fi
  mkdir -p "$PACKAGE_DIR"
  cp "$SRC" "$PACKAGE_DIR/"
  cp "$MANIFEST" "$PACKAGE_DIR/plugin.toml"
  if [[ -f "plugins/$crate/README.md" ]]; then
    cp "plugins/$crate/README.md" "$PACKAGE_DIR/README.md"
  fi
  artifact_file=$(basename "$SRC")
  artifact_hash=$(hash_file "$PACKAGE_DIR/$artifact_file")
  manifest_hash=$(hash_file "$PACKAGE_DIR/plugin.toml")
  printf '[files]\n"%s" = "%s"\n"plugin.toml" = "%s"\n' \
    "$artifact_file" "$artifact_hash" "$manifest_hash" > "$PACKAGE_DIR/checksums.toml"
done

# Create archive per-OS format
if [[ "$OS_LABEL" != "windows" ]]; then
  rm -f "$ARCHIVE_TGZ"
  tar -C "$STAGING_DIR" -czf "$ARCHIVE_TGZ" .
  echo "Created archive: $ARCHIVE_TGZ"
fi

# Always create a zip archive as a portable fallback for installers and manual downloads.
rm -f "$ARCHIVE_ZIP"
if command -v zip >/dev/null 2>&1; then
  (cd "$STAGING_DIR" && zip -9 -r "../$(basename "$ARCHIVE_ZIP")" . >/dev/null)
elif command -v 7z >/dev/null 2>&1; then
  (cd "$STAGING_DIR" && 7z a -tzip -mx=9 "../$(basename "$ARCHIVE_ZIP")" . >/dev/null)
else
  echo "Neither zip nor 7z found on PATH for plugin packaging" 1>&2
  exit 1
fi
echo "Created archive: $ARCHIVE_ZIP"

echo "Done."
