#!/usr/bin/env bash
# Builds dist/B-303-<version>-macos-universal.pkg (installs VST3 + CLAP to /Library/Audio/Plug-Ins)
set -euo pipefail
version="$1"
cd "$(dirname "$0")/../.."
cargo xtask bundle-universal b303-plugin --release
mkdir -p dist
stage="target/stage"
rm -rf "$stage" && mkdir -p "$stage/vst3" "$stage/clap"
cp -R target/bundled/B-303.vst3 "$stage/vst3/"
cp -R target/bundled/B-303.clap "$stage/clap/"
codesign --force --deep --sign - "$stage/vst3/B-303.vst3" "$stage/clap/B-303.clap"

pkgbuild --root "$stage/vst3" --identifier dev.codekiste.b303.vst3 --version "$version" \
    --install-location "/Library/Audio/Plug-Ins/VST3" "$stage/b303-vst3.pkg"
pkgbuild --root "$stage/clap" --identifier dev.codekiste.b303.clap --version "$version" \
    --install-location "/Library/Audio/Plug-Ins/CLAP" "$stage/b303-clap.pkg"
productbuild --package "$stage/b303-vst3.pkg" --package "$stage/b303-clap.pkg" \
    "dist/B-303-$version-macos-universal.pkg"
ls -lh dist
