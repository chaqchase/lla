#!/usr/bin/env bash

set -euo pipefail

REPO="chaqchase/lla"
DEFAULT_INSTALL_DIR="/usr/local/bin"
INSTALL_DIR="${LLA_INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
BINARY_NAME="${LLA_BINARY_NAME:-lla}"
TARGET_PATH="${INSTALL_DIR}/${BINARY_NAME}"
REQUESTED_VERSION="${LLA_VERSION:-}"

if [[ -t 1 ]] && [[ "${TERM:-}" != "dumb" ]] && [[ "${NO_COLOR:-}" == "" ]]; then
    BOLD="\033[1m"
    RESET="\033[0m"
    ACCENT="\033[38;5;39m"
    SUCCESS="\033[0;32m"
    INFO="\033[0;36m"
    ERROR="\033[0;31m"
    MUTED="\033[0;90m"
else
    BOLD=""
    RESET=""
    ACCENT=""
    SUCCESS=""
    INFO=""
    ERROR=""
    MUTED=""
fi

SUCCESS_ICON="${SUCCESS}[OK]${RESET}"
INFO_ICON="${INFO}[>>]${RESET}"
ERROR_ICON="${ERROR}[XX]${RESET}"

SPINNER_FRAMES=('-' '\' '|' '/')
HAS_TTY=0
if [[ -t 1 ]] && [[ -t 2 ]]; then
    HAS_TTY=1
fi

WORK_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t lla-install-XXXXXX)"
BINARY_PATH="${WORK_DIR}/lla"
SPINNER_PID=""
SPINNER_MESSAGE=""

cleanup() {
    stop_spinner >/dev/null 2>&1 || true
    if [[ -n "${WORK_DIR:-}" && -d "${WORK_DIR:-}" ]]; then
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

status_line() {
    local kind="$1"
    local message="$2"
    case "$kind" in
        success) echo -e "$SUCCESS_ICON $message" ;;
        info) echo -e "$INFO_ICON $message" ;;
        error) echo -e "$ERROR_ICON $message" ;;
    esac
}

banner() {
    local title="$1"
    local padding=6
    local width=$(( ${#title} + padding ))
    local line
    line=$(printf '%*s' "$width" '' | tr ' ' '=')
    echo -e "\n${ACCENT}${line}${RESET}"
    echo -e "${ACCENT}==${RESET}  ${BOLD}${title}${RESET}  ${ACCENT}==${RESET}"
    echo -e "${ACCENT}${line}${RESET}"
}

section() {
    local title="$1"
    echo -e "\n${ACCENT}== ${BOLD}${title}${RESET} ${ACCENT}==${RESET}"
}

bullet_line() {
    local label="$1"
    local value="$2"
    echo -e "  ${ACCENT}*${RESET} ${BOLD}${label}${RESET}: $value"
}

start_spinner() {
    local message="$1"
    SPINNER_MESSAGE="$message"
    if [[ $HAS_TTY -ne 1 ]]; then
        return
    fi
    (
        trap 'exit 0' TERM
        local i=0
        while true; do
            local frame="${SPINNER_FRAMES[$i]}"
            echo -ne "\r${ACCENT}${frame}${RESET} $SPINNER_MESSAGE"
            i=$(( (i + 1) % ${#SPINNER_FRAMES[@]} ))
            sleep 0.12
        done
    ) &
    SPINNER_PID=$!
}

stop_spinner() {
    if [[ -n "${SPINNER_PID:-}" ]]; then
        kill "$SPINNER_PID" >/dev/null 2>&1 || true
        wait "$SPINNER_PID" >/dev/null 2>&1 || true
        SPINNER_PID=""
        if [[ $HAS_TTY -eq 1 ]]; then
            printf "\r\033[K"
        fi
    fi
}

finish_spinner() {
    local kind="$1"
    local message="$2"
    stop_spinner
    status_line "$kind" "$message"
}

sanitize_filename() {
    echo "$1" | tr ' /' '__' | tr -cd '[:alnum:]_-.'
}

spinner_wrap() {
    local message="$1"; shift
    if [[ $HAS_TTY -eq 1 ]]; then
        local logfile="${WORK_DIR}/$(sanitize_filename "$message").log"
        start_spinner "$message"
        if "$@" >"$logfile" 2>&1; then
            finish_spinner success "$message"
            rm -f "$logfile"
        else
            finish_spinner error "$message"
            [[ -s "$logfile" ]] && cat "$logfile"
            exit 1
        fi
    else
        status_line info "$message"
        if "$@"; then
            status_line success "$message"
        else
            status_line error "$message"
            exit 1
        fi
    fi
}

fatal() {
    stop_spinner
    status_line error "$1"
    exit 1
}

ensure_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        fatal "Required command '$1' is not available."
    fi
}

normalize_version() {
    local value="$1"
    if [[ "$value" == v* ]]; then
        echo "$value"
    else
        echo "v${value}"
    fi
}

detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux) OS_LABEL="linux"; OS_DISPLAY="Linux" ;;
        Darwin) OS_LABEL="macos"; OS_DISPLAY="macOS" ;;
        *) fatal "Unsupported operating system: $os" ;;
    esac

    case "$arch" in
        x86_64) ARCH_LABEL="amd64"; ARCH_DISPLAY="x86_64" ;;
        aarch64|arm64) ARCH_LABEL="arm64"; ARCH_DISPLAY="arm64" ;;
        i386|i686) ARCH_LABEL="i686"; ARCH_DISPLAY="i686" ;;
        *) fatal "Unsupported architecture: $arch" ;;
    esac

    PLATFORM="lla-${OS_LABEL}-${ARCH_LABEL}"
    FRIENDLY_PLATFORM="${OS_DISPLAY} (${ARCH_DISPLAY})"
}

