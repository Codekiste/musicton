#!/usr/bin/env bash
# Installs Musicton for the current user (no root needed). Run from the unpacked archive.
#   ./install.sh             install to ~/.local
#   ./install.sh --uninstall remove it again
set -euo pipefail
cd "$(dirname "$0")"
bin="$HOME/.local/bin"; apps="$HOME/.local/share/applications"; icons="$HOME/.local/share/icons/hicolor/scalable/apps"
if [[ "${1:-}" == "--uninstall" ]]; then
    rm -f "$bin/musicton" "$bin/musicton-mcp" "$bin/musicton-plughost" "$apps/musicton.desktop" "$icons/musicton.svg"
    echo "Musicton removed. Your projects and samples in ~/.local/share/musicton were kept."
    exit 0
fi
install -Dm755 musicton "$bin/musicton"
install -Dm755 musicton-mcp "$bin/musicton-mcp"
install -Dm755 musicton-plughost "$bin/musicton-plughost"
install -Dm644 musicton.desktop "$apps/musicton.desktop"
install -Dm644 musicton.svg "$icons/musicton.svg"
update-desktop-database "$apps" 2>/dev/null || true
echo "Musicton installed. Start it from your application menu or run 'musicton'."
case ":$PATH:" in *":$bin:"*) ;; *) echo "Note: $bin is not in your PATH." ;; esac
