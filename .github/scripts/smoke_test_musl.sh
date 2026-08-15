#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <lla-musl-binary>" 1>&2
  exit 2
fi

binary="$(realpath "$1")"
if [[ ! -x "$binary" ]]; then
  echo "Musl binary is not executable: $binary" 1>&2
  exit 1
fi

fixture_dir="$(mktemp -d)"
trap 'rm -rf "$fixture_dir"' EXIT
export HOME="$fixture_dir/home"
mkdir -p "$HOME" "$fixture_dir/files/subdir"
printf 'needle in a haystack\n' > "$fixture_dir/files/example.txt"
printf 'another file\n' > "$fixture_dir/files/subdir/other.txt"
tar -C "$fixture_dir/files" -czf "$fixture_dir/example.tar.gz" .

"$binary" init --default >/dev/null
config_file="$HOME/.config/lla/config.toml"
sed 's/enabled_plugins = \[\]/enabled_plugins = ["missing_plugin"]/' \
  "$config_file" >"$config_file.tmp"
mv "$config_file.tmp" "$config_file"

"$binary" "$fixture_dir/files" >/dev/null 2>"$fixture_dir/default.stderr"
[[ ! -s "$fixture_dir/default.stderr" ]]
"$binary" "$fixture_dir/files" --long >/dev/null 2>"$fixture_dir/long.stderr"
[[ ! -s "$fixture_dir/long.stderr" ]]
"$binary" "$fixture_dir/files" --table >/dev/null 2>"$fixture_dir/table.stderr"
[[ ! -s "$fixture_dir/table.stderr" ]]
"$binary" "$fixture_dir/files" --json >"$fixture_dir/listing.json" 2>"$fixture_dir/json.stderr"
[[ ! -s "$fixture_dir/json.stderr" ]]
grep -q 'example.txt' "$fixture_dir/listing.json"
"$binary" "$fixture_dir/files" --csv >"$fixture_dir/listing.csv" 2>"$fixture_dir/csv.stderr"
[[ ! -s "$fixture_dir/csv.stderr" ]]
grep -q 'example.txt' "$fixture_dir/listing.csv"
"$binary" "$fixture_dir/files" --filter '*.txt' >/dev/null
"$binary" "$fixture_dir/files" --search needle >"$fixture_dir/search.txt"
grep -q 'needle' "$fixture_dir/search.txt"
"$binary" "$fixture_dir/example.tar.gz" --long >"$fixture_dir/archive.txt"
grep -q 'example.txt' "$fixture_dir/archive.txt"
"$binary" config show-effective >/dev/null
"$binary" theme preview default >/dev/null

plugin_output="$fixture_dir/plugin-error.txt"
assert_plugin_unavailable() {
  if "$binary" "$@" >"$plugin_output" 2>&1; then
    echo "Static musl plugin command unexpectedly succeeded: $*" 1>&2
    exit 1
  fi
  grep -qF \
    'Dynamic plugins are unavailable in the static musl build; use a GNU build for plugin support.' \
    "$plugin_output"
}

assert_plugin_unavailable install --prebuilt
assert_plugin_unavailable list-plugins
assert_plugin_unavailable "$fixture_dir/files" --enable-plugin missing_plugin

echo "Static musl smoke tests passed"
