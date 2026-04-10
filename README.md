<p align="center">
  <img src="assets/icon.png" alt="PikaViewer" width="200">
</p>

# PikaViewer

A fast, lightweight, GPU-accelerated image viewer built with Rust.

PikaViewer aims to replace the default image viewer on Linux and macOS with something snappy, keyboard-driven, and minimal. It renders images as textured quads on the GPU via wgpu, with a thin egui overlay for file info and controls.

![License](https://img.shields.io/badge/license-MIT-blue)

## Features

- **GPU-accelerated rendering** via wgpu (Vulkan on Linux, Metal on macOS)
- **Format support**: JPEG, PNG, GIF, BMP, TIFF, WebP, ICO, and optionally HEIC/HEIF/AVIF
- **Three display modes**: Regular (auto-sized), Fullscreen, Fixed (1280x720)
- **Image rotation** (90 CW/CCW) with correct aspect ratio preservation
- **EXIF auto-rotation** on load
- **Image info panel** with dimensions, aspect ratio, file size, and format
- **File deletion** with confirmation dialog
- **Keyboard-driven** with extensive shortcuts (press `H` for help)
- **Desktop integration** on Linux (`--install` / `--uninstall`, `.desktop` file, `xdg-mime`)
- **macOS native** Finder "Open With" support via ObjC runtime injection
- **Configurable** via `~/.config/pikaviewer/config.toml`

## Keyboard Shortcuts

| Key | Action |
|---|---|
| `Right` / `Down` / `Space` / `.` | Next image |
| `Left` / `Up` / `Shift+Space` / `,` | Previous image |
| `M` | Cycle display mode |
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

Requires `libheif` development libraries.

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
| `iv-core` | `FormatPlugin` trait, `PluginRegistry`, `ImageList` -- zero UI deps |
| `iv-formats` | image-rs plugin (JPEG, PNG, GIF, BMP, TIFF, WebP, ICO) |
| `iv-renderer` | wgpu renderer, display modes, WGSL shader, egui integration |
| `iv-app` | Main binary: winit event loop, egui overlays, CLI |

### Adding a new image format

1. Create `crates/iv-format-<name>/`
2. Implement `FormatPlugin` for your type
3. Add to `iv-app/Cargo.toml` as an optional dependency
4. Register in `iv-app/src/main.rs` via `registry.register(MyPlugin)`

## License

MIT

## Acknowledgements

Built with [Rust](https://www.rust-lang.org/), [wgpu](https://wgpu.rs/), [egui](https://github.com/emilk/egui), and [winit](https://github.com/rust-windowing/winit).

Developed with assistance from [Claude Code](https://claude.ai/claude-code) (Anthropic).
