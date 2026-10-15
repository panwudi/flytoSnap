#!/bin/bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
binary="${1:-dist/flytoSnap.app/Contents/MacOS/flytosnap}"
for language in zh-CN zh-TW en; do
    output="target/documentation-$language"
    "$binary" --ui-snapshot "$output" --lang "$language"
    for name in screenshot recording scroll-ready scroll-controls recording-result ocr; do
        if [ -f "$output/$name.png" ]; then
            cp "$output/$name.png" "docs/images/$name-$language.png"
        fi
    done
done
