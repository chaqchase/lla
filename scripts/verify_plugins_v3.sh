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

# Package verification runs WASM components with exactly their declared
# permissions without persisting release-test grants in the archive.
GRANTS_PATH="$PACKAGE_DIR/plugin-grants.toml"
REMOVE_GRANTS=false
if [[ ! -f "$GRANTS_PATH" ]]; then
  python3 - "$PACKAGE_DIR" "$GRANTS_PATH" <<'PY'
import json
import pathlib
import sys
import tomllib

package_dir = pathlib.Path(sys.argv[1])
grants_path = pathlib.Path(sys.argv[2])
lines = ["schema_version = 1", ""]
for manifest_path in sorted(package_dir.glob("*/plugin.toml")):
    manifest = tomllib.loads(manifest_path.read_text())
    if manifest["plugin"].get("runtime", "native") != "wasm-component":
        continue
    plugin = manifest["plugin"]
    permissions = manifest.get("permissions", {})
    plugin_id = json.dumps(plugin["id"])
    lines.extend([
        f"[plugins.{plugin_id}]",
        f"version = {json.dumps(plugin['version'])}",
        "",
        f"[plugins.{plugin_id}.permissions]",
        "filesystem = " + json.dumps(permissions.get("filesystem", [])),
        "network = " + json.dumps(permissions.get("network", [])),
        "process = " + str(permissions.get("process", False)).lower(),
        "clipboard = " + str(permissions.get("clipboard", False)).lower(),
        "open_url = " + str(permissions.get("open_url", False)).lower(),
        "",
    ])
grants_path.write_text("\n".join(lines))
PY
  REMOVE_GRANTS=true
fi

cleanup_grants() {
  if [[ "$REMOVE_GRANTS" == true ]]; then
    rm -f "$GRANTS_PATH"
  fi
}
trap cleanup_grants EXIT

cargo build -p lla
TEST_HOME=$(mktemp -d)
cleanup_all() {
  cleanup_grants
  rm -rf "$TEST_HOME"
}
trap cleanup_all EXIT

HOME="$TEST_HOME" target/debug/lla --plugins-dir "$PACKAGE_DIR" plugin doctor

if [[ -d "$PACKAGE_DIR/file_hash" ]]; then
  HOME="$TEST_HOME" target/debug/lla --plugins-dir "$PACKAGE_DIR" \
    plugin run file_hash help --output json >/dev/null
  HASH_FIXTURE="$TEST_HOME/hash-fixture"
  mkdir -p "$HASH_FIXTURE"
  printf 'abc' > "$HASH_FIXTURE/abc.txt"
  HOME="$TEST_HOME" target/debug/lla --plugins-dir "$PACKAGE_DIR" \
    --enable-plugin file_hash --json "$HASH_FIXTURE" | python3 -c '
import json
import sys

entries = json.load(sys.stdin)
fields = entries[0]["plugin"]
assert fields["sha1"] == "a9993e364706816aba3e25717850c26c9cd0d89d"
assert fields["sha256"] == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
'
fi
