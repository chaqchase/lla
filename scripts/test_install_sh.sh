#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
test_dir="$(mktemp -d)"
trap 'rm -rf "$test_dir"' EXIT

mkdir -p "$test_dir/bin"

cat >"$test_dir/bin/uname" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
    -s) printf '%s\n' Linux ;;
    -m) printf '%s\n' x86_64 ;;
    *) exit 1 ;;
esac
EOF

cat >"$test_dir/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

output=""
url=""
while (( $# > 0 )); do
    case "$1" in
        -o)
            output="$2"
            shift 2
            ;;
        http://*|https://*)
            url="$1"
            shift
            ;;
        *)
            shift
            ;;
    esac
done

case "$url" in
    */releases/latest)
        printf '%s\n' '{"tag_name":"v0.0.0-test"}'
        ;;
    */SHA256SUMS)
        printf '%s  %s\n' "$MOCK_SHA256" lla-linux-amd64 >"$output"
        ;;
    */lla-linux-amd64)
        printf '%s\n' 'fake lla binary' >"$output"
        ;;
    *)
        printf 'Unexpected curl URL: %s\n' "$url" >&2
        exit 1
        ;;
esac
EOF

cat >"$test_dir/bin/sudo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$MOCK_SUDO_LOG"
EOF

chmod +x "$test_dir/bin/uname" "$test_dir/bin/curl" "$test_dir/bin/sudo"

if command -v sha256sum >/dev/null 2>&1; then
    mock_sha256="$(printf '%s\n' 'fake lla binary' | sha256sum | awk '{print $1}')"
else
    mock_sha256="$(printf '%s\n' 'fake lla binary' | shasum -a 256 | awk '{print $1}')"
fi

run_installer() {
    local mode="$1"
    local output_file="$test_dir/${mode}.out"
    local sudo_log="$test_dir/${mode}.sudo"

    : >"$sudo_log"
    if [[ "$mode" == direct ]]; then
        PATH="$test_dir/bin:$PATH" \
            MOCK_SHA256="$mock_sha256" \
            MOCK_SUDO_LOG="$sudo_log" \
            bash "$repo_root/install.sh" >"$output_file"
    else
        PATH="$test_dir/bin:$PATH" \
            MOCK_SHA256="$mock_sha256" \
            MOCK_SUDO_LOG="$sudo_log" \
            bash -s -- <"$repo_root/install.sh" >"$output_file"
    fi

    grep -Fq 'lla v0.0.0-test has been installed successfully!' "$output_file"
    grep -Fq 'mv ' "$sudo_log"
}

bash -n "$repo_root/install.sh"
run_installer direct
run_installer piped

sourced_output="$(bash -c 'source "$1"; printf sourced' _ "$repo_root/install.sh")"
[[ "$sourced_output" == sourced ]]

printf '%s\n' 'install.sh tests passed'
