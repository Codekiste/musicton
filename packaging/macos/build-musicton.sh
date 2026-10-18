#!/usr/bin/env bash
# Builds dist/Musicton-<version>-macos-universal.dmg (Apple Silicon + Intel)
set -euo pipefail
version="$1"
cd "$(dirname "$0")/../.."
for t in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release -p musicton --target "$t"
done
mkdir -p dist

app="target/stage/Musicton.app"
rm -rf target/stage && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for b in musicton musicton-mcp; do
    lipo -create -output "$app/Contents/MacOS/$b" \
        "target/aarch64-apple-darwin/release/$b" "target/x86_64-apple-darwin/release/$b"
done
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# App icon from the SVG
brew list librsvg >/dev/null 2>&1 || brew install librsvg
iconset="target/stage/Musicton.iconset"
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
    rsvg-convert -w $s -h $s assets/musicton.svg -o "$iconset/icon_${s}x${s}.png"
    rsvg-convert -w $((s*2)) -h $((s*2)) assets/musicton.svg -o "$iconset/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Musicton.icns"

# Ad-hoc signature so Apple Silicon runs it; see RELEASING.md for Developer ID signing
codesign --force --deep --sign - "$app"

dmgroot="target/stage/dmg"
mkdir -p "$dmgroot"
cp -R "$app" "$dmgroot/"
ln -s /Applications "$dmgroot/Applications"
hdiutil create -volname "Musicton $version" -srcfolder "$dmgroot" -ov -format UDZO \
    "dist/Musicton-$version-macos-universal.dmg"
ls -lh dist
