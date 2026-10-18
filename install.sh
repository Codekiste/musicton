#!/usr/bin/env bash
# Builds Musicton (and optionally the B-303 plug-in) and installs it for the current user.
#   ./install.sh            Musicton
#   ./install.sh --plugin   Musicton + B-303 VST3/CLAP into ~/.vst3 and ~/.clap
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release -p musicton
install -Dm755 target/release/musicton "$HOME/.local/bin/musicton"
install -Dm755 target/release/musicton-mcp "$HOME/.local/bin/musicton-mcp"
install -Dm644 assets/musicton.desktop "$HOME/.local/share/applications/musicton.desktop"
install -Dm644 assets/musicton.svg "$HOME/.local/share/icons/hicolor/scalable/apps/musicton.svg"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Installed Musicton: run 'musicton' or start it from your application menu."
if [[ "${1:-}" == "--plugin" ]]; then
    cargo xtask bundle b303-plugin --release
    mkdir -p "$HOME/.vst3" "$HOME/.clap"
    rm -rf "$HOME/.vst3/B-303.vst3"
    cp -r target/bundled/B-303.vst3 "$HOME/.vst3/"
    cp target/bundled/B-303.clap "$HOME/.clap/"
    echo "Installed the B-303 plug-in to ~/.vst3 and ~/.clap - rescan plug-ins in your DAW."
fi
