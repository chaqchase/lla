#!/usr/bin/env bash

set -e

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
        *)
            print_error "Unsupported operating system: $OS"
            exit 1
            ;;
    esac

    case "$ARCH" in
        x86_64)  ARCH="amd64" ;;
        aarch64) ARCH="arm64" ;;
        arm64)   ARCH="arm64" ;;
        i386)    ARCH="i686" ;;
        i686)    ARCH="i686" ;;
        *)
            print_error "Unsupported architecture: $ARCH"
            exit 1
            ;;
    esac

    PLATFORM="lla-${OS}-${ARCH}"
}

get_latest_version() {
    LATEST_RELEASE_URL="https://api.github.com/repos/chaqchase/lla/releases/latest"
    VERSION=$(curl -s $LATEST_RELEASE_URL | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$VERSION" ]; then
        print_error "Failed to fetch latest version"
        exit 1
    fi
}

download_binary() {
    print_step "Downloading lla ${VERSION} for ${OS}-${ARCH}..."
    
    DOWNLOAD_URL="https://github.com/chaqchase/lla/releases/download/${VERSION}/${PLATFORM}"
    TMP_DIR=$(mktemp -d)
    curl -L "$DOWNLOAD_URL" -o "${TMP_DIR}/lla"
    
    if [ $? -ne 0 ]; then
        print_error "Failed to download binary"
        rm -rf "$TMP_DIR"
        exit 1
    fi
}

verify_checksum() {
    print_step "Verifying checksum..."
    
    CHECKSUM_URL="https://github.com/chaqchase/lla/releases/download/${VERSION}/SHA256SUMS"
    curl -L "$CHECKSUM_URL" -o "${TMP_DIR}/SHA256SUMS"
    
    cd "$TMP_DIR"
    mkdir "${PLATFORM}"
    cp lla "${PLATFORM}/${PLATFORM}"
    if ! sha256sum -c --ignore-missing SHA256SUMS; then
        print_error "Checksum verification failed"
        cd - > /dev/null
        rm -rf "$TMP_DIR"
        exit 1
    fi
    cd - > /dev/null
}

install_binary() {
    print_step "Installing lla to /usr/local/bin..."
    
    sudo mkdir -p /usr/local/bin
    sudo chmod +x "${TMP_DIR}/lla"
    sudo mv "${TMP_DIR}/lla" /usr/local/bin/
    rm -rf "$TMP_DIR"
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

main
