#!/usr/bin/env bash
# Amber Shield Installer — macOS / Linux
# Usage: curl -fsSL https://ambershield.app/install.sh | sh
# Requires: macOS 13+ or Linux x64, curl

set -euo pipefail

REPO="aurora-ember-bio-lab/Amber-Shield-releases"
VERSION="v0.1.0"
INSTALL_DIR="${HOME}/.local/bin"
BINARY="amber-shield-lite"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${CYAN}[amber-shield]${NC} $1"; }
ok()    { echo -e "${GREEN}[amber-shield]${NC} $1"; }
warn()  { echo -e "${YELLOW}[amber-shield]${NC} $1"; }
err()   { echo -e "${RED}[amber-shield] ERROR:${NC} $1"; }

echo ""
echo -e "${YELLOW}  ╔══════════════════════════════════════╗${NC}"
echo -e "${YELLOW}  ║     AMBER SHIELD INSTALLER           ║${NC}"
echo -e "${YELLOW}  ║     Local Security Intelligence      ║${NC}"
echo -e "${YELLOW}  ╚══════════════════════════════════════╝${NC}"
echo ""

# Detect OS and arch
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin)
        PLATFORM="apple-darwin"
        if [ "$ARCH" = "arm64" ]; then
            ARCH_NAME="aarch64"
        else
            ARCH_NAME="x86_64"
        fi
        ;;
    Linux)
        PLATFORM="unknown-linux-gnu"
        ARCH_NAME="x86_64"
        ;;
    *)
        err "Unsupported OS: $OS"
        err "Download manually from: https://github.com/$REPO/releases"
        exit 1
        ;;
esac

if [ "$ARCH_NAME" != "x86_64" ] && [ "$ARCH_NAME" != "aarch64" ]; then
    err "Unsupported architecture: $ARCH"
    exit 1
fi

info "Detected: $OS ($ARCH_NAME)"

# Ensure install directory exists
mkdir -p "$INSTALL_DIR"

# Try brew on macOS
if command -v brew &>/dev/null && [ "$OS" = "Darwin" ]; then
    info "Homebrew detected. Trying brew install..."
    if brew install --cask amber-shield 2>/dev/null; then
        ok "Installed via Homebrew!"
        echo ""
        echo -e "  Launch: ${GREEN}amber-shield-lite${NC}"
        echo ""
        exit 0
    fi
    warn "brew install failed, falling back to direct download..."
fi

# Download binary from GitHub Releases
BINARY_NAME="amber-shield-lite-${ARCH_NAME}-${PLATFORM}"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$BINARY_NAME"
TARGET="$INSTALL_DIR/$BINARY"

info "Downloading $BINARY_NAME ..."
if ! curl -fsSL "$DOWNLOAD_URL" -o "$TARGET"; then
    err "Download failed from: $DOWNLOAD_URL"
    err "Try downloading manually from: https://github.com/$REPO/releases"
    exit 1
fi

chmod +x "$TARGET"

# Add to PATH if needed
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    SHELL_RC=""
    if [ -f "$HOME/.bashrc" ]; then SHELL_RC="$HOME/.bashrc"
    elif [ -f "$HOME/.zshrc" ]; then SHELL_RC="$HOME/.zshrc"
    fi

    if [ -n "$SHELL_RC" ]; then
        if ! grep -q "$INSTALL_DIR" "$SHELL_RC" 2>/dev/null; then
            echo "export PATH=\"\$HOME/.local/bin:\$PATH\"" >> "$SHELL_RC"
            info "Added $INSTALL_DIR to PATH in $SHELL_RC"
        fi
    fi
fi

ok "Amber Shield installed to $TARGET"
echo ""
echo -e "  Launch: ${GREEN}$BINARY${NC}"
echo -e "  Source: https://github.com/$REPO"
echo -e "  License: MIT"
echo ""
