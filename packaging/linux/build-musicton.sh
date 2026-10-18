#!/usr/bin/env bash
# Builds dist/musicton-<version>-linux-x86_64.tar.gz and dist/musicton_<version>_amd64.deb
set -euo pipefail
version="$1"
cd "$(dirname "$0")/../.."
cargo build --release -p musicton
mkdir -p dist

# tar.gz with a per-user installer
name="musicton-$version-linux-x86_64"
stage="target/stage/$name"
rm -rf "$stage" && mkdir -p "$stage"
cp target/release/musicton target/release/musicton-mcp "$stage/"
cp assets/musicton.desktop assets/musicton.svg LICENSE "$stage/"
cp packaging/linux/install-musicton.sh "$stage/install.sh"
tar -C target/stage -czf "dist/$name.tar.gz" "$name"

# .deb (system-wide install)
deb="target/stage/deb"
rm -rf "$deb"
install -Dm755 target/release/musicton "$deb/usr/bin/musicton"
install -Dm755 target/release/musicton-mcp "$deb/usr/bin/musicton-mcp"
install -Dm644 assets/musicton.desktop "$deb/usr/share/applications/musicton.desktop"
install -Dm644 assets/musicton.svg "$deb/usr/share/icons/hicolor/scalable/apps/musicton.svg"
install -Dm644 LICENSE "$deb/usr/share/doc/musicton/copyright"
mkdir -p "$deb/DEBIAN"
cat > "$deb/DEBIAN/control" <<EOF
Package: musicton
Version: $version
Section: sound
Priority: optional
Architecture: amd64
Depends: libasound2, libgtk-3-0, libgl1, libxkbcommon0
Maintainer: CodeKiste <kontakt@codekiste.dev>
Homepage: https://codekiste.dev
Description: Clean-room techno DAW for Linux
 Musicton is a native techno DAW with a clip launcher, timeline,
 built-in instruments and effects. Freeware, GPL-3.0-or-later.
EOF
dpkg-deb --root-owner-group --build "$deb" "dist/musicton_${version}_amd64.deb"
ls -lh dist
