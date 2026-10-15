#!/bin/bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
version="2.10.0"
checksum="c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c"
vendor_dir="$project_dir/target/vendor/Sparkle-$version"
archive="$project_dir/target/vendor/Sparkle-$version.tar.xz"
if [ ! -f "$vendor_dir/.verified-$checksum" ]; then
    mkdir -p "$vendor_dir"
    curl --fail --location --proto '=https' --retry 3 --connect-timeout 20 --max-time 180 \
        "https://github.com/sparkle-project/Sparkle/releases/download/$version/Sparkle-$version.tar.xz" -o "$archive"
    actual="$(shasum -a 256 "$archive" | awk '{print $1}')"
    if [ "$actual" != "$checksum" ]; then
        printf 'Sparkle archive checksum mismatch\n' >&2
        exit 1
    fi
    tar -xJf "$archive" -C "$vendor_dir"
    touch "$vendor_dir/.verified-$checksum"
fi
printf '%s\n' "$vendor_dir"
