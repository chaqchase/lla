#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")/.."

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <packaged-plugin-directory>" 1>&2
  exit 2
fi

PACKAGE_DIR="$1"
if [[ ! -d "$PACKAGE_DIR" ]]; then
  echo "Plugin package directory not found: $PACKAGE_DIR" 1>&2
  exit 1
fi

expected_count=$(find plugins -mindepth 2 -maxdepth 2 -name Cargo.toml -type f | wc -l | tr -d ' ')
packaged_count=$(find "$PACKAGE_DIR" -mindepth 2 -maxdepth 2 -name plugin.toml -type f | wc -l | tr -d ' ')
if [[ "$packaged_count" != "$expected_count" ]]; then
  echo "Expected $expected_count plugin packages, found $packaged_count in $PACKAGE_DIR" 1>&2
  exit 1
fi

for manifest in plugins/*/plugin.toml; do
  plugin_name=$(basename "$(dirname "$manifest")")
  packaged_manifest="$PACKAGE_DIR/$plugin_name/plugin.toml"
  if [[ ! -f "$packaged_manifest" ]]; then
    echo "Missing packaged manifest for $plugin_name" 1>&2
    exit 1
  fi
  if ! cmp -s "$manifest" "$packaged_manifest"; then
    echo "Packaged manifest for $plugin_name differs from the source manifest" 1>&2
    exit 1
  fi
done

cargo build -p lla
target/debug/lla --plugins-dir "$PACKAGE_DIR" plugin doctor
