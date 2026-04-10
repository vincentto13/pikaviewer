#!/usr/bin/env bash
# make-icns.sh — Convert assets/icon.png (1024×1024) to assets/icon.icns
# Requires macOS (uses sips + iconutil).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SRC="$REPO_ROOT/assets/icon.png"
OUT="$REPO_ROOT/assets/icon.icns"

if [[ "$(uname)" != "Darwin" ]]; then
    echo "error: this script requires macOS (sips + iconutil)"
    exit 1
fi

if [[ ! -f "$SRC" ]]; then
    echo "error: $SRC not found"
    exit 1
fi

ICONSET=$(mktemp -d)/icon.iconset
mkdir -p "$ICONSET"

# Helper: resize preserving alpha channel
resize() {
    local size=$1 out=$2
    sips --resampleHeightWidth "$size" "$size" "$SRC" --out "$out" >/dev/null 2>&1
}

# Standard sizes + retina variants required by iconutil
resize 16   "$ICONSET/icon_16x16.png"
resize 32   "$ICONSET/icon_16x16@2x.png"
resize 32   "$ICONSET/icon_32x32.png"
resize 64   "$ICONSET/icon_32x32@2x.png"
resize 128  "$ICONSET/icon_128x128.png"
resize 256  "$ICONSET/icon_128x128@2x.png"
resize 256  "$ICONSET/icon_256x256.png"
resize 512  "$ICONSET/icon_256x256@2x.png"
resize 512  "$ICONSET/icon_512x512.png"
resize 1024 "$ICONSET/icon_512x512@2x.png"

iconutil -c icns "$ICONSET" -o "$OUT"
rm -rf "$(dirname "$ICONSET")"

echo "Created $OUT"
