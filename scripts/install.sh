#!/usr/bin/env sh
set -e

# wxec universal installer
# usage: curl -fsSL https://raw.githubusercontent.com/kaeinmotion/wxec/main/scripts/install.sh | sh

REPO="kaeinmotion/wxec"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  arm64|aarch64) ARCH="aarch64" ;;
  *) echo "unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

case "$OS" in
  linux) TARGET="${ARCH}-unknown-linux-gnu" ;;
  darwin) TARGET="${ARCH}-apple-darwin" ;;
  *) echo "unsupported os: $OS" >&2; exit 1 ;;
esac

TAG="${WXEC_VERSION:-v0.2.8}"
URL="https://github.com/${REPO}/releases/download/${TAG}/wxec-${TARGET}.tar.gz"

DEST_DIR="${WXEC_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$DEST_DIR"

echo "installing wxec ${TAG} (${TARGET})..."
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

if command -v curl >/dev/null 2>&1; then
  curl -fsSL "$URL" -o "$TMP_DIR/wxec.tar.gz"
elif command -v wget >/dev/null 2>&1; then
  wget -qO "$TMP_DIR/wxec.tar.gz" "$URL"
else
  echo "error: curl or wget required" >&2
  exit 1
fi

tar -xzf "$TMP_DIR/wxec.tar.gz" -C "$TMP_DIR"
install -m 755 "$TMP_DIR/wxec" "$DEST_DIR/wxec" 2>/dev/null || (cp "$TMP_DIR/wxec" "$DEST_DIR/wxec" && chmod +x "$DEST_DIR/wxec")

echo "wxec successfully installed to $DEST_DIR/wxec"
case ":$PATH:" in
  *":$DEST_DIR:"*) ;;
  *) echo "note: add $DEST_DIR to your PATH: export PATH=\"\$PATH:$DEST_DIR\"" ;;
esac
