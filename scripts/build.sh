#!/usr/bin/env bash
# Build the imageviewer binary inside Docker and copy it to ./target/release/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

IMAGE_TAG="imageviewer-builder:latest"

echo "==> Building Docker image..."
docker --context default build -t "$IMAGE_TAG" "$REPO_ROOT"

echo "==> Extracting binary..."
mkdir -p "$REPO_ROOT/target/release"
docker --context default create --name iv-extract "$IMAGE_TAG" 2>/dev/null
docker --context default cp iv-extract:/usr/local/bin/imageviewer "$REPO_ROOT/target/release/imageviewer"
docker --context default rm iv-extract

echo ""
echo "Binary ready at: $REPO_ROOT/target/release/imageviewer"
echo ""
echo "Run with:  ./target/release/imageviewer <image-file-or-directory>"
