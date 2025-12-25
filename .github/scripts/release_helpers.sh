#!/usr/bin/env bash

set -euo pipefail

if [[ -n "${GITHUB_TOKEN:-}" && -z "${GH_TOKEN:-}" ]]; then
  export GH_TOKEN="$GITHUB_TOKEN"
fi

require_cmd() {
  local missing=()
  for cmd in "$@"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      missing+=("$cmd")
    fi
  done

  if [[ ${#missing[@]} -gt 0 ]]; then
    echo "::error::Missing required commands: ${missing[*]}" >&2
    exit 1
  fi
}

log_note() {
  echo "::notice::$*"
}

ensure_release_exists() {
  local tag="$1"
  local release_name="$2"
  local notes_file="${3:-}"

  require_cmd gh

  if gh release view "$tag" >/dev/null 2>&1; then
    log_note "Release $tag already exists"
    if [[ -n "$notes_file" && -f "$notes_file" ]]; then
      gh release edit "$tag" --title "$release_name" --notes-file "$notes_file"
    fi
  else
    log_note "Creating release $tag"
    if [[ -n "$notes_file" && -f "$notes_file" ]]; then
      gh release create "$tag" --title "$release_name" --notes-file "$notes_file"
    else
      gh release create "$tag" --title "$release_name" --notes "Release $tag"
    fi
  fi
}

asset_exists() {
  local tag="$1"
  local asset_name="$2"

  require_cmd gh jq

  gh release view "$tag" --json assets --jq '.assets[].name' 2>/dev/null | grep -Fxq "$asset_name"
}

dispatch_next_stage() {
  local event_type="$1"
  local tag="$2"
  local version="$3"

  if [[ -z "$event_type" ]]; then
    log_note "No dispatch event provided, skipping"
    return 0
  fi

  require_cmd gh jq

  log_note "Dispatching $event_type for $tag"

  # Build the full request body as JSON and pipe it to gh api
  # This ensures client_payload is sent as an object, not a string
  jq -nc \
    --arg event_type "$event_type" \
    --arg tag "$tag" \
    --arg version "$version" \
    '{event_type: $event_type, client_payload: {tag: $tag, version: $version}}' \
  | gh api "repos/${GITHUB_REPOSITORY}/dispatches" --input -
}

