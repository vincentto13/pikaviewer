<p align="center">
  <img src="assets/icon.png" alt="PikaViewer" width="200">
</p>

# PikaViewer

A fast, lightweight, GPU-accelerated image viewer built with Rust.

PikaViewer aims to replace the default image viewer on Linux and macOS with something snappy, keyboard-driven, and minimal. It renders images as textured quads on the GPU via wgpu, with a thin egui overlay for file info and controls.

![License](https://img.shields.io/badge/license-MIT-blue)

## Features

- **GPU-accelerated rendering** via wgpu (Vulkan on Linux, Metal on macOS)
- **Format support**: JPEG, PNG, GIF, BMP, TIFF, WebP, ICO, QOI, and optionally HEIC/HEIF/AVIF
- **Two display modes**: Window (fit-to-image or fixed size) and Fullscreen
- **Zoom and pan** with keyboard shortcuts (`+`/`-`, arrow keys when zoomed)
- **Image rotation** (90 CW/CCW) with correct aspect ratio preservation
- **EXIF auto-rotation** on load
- **EXIF info panel** with camera, lens, exposure, aperture, ISO, focal length, date taken
- **Async prefetch cache** — background decode of adjacent images for instant navigation
- **Premultiplied alpha** compositing in linear space for correct sRGB blending
- **File deletion** with confirmation dialog
- **Keyboard-driven** with extensive shortcuts (press `H` for help)
- **Desktop integration** on Linux (`--install` / `--uninstall`, `.desktop` file, `xdg-mime`)
- **macOS native** Finder "Open With" support via ObjC runtime injection
- **Configurable** via `~/.config/pikaviewer/config.toml` and Settings window (`Ctrl/Cmd + ,`)

## Keyboard Shortcuts

| Key | Action |
|---|---|
| `Right` / `Down` | Next image (pan right/down when zoomed) |
| `Left` / `Up` | Previous image (pan left/up when zoomed) |
| `Space` | Next image / reset zoom when zoomed |
| `Shift+Space` | Previous image / reset zoom when zoomed |
| `.` | Next image |
| `,` | Previous image |
| `+` / `=` | Zoom in |
| `-` | Zoom out / reset |
| `M` | Cycle display mode (Window / Fullscreen) |
| `R` / `]` | Rotate 90 clockwise |
| `L` / `[` | Rotate 90 counter-clockwise |
| `I` | Toggle image info panel |
| `D` | Delete current image (with confirmation) |
| `H` | Show keyboard shortcuts help |
| `Ctrl/Cmd + ,` | Open settings |
| `Ctrl/Cmd + W` | Close window |
| `Q` / `Escape` | Quit |

## Installation

### Pre-built packages (Linux x86_64)

Download the latest `.deb` or `.AppImage` from the [Releases](../../releases) page.

**Debian/Ubuntu:**
```bash
sudo dpkg -i pikaviewer_VERSION_amd64.deb
```

**AppImage (any distro):**
```bash
chmod +x PikaViewer-VERSION-x86_64.AppImage
./PikaViewer-VERSION-x86_64.AppImage
```

### macOS (arm64)

Download `PikaViewer-VERSION.dmg` from [Releases](../../releases), open it, and drag PikaViewer to Applications.

On first launch, Gatekeeper will warn about an unsigned app. Right-click the app and select Open, or run:
```bash
xattr -cr /Applications/PikaViewer.app
```

### Set as default image viewer

**Linux:**
```bash
pikaviewer --install    # installs .desktop file and icon
# then use Settings > "Set as Default Image Viewer", or:
xdg-mime default pikaviewer.desktop image/jpeg image/png image/gif
```

**macOS:**
Finder > right-click any image > Get Info > Open with > PikaViewer > Change All.

## Building from Source

### Requirements

- Rust toolchain (stable)
- Linux: Vulkan SDK, X11/Wayland dev libs, libudev
- macOS: Xcode Command Line Tools (`xcode-select --install`)

### Quick build

```bash
cargo build --release
./target/release/pikaviewer photo.jpg
```

### With HEIC/AVIF support

Requires `libheif` (LGPL-3.0) development libraries. This is an optional feature — not compiled by default in source builds. Pre-built packages include it.

```bash
# Linux (Debian/Ubuntu)
sudo apt install libheif-dev

# macOS
brew install libheif

cargo build --release --features iv-app/heic
```

### Docker build (Linux, no Rust needed on host)

```bash
./scripts/build.sh
./target/release/pikaviewer photo.jpg
```

### Packaging

**Linux (.deb + .AppImage via Docker):**
```bash
./scripts/package-linux.sh
# Output: dist/linux/pikaviewer_0.1.0_amd64.deb
#         dist/linux/PikaViewer-0.1.0-x86_64.AppImage
```

**macOS (.app bundle + optional .dmg):**
```bash
./scripts/package-macos.sh           # .app bundle
./scripts/package-macos.sh --dmg     # .dmg with drag-install
```

The macOS script auto-detects `libheif >= 1.18` via `pkg-config` and enables HEIC/AVIF support if found. Homebrew dylibs are bundled into `Frameworks/` for a self-contained `.app`.

## Usage

```bash
pikaviewer photo.jpg        # open a specific file
pikaviewer ~/Pictures       # open a directory
pikaviewer                  # open current directory
```

## Architecture

PikaViewer is organized as a Cargo workspace:

| Crate | Purpose |
|---|---|
| `iv-core` | `FormatPlugin` trait, `PluginRegistry`, `ImageList`, premultiplied alpha -- zero UI deps |
| `iv-formats` | image-rs plugin (JPEG, PNG, GIF, BMP, TIFF, WebP, ICO, QOI) |
| `iv-format-heic` | libheif plugin (HEIC, HEIF, AVIF) -- optional, not in workspace members |
| `iv-renderer` | wgpu renderer, viewport (zoom/pan), display modes, WGSL shader, egui integration |
| `iv-app` | Main binary: winit event loop, egui overlays, prefetch cache, settings, CLI |

### Adding a new image format

1. Create `crates/iv-format-<name>/`
2. Implement `FormatPlugin` for your type
3. Add to `iv-app/Cargo.toml` as an optional dependency
4. Register in `iv-app/src/main.rs` via `registry.register(MyPlugin)`

## License

MIT

Pre-built binaries include HEIC support via libheif (LGPL-3.0), dynamically linked.

## Acknowledgements

Built with [Rust](https://www.rust-lang.org/), [wgpu](https://wgpu.rs/), [egui](https://github.com/emilk/egui), and [winit](https://github.com/rust-windowing/winit).

Developed with assistance from [Claude Code](https://claude.ai/claude-code) (Anthropic).
