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
sparkle_dir="$(bash scripts/prepare-sparkle.sh)"
mkdir -p "$bundle_dir/Contents/MacOS" "$bundle_dir/Contents/Resources"
mkdir -p "$bundle_dir/Contents/Frameworks"
ditto "$sparkle_dir/Sparkle.framework" "$bundle_dir/Contents/Frameworks/Sparkle.framework"
cp "$sparkle_dir/LICENSE" "$bundle_dir/Contents/Resources/Sparkle-LICENSE.txt"
cp "target/$build_profile/flytosnap" "$bundle_dir/Contents/MacOS/flytosnap"
cp assets/Info.plist "$bundle_dir/Contents/Info.plist"
icon_dir="$project_dir/target/flytoSnap.iconset"
mkdir -p "$icon_dir"
"target/$build_profile/flytosnap" --export-icon "$project_dir/target/app-icon.png"
cp "$project_dir/target/app-icon.png" "$bundle_dir/Contents/Resources/AppIcon.png"
cp "$project_dir/target/menu-icon.png" "$bundle_dir/Contents/Resources/MenuIcon.png"
for size in 16 32 128 256 512; do
    double_size=$((size * 2))
    sips -z "$size" "$size" "$project_dir/target/app-icon.png" --out "$icon_dir/icon_${size}x${size}.png" >/dev/null
    sips -z "$double_size" "$double_size" "$project_dir/target/app-icon.png" --out "$icon_dir/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$icon_dir" -o "$bundle_dir/Contents/Resources/AppIcon.icns"
signing_identity="${FLYTOSNAP_SIGNING_IDENTITY:-}"
if [ -z "$signing_identity" ]; then
    identities="$(security find-identity -v -p codesigning)"
    signing_identity="$(printf '%s\n' "$identities" | awk '/"Developer ID Application:/{print $2; exit}')"
    if [ -z "$signing_identity" ]; then
        signing_identity="$(printf '%s\n' "$identities" | awk '/"Apple Development:/{print $2; exit}')"
    fi
fi
if [ -z "$signing_identity" ]; then signing_identity="-"; fi
sparkle_framework="$bundle_dir/Contents/Frameworks/Sparkle.framework/Versions/B"
for component in "$sparkle_framework/XPCServices/Downloader.xpc" "$sparkle_framework/XPCServices/Installer.xpc" "$sparkle_framework/Updater.app" "$sparkle_framework/Autoupdate"; do
    codesign --force --sign "$signing_identity" --options runtime --timestamp --preserve-metadata=entitlements "$component"
done
codesign --force --sign "$signing_identity" --options runtime --timestamp "$bundle_dir/Contents/Frameworks/Sparkle.framework"
if [ "$signing_identity" = "-" ]; then
    printf 'Warning: ad-hoc signing changes the permission identity between builds. Reauthorization may be required.\n' >&2
    codesign --force --sign - --identifier com.flyto.snap "$bundle_dir"
else
    codesign --force --sign "$signing_identity" --options runtime --timestamp --identifier com.flyto.snap "$bundle_dir"
fi
codesign --verify --deep --strict "$bundle_dir"
machine_arch="$(uname -m)"
archive_path="$project_dir/dist/flytoSnap-mac-$machine_arch.zip"
ditto -c -k --sequesterRsrc --keepParent "$bundle_dir" "$archive_path"
printf 'Built %s\n' "$bundle_dir"
printf 'Archive %s\n' "$archive_path"
