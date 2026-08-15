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

log_error() {
  echo "::error::$*" >&2
}

workspace_version() {
  awk '
    /^\[workspace\.package\]/ { in_section = 1; next }
    /^\[/ && in_section { exit }
    in_section && /^version[[:space:]]*=/ {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' Cargo.toml
}

validate_semver_tag() {
  local tag="$1"

  if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
    log_error "Release tag must be semver with a v prefix, for example v1.2.3. Got: $tag"
    exit 1
  fi
}

validate_tag_on_main() {
  local tag="$1"
  local main_ref="${2:-origin/main}"

  require_cmd git

  git fetch --tags origin main >/dev/null 2>&1

  local tag_sha
  tag_sha="$(git rev-list -n 1 "$tag")"

  if ! git merge-base --is-ancestor "$tag_sha" "$main_ref"; then
    log_error "Tag $tag does not point to a commit reachable from $main_ref"
    exit 1
  fi
}

validate_changelog_section() {
  local version="$1"

  if ! grep -Eq "^## \\[$version\\]" CHANGELOG.md; then
    log_error "CHANGELOG.md is missing a section for $version"
    exit 1
  fi
}

validate_release_versions() {
  local version="$1"
  local current_version

  current_version="$(workspace_version)"
  if [[ "$current_version" != "$version" ]]; then
    log_error "Workspace version is $current_version, expected $version"
    exit 1
  fi

  if ! grep -Eq 'lla_plugin_interface = \{[^}]*version = "'"$version"'"' lla/Cargo.toml; then
    log_error "lla/Cargo.toml must depend on lla_plugin_interface $version"
    exit 1
  fi

  if ! grep -Eq 'lla_plugin_utils = \{[^}]*version = "'"$version"'"' lla/Cargo.toml; then
    log_error "lla/Cargo.toml must depend on lla_plugin_utils $version"
    exit 1
  fi

  if ! grep -Eq 'lla_plugin_interface = \{[^}]*version = "'"$version"'"' lla_plugin_utils/Cargo.toml; then
    log_error "lla_plugin_utils/Cargo.toml must depend on lla_plugin_interface $version"
    exit 1
  fi

  local manifest plugin_version
  while IFS= read -r manifest; do
    plugin_version="$(awk -F '"' '/^version[[:space:]]*=/ { print $2; exit }' "$manifest")"
    if [[ "$plugin_version" != "$version" ]]; then
      log_error "$manifest is version $plugin_version, expected $version"
      exit 1
    fi
  done < <(find plugins -mindepth 2 -maxdepth 2 -name Cargo.toml | sort)

  while IFS= read -r manifest; do
    plugin_version="$(awk -F '"' '/^version[[:space:]]*=/ { print $2; exit }' "$manifest")"
    if [[ "$plugin_version" != "$version" ]]; then
      log_error "$manifest is version $plugin_version, expected $version"
      exit 1
    fi
  done < <(find plugins -mindepth 2 -maxdepth 2 -name plugin.toml | sort)
}

crate_version_published() {
  local crate_name="$1"
  local version="$2"

  require_cmd curl

  curl -A "lla-release-workflow" -fsSL "https://crates.io/api/v1/crates/${crate_name}/${version}" >/dev/null 2>&1
}

validate_crates_io_state() {
  local version="$1"
  local interface_published=false
  local utils_published=false
  local cli_published=false

  if crate_version_published "lla_plugin_interface" "$version"; then
    interface_published=true
  fi
  if crate_version_published "lla_plugin_utils" "$version"; then
    utils_published=true
  fi
  if crate_version_published "lla" "$version"; then
    cli_published=true
  fi

  if [[ "$utils_published" == "true" && "$interface_published" != "true" ]]; then
    log_error "lla_plugin_utils $version exists on crates.io but lla_plugin_interface $version does not"
    exit 1
  fi

  if [[ "$cli_published" == "true" && ( "$interface_published" != "true" || "$utils_published" != "true" ) ]]; then
    log_error "lla $version exists on crates.io before its internal dependencies; this release cannot resume safely"
    exit 1
  fi
}

write_expected_assets() {
  local version="$1"
  local output_file="$2"

  cat > "$output_file" <<EOF
lla-linux-amd64
lla-linux-amd64-musl
lla-linux-arm64
lla-linux-arm64-musl
lla-linux-i686
lla-macos-amd64
lla-macos-arm64
plugins-linux-amd64.tar.gz
plugins-linux-amd64.zip
plugins-linux-arm64.tar.gz
plugins-linux-arm64.zip
plugins-linux-i686.tar.gz
plugins-linux-i686.zip
plugins-macos-amd64.tar.gz
plugins-macos-amd64.zip
plugins-macos-arm64.tar.gz
plugins-macos-arm64.zip
lla_${version}_amd64.deb
lla-${version}-1.x86_64.rpm
lla-${version}-r0.x86_64.apk
lla-${version}-1-x86_64.pkg.tar.zst
lla_${version}_arm64.deb
lla-${version}-1.aarch64.rpm
lla-${version}-r0.aarch64.apk
lla-${version}-1-aarch64.pkg.tar.zst
lla_${version}_i386.deb
lla-${version}-1.i686.rpm
lla-${version}-r0.x86.apk
lla-${version}-1-i686.pkg.tar.zst
themes.zip
SHA256SUMS
EOF
}

check_expected_files() {
  local expected_file="$1"
  local asset_dir="$2"
  local missing=0
  local asset

  while IFS= read -r asset; do
    [[ -z "$asset" ]] && continue
    if [[ ! -f "$asset_dir/$asset" ]]; then
      log_error "Missing expected release asset: $asset"
      missing=$((missing + 1))
    fi
  done < "$expected_file"

  if [[ "$missing" -gt 0 ]]; then
    exit 1
  fi
}

generate_checksums() {
  local asset_dir="$1"
  local output_file="$2"

  require_cmd sha256sum

  (
    cd "$asset_dir"
    find . -maxdepth 1 -type f ! -name SHA256SUMS -print0 \
      | sort -z \
      | xargs -0 sha256sum \
      | sed 's|  \./|  |'
  ) > "$output_file"
}

create_release_notes() {
  local tag="$1"
  local version="$2"
  local checksums_file="$3"
  local output_file="$4"

  {
    echo "# Release ${tag}"
    echo
    echo "## Changelog"
    echo
    sed -n "/^## \\[${version}\\]/,/^## \\[/p" CHANGELOG.md | sed '$d'
    echo
    echo "## SHA256 Checksums"
    echo
    echo '```'
    cat "$checksums_file"
    echo '```'
  } > "$output_file"
}

ensure_release_exists() {
  local tag="$1"
  local release_name="$2"
  local notes_file="${3:-}"
  local draft="${4:-false}"

  require_cmd gh

  if gh release view "$tag" >/dev/null 2>&1; then
    log_note "Release $tag already exists"
    if [[ -n "$notes_file" && -f "$notes_file" ]]; then
      gh release edit "$tag" --title "$release_name" --notes-file "$notes_file"
    fi
  else
    log_note "Creating release $tag"
    if [[ -n "$notes_file" && -f "$notes_file" ]]; then
      if [[ "$draft" == "true" ]]; then
        gh release create "$tag" --verify-tag --draft --title "$release_name" --notes-file "$notes_file"
      else
        gh release create "$tag" --verify-tag --title "$release_name" --notes-file "$notes_file"
      fi
    else
      if [[ "$draft" == "true" ]]; then
        gh release create "$tag" --verify-tag --draft --title "$release_name" --notes "Release $tag"
      else
        gh release create "$tag" --verify-tag --title "$release_name" --notes "Release $tag"
      fi
    fi
  fi
}

asset_exists() {
  local tag="$1"
  local asset_name="$2"

  require_cmd gh jq

  gh release view "$tag" --json assets --jq '.assets[].name' 2>/dev/null | grep -Fxq "$asset_name"
}

upload_asset_if_missing() {
  local tag="$1"
  local asset_path="$2"
  local filename

  filename="$(basename "$asset_path")"
  if asset_exists "$tag" "$filename"; then
    log_note "Skipping existing asset $filename"
  else
    gh release upload "$tag" "$asset_path"
  fi
}

publish_release() {
  local tag="$1"
  local title="$2"
  local notes_file="$3"

  require_cmd gh

  gh release edit "$tag" --title "$title" --notes-file "$notes_file" --draft=false --latest
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
