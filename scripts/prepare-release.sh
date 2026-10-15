#!/bin/bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
bundle="dist/flytoSnap.app"
version="$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$bundle/Contents/Info.plist")"
build="$(/usr/libexec/PlistBuddy -c 'Print CFBundleVersion' "$bundle/Contents/Info.plist")"
notes="${1:-docs/releases/v$version.md}"
test -f "$notes"
codesign --verify --deep --strict "$bundle"
"$bundle/Contents/MacOS/flytosnap" --updater-check
release_dir="dist/releases/v$version"
mkdir -p "$release_dir"
archive="flytoSnap-$version-mac-$(uname -m).zip"
ditto -c -k --sequesterRsrc --keepParent "$bundle" "$release_dir/$archive"
cp "$notes" "$release_dir/${archive%.zip}.md"
sparkle_dir="$(bash scripts/prepare-sparkle.sh)"
"$sparkle_dir/bin/generate_appcast" --account com.flyto.snap --maximum-deltas 0 --embed-release-notes \
    --download-url-prefix "https://github.com/panwudi/flytoSnap/releases/download/v$version/" \
    --link "https://github.com/panwudi/flytoSnap/releases/tag/v$version" \
    -o "$release_dir/appcast.xml" "$release_dir"
"$sparkle_dir/bin/sign_update" --account com.flyto.snap --verify "$release_dir/appcast.xml"
cp "$release_dir/appcast.xml" appcast.xml
(cd "$release_dir" && shasum -a 256 "$archive" > SHA256SUMS.txt)
printf 'Release prepared: %s (build %s)\nArchive: %s/%s\n' "$version" "$build" "$release_dir" "$archive"
