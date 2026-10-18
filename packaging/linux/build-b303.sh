#!/usr/bin/env bash
# Builds dist/b303-<version>-linux-x86_64.tar.gz (VST3 + CLAP + per-user installer)
set -euo pipefail
version="$1"
cd "$(dirname "$0")/../.."
cargo xtask bundle b303-plugin --release
mkdir -p dist
name="b303-$version-linux-x86_64"
stage="target/stage/$name"
rm -rf "$stage" && mkdir -p "$stage"
cp -r target/bundled/B-303.vst3 target/bundled/B-303.clap LICENSE "$stage/"
cp packaging/linux/install-b303.sh "$stage/install.sh"
tar -C target/stage -czf "dist/$name.tar.gz" "$name"
ls -lh dist
