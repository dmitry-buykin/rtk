#!/bin/sh
# rtk installer - https://github.com/rtk-ai/rtk
# Usage: curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/refs/heads/master/install.sh | sh

set -e

REPO="rtk-ai/rtk"
BINARY_NAME="rtk"
INSTALL_DIR="${RTK_INSTALL_DIR:-$HOME/.local/bin}"
SKIP_CHECKSUM="${RTK_SKIP_CHECKSUM:-0}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() {
    printf "${GREEN}[INFO]${NC} %s\n" "$1"
}

warn() {
    printf "${YELLOW}[WARN]${NC} %s\n" "$1"
}

error() {
    printf "${RED}[ERROR]${NC} %s\n" "$1"
    exit 1
}

# Detect OS
detect_os() {
    case "$(uname -s)" in
        Linux*)  OS="linux";;
        Darwin*) OS="darwin";;
        *)       error "Unsupported operating system: $(uname -s)";;
    esac
}

# Detect architecture
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  ARCH="x86_64";;
        arm64|aarch64) ARCH="aarch64";;
        *)             error "Unsupported architecture: $(uname -m)";;
    esac
}

# Get latest release version
get_latest_version() {
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$VERSION" ]; then
        error "Failed to get latest version"
    fi
}

# Build target triple
get_target() {
    case "$OS" in
        linux)
            TARGET="${ARCH}-unknown-linux-gnu"
            ;;
        darwin)
            TARGET="${ARCH}-apple-darwin"
            ;;
    esac
}

# Compute SHA-256 hash (supports Linux/macOS)
sha256_file() {
    file="$1"

    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
        return 0
    fi

    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
        return 0
    fi

    return 1
}

# Download and install
install() {
    info "Detected: $OS $ARCH"
    info "Target: $TARGET"
    info "Version: $VERSION"

    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${BINARY_NAME}-${TARGET}.tar.gz"
    TEMP_DIR=$(mktemp -d)
    ARCHIVE="${TEMP_DIR}/${BINARY_NAME}.tar.gz"

    info "Downloading from: $DOWNLOAD_URL"
    if ! curl -fsSL "$DOWNLOAD_URL" -o "$ARCHIVE"; then
        error "Failed to download binary"
    fi

    if [ "$SKIP_CHECKSUM" = "1" ]; then
        warn "Skipping checksum verification (RTK_SKIP_CHECKSUM=1)"
    else
        CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${VERSION}/checksums.txt"
        CHECKSUMS_FILE="${TEMP_DIR}/checksums.txt"
        ARCHIVE_NAME="${BINARY_NAME}-${TARGET}.tar.gz"

        info "Verifying checksum..."
        if ! curl -fsSL "$CHECKSUMS_URL" -o "$CHECKSUMS_FILE"; then
            error "Failed to download checksums.txt (set RTK_SKIP_CHECKSUM=1 to bypass)"
        fi

        EXPECTED_SHA=$(grep " ${ARCHIVE_NAME}$" "$CHECKSUMS_FILE" | head -n 1 | awk '{print $1}')
        if [ -z "$EXPECTED_SHA" ]; then
            error "No checksum entry for ${ARCHIVE_NAME} (set RTK_SKIP_CHECKSUM=1 to bypass)"
        fi

        ACTUAL_SHA=$(sha256_file "$ARCHIVE")
        if [ -z "$ACTUAL_SHA" ]; then
            error "No SHA-256 tool found (need sha256sum or shasum, or set RTK_SKIP_CHECKSUM=1)"
        fi

        if [ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]; then
            error "Checksum mismatch for ${ARCHIVE_NAME}"
        fi
        info "Checksum verified"
    fi

    info "Extracting..."
    tar -xzf "$ARCHIVE" -C "$TEMP_DIR"

    mkdir -p "$INSTALL_DIR"
    mv "${TEMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/"

    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

    # Cleanup
    rm -rf "$TEMP_DIR"

    info "Successfully installed ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"
}

# Verify installation
verify() {
    if command -v "$BINARY_NAME" >/dev/null 2>&1; then
        info "Verification: $($BINARY_NAME --version)"
    else
        warn "Binary installed but not in PATH. Add to your shell profile:"
        warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
}

main() {
    info "Installing $BINARY_NAME..."

    detect_os
    detect_arch
    get_target
    get_latest_version
    install
    verify

    echo ""
    info "Installation complete! Run '$BINARY_NAME --help' to get started."
}

main