determine_version() {
    if [[ -n "$REQUESTED_VERSION" ]]; then
        VERSION="$(normalize_version "$REQUESTED_VERSION")"
        return
    fi

    local url="https://api.github.com/repos/${REPO}/releases/latest"
    VERSION="$(curl -fsSL "$url" | grep '\"tag_name\":' | sed -E 's/.*"([^"]+)".*/\1/')"
    if [[ -z "$VERSION" ]]; then
        fatal "Failed to determine the latest release tag."
    fi
}

download_binary() {
    local download_url="https://github.com/${REPO}/releases/download/${VERSION}/${PLATFORM}"
    curl -fsSL "$download_url" -o "$BINARY_PATH"
}

calculate_sha256() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        fatal "sha256sum or shasum is required to verify checksums."
    fi
}

verify_checksum() {
    local checksum_url="https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS"
    local checksum_file="${WORK_DIR}/SHA256SUMS"
    curl -fsSL "$checksum_url" -o "$checksum_file"

    local expected
    expected="$(grep "/${PLATFORM}\$" "$checksum_file" | awk '{print $1}' | head -n 1 || true)"
    if [[ -z "$expected" ]]; then
        fatal "Could not find a checksum entry for ${PLATFORM}."
    fi

    local actual
    actual="$(calculate_sha256 "$BINARY_PATH")"
    if [[ "$actual" != "$expected" ]]; then
        fatal "Checksum mismatch (expected ${expected}, got ${actual})."
    fi
}

install_binary() {
    chmod +x "$BINARY_PATH"
    local target_dir
    target_dir="$(dirname "$TARGET_PATH")"

    if [[ ! -d "$target_dir" ]]; then
        if [[ -w "$(dirname "$target_dir")" ]]; then
            mkdir -p "$target_dir"
        elif command -v sudo >/dev/null 2>&1; then
            status_line info "Creating ${target_dir} (sudo)"
            sudo mkdir -p "$target_dir"
        else
            fatal "Cannot create ${target_dir}. Re-run with sudo."
        fi
    fi

    local temp_binary="${WORK_DIR}/lla-ready"
    cp "$BINARY_PATH" "$temp_binary"
    chmod 755 "$temp_binary"

    if [[ -w "$target_dir" && ( ! -e "$TARGET_PATH" || -w "$TARGET_PATH" ) ]]; then
        if command -v install >/dev/null 2>&1; then
            install -m 755 "$temp_binary" "$TARGET_PATH"
        else
            cp "$temp_binary" "$TARGET_PATH"
            chmod 755 "$TARGET_PATH"
        fi
    else
        if ! command -v sudo >/dev/null 2>&1; then
            fatal "Write permission to ${target_dir} is required (try running with sudo)."
        fi
        status_line info "Elevated permissions required for ${target_dir}"
        if command -v install >/dev/null 2>&1; then
            sudo install -m 755 "$temp_binary" "$TARGET_PATH"
        else
            sudo cp "$temp_binary" "$TARGET_PATH"
            sudo chmod 755 "$TARGET_PATH"
        fi
    fi
}

main() {
    banner "lla installer"
    
    section "Preflight"
    ensure_command curl

    spinner_wrap "Detecting platform" detect_platform
    spinner_wrap "Resolving release tag" determine_version

    section "Environment"
    bullet_line "Platform" "$FRIENDLY_PLATFORM"
    bullet_line "Release" "$VERSION"
    bullet_line "Destination" "$TARGET_PATH"

    section "Download"
    spinner_wrap "Downloading ${PLATFORM}" download_binary

    section "Verification"
    spinner_wrap "Verifying checksum" verify_checksum

    section "Installation"
    status_line info "Installing to ${TARGET_PATH}"
    install_binary
    status_line success "Installed lla ${VERSION} to ${TARGET_PATH}"

    section "Summary"
    bullet_line "Binary" "$TARGET_PATH"
    bullet_line "Version" "$VERSION"
    status_line success "Run 'lla --version' to confirm the installation."
    status_line info "Next step: run 'lla init' for the guided setup."
}

main "$@"
