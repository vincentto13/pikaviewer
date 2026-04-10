# ── Build stage ───────────────────────────────────────────────────────────────
FROM rust:latest AS builder

# System deps for winit (X11 + Wayland) and wgpu (Vulkan)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libvulkan-dev \
    mesa-vulkan-drivers \
    libx11-dev \
    libxrandr-dev \
    libxcursor-dev \
    libxi-dev \
    libxxf86vm-dev \
    libwayland-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev \
    libudev-dev \
    libheif-dev \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .
RUN cargo build --release --features iv-app/heic

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    libvulkan1 \
    mesa-vulkan-drivers \
    libx11-6 \
    libxrandr2 \
    libxcursor1 \
    libxi6 \
    libwayland-client0 \
    libxkbcommon0 \
    libheif1 \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/pikaviewer /usr/local/bin/pikaviewer

ENTRYPOINT ["pikaviewer"]
