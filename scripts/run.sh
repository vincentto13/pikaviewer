#!/usr/bin/env bash
# Run the pikaviewer Docker container with X11 forwarding.
# Usage: ./scripts/run.sh [image-or-directory]
set -euo pipefail

IMAGE_TAG="pikaviewer-builder:latest"
IMAGES_PATH="${1:-}"

if [[ -z "$IMAGES_PATH" ]]; then
    echo "Usage: $0 <image-file-or-directory>"
    exit 1
fi

IMAGES_ABS="$(realpath "$IMAGES_PATH")"

# Allow Docker to connect to the host X11 server.
xhost +local:docker 2>/dev/null || true

# Determine whether it's a file or directory and set the container path.
if [[ -f "$IMAGES_ABS" ]]; then
    CONTAINER_MOUNT="/images/$(basename "$IMAGES_ABS")"
    VOLUME_SRC="$(dirname "$IMAGES_ABS")"
    VOLUME_DST="/images"
else
    CONTAINER_MOUNT="/images"
    VOLUME_SRC="$IMAGES_ABS"
    VOLUME_DST="/images"
fi

docker --context default run --rm \
    -e DISPLAY="${DISPLAY:-:0}" \
    -e WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" \
    -e XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}" \
    -v /tmp/.X11-unix:/tmp/.X11-unix \
    -v "${XDG_RUNTIME_DIR:-/tmp}:${XDG_RUNTIME_DIR:-/tmp}" \
    -v "$VOLUME_SRC:$VOLUME_DST:ro" \
    -e VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
    "$IMAGE_TAG" "$CONTAINER_MOUNT"
