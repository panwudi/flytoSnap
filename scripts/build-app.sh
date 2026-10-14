#!/bin/bash
set -euo pipefail

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
if [ "${1:-}" = "--debug" ]; then
    cargo build
    build_profile="debug"
else
    cargo build --release
    build_profile="release"
fi
bundle_dir="$project_dir/dist/flytoSnap.app"
mkdir -p "$bundle_dir/Contents/MacOS" "$bundle_dir/Contents/Resources"
cp "target/$build_profile/flytosnap" "$bundle_dir/Contents/MacOS/flytosnap"
cp assets/Info.plist "$bundle_dir/Contents/Info.plist"
codesign --force --sign - --identifier com.flyto.snap "$bundle_dir"
codesign --verify --deep --strict "$bundle_dir"
machine_arch="$(uname -m)"
archive_path="$project_dir/dist/flytoSnap-mac-$machine_arch.zip"
ditto -c -k --sequesterRsrc --keepParent "$bundle_dir" "$archive_path"
printf 'Built %s\n' "$bundle_dir"
printf 'Archive %s\n' "$archive_path"
