#!/usr/bin/env bash

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' 

print_step() {
    echo -e "${BLUE}==>${NC} $1"
}

print_success() {
    echo -e "${GREEN}==>${NC} $1"
}

print_error() {
    echo -e "${RED}==>${NC} $1"
}

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)     OS="linux" ;;
        Darwin)    OS="macos" ;;
        NetBSD)    OS="netbsd" ;;
        *)
            print_error "Unsupported operating system: $OS"
            exit 1
            ;;
    esac

    case "$ARCH" in
        x86_64)  ARCH="amd64" ;;
        amd64)   ARCH="amd64" ;;
        aarch64) ARCH="arm64" ;;
        arm64)   ARCH="arm64" ;;
        i386)    ARCH="i686" ;;
        i686)    ARCH="i686" ;;
        *)
            print_error "Unsupported architecture: $ARCH"
            exit 1
            ;;
    esac

    if [ "$OS" = "netbsd" ] && [ "$ARCH" != "amd64" ]; then
        print_error "Official NetBSD binaries are available only for amd64"
        exit 1
    fi

    LIBC=""
    if [ "$OS" = "linux" ]; then
        if { command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; } \
            || compgen -G '/lib/ld-musl-*.so.1' >/dev/null; then
            LIBC="musl"
            if [ "$ARCH" = "i686" ]; then
                print_error "Static musl binaries are available only for amd64 and arm64"
                exit 1
            fi
        else
            LIBC="gnu"
        fi
    fi

    if [ "$LIBC" = "musl" ]; then
        PLATFORM="lla-${OS}-${ARCH}-musl"
    else
        PLATFORM="lla-${OS}-${ARCH}"
    fi
}

get_latest_version() {
    LATEST_RELEASE_URL="https://api.github.com/repos/chaqchase/lla/releases/latest"
    local json
    json="$(curl -fsSL "$LATEST_RELEASE_URL")"
    VERSION="$(
        printf '%s\n' "$json" \
            | grep -m 1 '"tag_name":' \
            | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/' \
            || true
    )"
    if [ -z "$VERSION" ]; then
        print_error "Failed to fetch latest version"
        exit 1
    fi
}

download_binary() {
    print_step "Downloading lla ${VERSION} for ${OS}-${ARCH}..."
    
    DOWNLOAD_URL="https://github.com/chaqchase/lla/releases/download/${VERSION}/${PLATFORM}"
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    if ! curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/lla"; then
        print_error "Failed to download binary"
        exit 1
    fi
}

verify_checksum() {
    print_step "Verifying checksum..."
    
    CHECKSUM_URL="https://github.com/chaqchase/lla/releases/download/${VERSION}/SHA256SUMS"
    if ! curl -fsSL "$CHECKSUM_URL" -o "${TMP_DIR}/SHA256SUMS"; then
        print_error "Failed to download checksum manifest (SHA256SUMS)"
        exit 1
    fi

    expected_line="$(grep -E "[[:space:]]${PLATFORM}$" "${TMP_DIR}/SHA256SUMS" | head -n 1 || true)"
    expected="$(printf '%s' "$expected_line" | awk '{print $1}' || true)"
    expected="${expected##*:}"

    if [ -z "$expected" ]; then
        print_error "No checksum entry found for ${PLATFORM} in SHA256SUMS"
        exit 1
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "${TMP_DIR}/lla" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        actual="$(shasum -a 256 "${TMP_DIR}/lla" | awk '{print $1}')"
    elif command -v sha256 >/dev/null 2>&1; then
        actual="$(sha256 -q "${TMP_DIR}/lla")"
    else
        print_error "No supported SHA-256 checksum utility is available"
        exit 1
    fi

    actual_lc="$(printf '%s' "$actual" | tr '[:upper:]' '[:lower:]')"
    expected_lc="$(printf '%s' "$expected" | tr '[:upper:]' '[:lower:]')"
    if [ "$actual_lc" != "$expected_lc" ]; then
        print_error "Checksum verification failed"
        print_error "Expected: ${expected}"
        print_error "Actual:   ${actual}"
        exit 1
    fi
}

install_binary() {
    print_step "Installing lla to /usr/local/bin..."
    
    sudo mkdir -p /usr/local/bin
    sudo chmod +x "${TMP_DIR}/lla"
    sudo mv "${TMP_DIR}/lla" /usr/local/bin/
    rm -rf "$TMP_DIR"
    trap - EXIT
    print_success "lla ${VERSION} has been installed successfully!"
    print_success "Run 'lla init' to create your configuration file"
}

main() {
    print_step "Installing lla..."
    if ! command -v curl >/dev/null 2>&1; then
        print_error "curl is required but not installed"
        exit 1
    fi
    
    detect_platform
    get_latest_version
    download_binary
    verify_checksum
    install_binary
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
