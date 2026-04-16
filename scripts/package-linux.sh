#!/usr/bin/env bash
# package-linux.sh — Build PikaViewer .deb and .AppImage via Docker
#
# Usage:
#   ./scripts/package-linux.sh                  # x86_64, version 0.1.0
#   ./scripts/package-linux.sh --version 0.2.0  # custom version
#
# Output: dist/linux/pikaviewer_VERSION_amd64.deb
#         dist/linux/PikaViewer-VERSION-x86_64.AppImage
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

VERSION="0.3.1"

while [[ $# -gt 0 ]]; do
    case $1 in
        --version) VERSION="$2"; shift 2 ;;
        *) echo "error: unknown option: $1"; exit 1 ;;
    esac
done

OUT_DIR="$REPO_ROOT/dist/linux"
mkdir -p "$OUT_DIR"

IMAGE_TAG="pikaviewer-package:latest"

echo "==> Building packages (version $VERSION)..."
docker --context default build \
    --network host \
    -f "$REPO_ROOT/Dockerfile.package" \
    --build-arg VERSION="$VERSION" \
    -t "$IMAGE_TAG" \
    "$REPO_ROOT"

echo "==> Extracting packages..."
docker --context default create --name pv-pkg "$IMAGE_TAG" 2>/dev/null
docker --context default cp pv-pkg:/out/. "$OUT_DIR/"
docker --context default rm pv-pkg

echo ""
echo "Done. Packages in $OUT_DIR:"
ls -lh "$OUT_DIR"/*.deb "$OUT_DIR"/*.AppImage 2>/dev/null
echo ""
echo "Install .deb:      sudo dpkg -i $OUT_DIR/pikaviewer_${VERSION}_amd64.deb"
echo "Run AppImage:      chmod +x $OUT_DIR/PikaViewer-${VERSION}-x86_64.AppImage && $OUT_DIR/PikaViewer-${VERSION}-x86_64.AppImage"
