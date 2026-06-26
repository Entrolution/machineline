#!/usr/bin/env bash
#
# install.sh — download the latest machineline release binary and wire it into Claude Code.
#
#   curl -fsSL https://raw.githubusercontent.com/Entrolution/machineline/main/install.sh | bash
#
# Or run it from a clone. Overrides (env):
#   MACHINELINE_BIN_DIR=<dir>   where to install the binary (default: ~/.local/bin)
#
# machineline is macOS-only — it reads pmset / the SMC / vm_stat, which don't exist elsewhere.
#
set -euo pipefail

REPO="Entrolution/machineline"
INSTALL_DIR="${MACHINELINE_BIN_DIR:-$HOME/.local/bin}"

os="$(uname -s)"
arch="$(uname -m)"
if [ "$os" != "Darwin" ]; then
  echo "error: machineline is macOS-only (it reads pmset/SMC/vm_stat); detected $os" >&2
  exit 1
fi
case "$arch" in
  arm64) target="aarch64-apple-darwin" ;;
  x86_64) target="x86_64-apple-darwin" ;;
  *) echo "error: unsupported macOS arch: $arch" >&2; exit 1 ;;
esac

asset="machineline-${target}.tar.gz"
url="https://github.com/${REPO}/releases/latest/download/${asset}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "downloading ${url}"
curl -fsSL "$url" -o "$tmp/$asset"
tar -xzf "$tmp/$asset" -C "$tmp"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$tmp/machineline" "$INSTALL_DIR/machineline"
echo "installed → $INSTALL_DIR/machineline"
echo

# Wire it into ~/.claude/settings.json (backs up first, captures any existing status line).
"$INSTALL_DIR/machineline" install

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo; echo "note: $INSTALL_DIR is not on your PATH — add it to run 'machineline check' directly." ;;
esac
