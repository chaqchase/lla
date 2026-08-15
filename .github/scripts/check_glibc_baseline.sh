#!/usr/bin/env bash

set -euo pipefail

usage() {
  echo "Usage: $0 <maximum-glibc-version> <elf-file> [elf-file ...]" 1>&2
}

if [[ $# -lt 2 ]]; then
  usage
  exit 2
fi

baseline="$1"
shift

if ! [[ "$baseline" =~ ^[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid glibc baseline: $baseline" 1>&2
  exit 2
fi

if ! command -v readelf >/dev/null 2>&1; then
  echo "readelf is required to verify glibc compatibility" 1>&2
  exit 1
fi

for artifact in "$@"; do
  if [[ ! -f "$artifact" ]]; then
    echo "ELF artifact not found: $artifact" 1>&2
    exit 1
  fi

  highest="$({
    readelf -W --version-info "$artifact" 2>/dev/null || true
  } | grep -oE 'GLIBC_[0-9]+(\.[0-9]+)+' | sed 's/^GLIBC_//' | sort -Vu | tail -n 1)"

  if [[ -z "$highest" ]]; then
    echo "No versioned glibc symbols found in $artifact"
    continue
  fi

  oldest="$(printf '%s\n%s\n' "$baseline" "$highest" | sort -V | head -n 1)"
  if [[ "$oldest" != "$highest" ]]; then
    echo "$artifact requires GLIBC_$highest, newer than supported GLIBC_$baseline" 1>&2
    exit 1
  fi

  echo "$artifact requires at most GLIBC_$highest (baseline: GLIBC_$baseline)"
done
