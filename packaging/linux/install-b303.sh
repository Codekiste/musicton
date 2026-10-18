#!/usr/bin/env bash
# Installs the B-303 plug-in to ~/.vst3 and ~/.clap. Run from the unpacked archive.
#   ./install.sh             install
#   ./install.sh --uninstall remove it again
set -euo pipefail
cd "$(dirname "$0")"
if [[ "${1:-}" == "--uninstall" ]]; then
    rm -rf "$HOME/.vst3/B-303.vst3" "$HOME/.clap/B-303.clap"
    echo "B-303 removed."
    exit 0
fi
mkdir -p "$HOME/.vst3" "$HOME/.clap"
rm -rf "$HOME/.vst3/B-303.vst3"
cp -r B-303.vst3 "$HOME/.vst3/"
cp B-303.clap "$HOME/.clap/"
echo "B-303 installed to ~/.vst3 and ~/.clap. Rescan plug-ins in your DAW."
