#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")/../.."

source .github/scripts/release_helpers.sh

version="${RELEASE_VERSION:-}"
changelog="${RELEASE_CHANGELOG:-}"

if [[ -z "$version" ]]; then
  log_error "RELEASE_VERSION is required"
  exit 1
fi

version="${version#v}"
tag="v${version}"

validate_semver_tag "$tag"

if grep -Eq "^## \\[$version\\]" CHANGELOG.md; then
  log_error "CHANGELOG.md already has a section for $version"
  exit 1
fi

export VERSION="$version"

perl -0pi -e 's/(\[workspace\.package\][\s\S]*?version\s*=\s*")[^"]+(")/$1$ENV{VERSION}$2/s' Cargo.toml

perl -0pi -e '
  s/(lla_plugin_interface = \{[^}]*version = ")[^"]+(")/$1$ENV{VERSION}$2/g;
  s/(lla_plugin_sdk = \{[^}]*version = ")[^"]+(")/$1$ENV{VERSION}$2/g;
  s/(lla_plugin_utils = \{[^}]*version = ")[^"]+(")/$1$ENV{VERSION}$2/g;
' lla/Cargo.toml

perl -0pi -e '
  s/(lla_plugin_interface = \{[^}]*version = ")[^"]+(")/$1$ENV{VERSION}$2/g;
  s/(lla_plugin_sdk = \{[^}]*version = ")[^"]+(")/$1$ENV{VERSION}$2/g;
' utils/Cargo.toml

perl -0pi -e '
  s/(lla_plugin_sdk_macros = \{[^}]*version = ")[^"]+(")/$1$ENV{VERSION}$2/g;
' sdk/Cargo.toml

perl -0pi -e '
  s/(lla_plugin_interface = \{[^}]*version = ")[^"]+(")/$1$ENV{VERSION}$2/g;
' macros/Cargo.toml

perl -0pi -e '
  s/(lla_plugin_interface = \{[^}]*version = ")[^"]+(")/$1$ENV{VERSION}$2/g;
  s/(lla_plugin_sdk = \{[^}]*version = ")[^"]+(")/$1$ENV{VERSION}$2/g;
' Cargo.toml

while IFS= read -r manifest; do
  perl -0pi -e 's/(^version\s*=\s*")[^"]+(")/$1$ENV{VERSION}$2/m' "$manifest"
done < <(find plugins -mindepth 2 -maxdepth 2 -name Cargo.toml | sort)

while IFS= read -r manifest; do
  perl -0pi -e 's/(^version\s*=\s*")[^"]+(")/$1$ENV{VERSION}$2/m' "$manifest"
done < <(find plugins -mindepth 2 -maxdepth 2 -name plugin.toml | sort)

tmp_changelog="$(mktemp)"
today="$(date +%F)"

if grep -Eq '^## \[Unreleased\]' CHANGELOG.md; then
  awk \
    -v version="$version" \
    -v today="$today" '
      /^## \[Unreleased\]/ {
        print "## [Unreleased]"
        print ""
        print "## [" version "] - " today
        in_unreleased = 1
        promoted = 1
        next
      }
      in_unreleased && /^## \[/ {
        print ""
        in_unreleased = 0
      }
      { print }
      END {
        if (promoted == 0) {
          exit 1
        }
      }
    ' CHANGELOG.md > "$tmp_changelog"
elif [[ -n "$changelog" ]]; then
  changelog_file="$(mktemp)"
  printf '%s\n' "$changelog" > "$changelog_file"

  awk \
    -v version="$version" \
    -v today="$today" \
    -v changelog_file="$changelog_file" '
      /^## \[/ && inserted == 0 {
        print "## [" version "] - " today
        print ""
        while ((getline line < changelog_file) > 0) {
          print line
        }
        close(changelog_file)
        print ""
        inserted = 1
      }
      { print }
      END {
        if (inserted == 0) {
          print ""
          print "## [" version "] - " today
          print ""
          while ((getline line < changelog_file) > 0) {
            print line
          }
          close(changelog_file)
        }
      }
    ' CHANGELOG.md > "$tmp_changelog"

  rm -f "$changelog_file"
else
  log_error "CHANGELOG.md must contain a ## [Unreleased] section, or RELEASE_CHANGELOG must be provided"
  rm -f "$tmp_changelog"
  exit 1
fi

mv "$tmp_changelog" CHANGELOG.md

validate_release_versions "$version"
validate_changelog_section "$version"
