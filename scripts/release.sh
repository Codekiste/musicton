#!/usr/bin/env bash
# Cuts a release: bumps the version, dates the changelog entry, commits and tags.
#   scripts/release.sh musicton 0.2.0
#   scripts/release.sh b303 0.1.1
# Then `git push && git push --tags` and the Release workflow builds the installers.
set -euo pipefail
cd "$(dirname "$0")/.."
product="${1:?usage: release.sh musicton|b303 X.Y.Z}"
version="${2:?usage: release.sh musicton|b303 X.Y.Z}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "not a SemVer version: $version"; exit 1; }

case "$product" in
    musicton) manifest=musicton/Cargo.toml;    changelog=CHANGELOG.md;             tag="musicton-v$version" ;;
    b303)     manifest=b303-plugin/Cargo.toml; changelog=b303-plugin/CHANGELOG.md; tag="b303-v$version" ;;
    *) echo "unknown product: $product (musicton or b303)"; exit 1 ;;
esac

[[ -z "$(git status --porcelain)" ]] || { echo "working tree is not clean"; exit 1; }
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && { echo "tag $tag already exists"; exit 1; }
grep -q '^## \[Unreleased\]' "$changelog" || { echo "$changelog has no '## [Unreleased]' section"; exit 1; }

sed -i.bak -E "s/^version = \"[^\"]+\"/version = \"$version\"/" "$manifest" && rm "$manifest.bak"
sed -i.bak "s/^## \[Unreleased\]/## [Unreleased]\n\n## [$version] - $(date +%Y-%m-%d)/" "$changelog" && rm "$changelog.bak"
cargo update -w --offline >/dev/null 2>&1 || cargo update -w >/dev/null

git add "$manifest" "$changelog" Cargo.lock
git commit -m "Release $tag"
git tag -a "$tag" -m "Release $tag"
echo "Tagged $tag. Push with: git push && git push origin $tag"
