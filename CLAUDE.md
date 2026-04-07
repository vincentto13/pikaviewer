# ImageViewer — Project Guide

## What this is

A cross-platform image viewer built in Rust. Target platforms: **Linux** (built here), **macOS** (built by user), Windows (future). Goal: replace the OS default image viewer with full file-association support.

## Tech stack

| Layer | Crate | Purpose |
|---|---|---|
| Window + events | `winit 0.30` | Cross-platform windowing, keyboard input |
| GPU rendering | `wgpu 0.20` | Vulkan/Metal/DX12 backend, image as textured quad |
| Image decoding | `image 0.25` | JPEG, PNG, GIF, BMP, TIFF, WebP, ICO |
| App entry | `clap 4` | CLI arg parsing |
| Blocking async | `pollster` | `block_on` for wgpu adapter init |

## Workspace structure

```
crates/
  iv-core/        # FormatPlugin trait, PluginRegistry, ImageList — zero UI deps
  iv-formats/     # image-rs plugin (wraps the `image` crate)
  iv-renderer/    # wgpu renderer, display mode logic, WGSL shader
  iv-app/         # main binary: winit event loop, App struct
```

## Key abstractions

**`FormatPlugin` trait** (`iv-core/src/format.rs`): the extension point for new image formats.
- Implement `descriptor()` → extensions list
- Implement `decode(&[u8]) → Result<DecodedImage, FormatError>`
- Register with `PluginRegistry::register()`
- Future RAW (NEF) and HEIC support = new crates implementing this trait

**`ImageList`** (`iv-core/src/image_list.rs`): scans a directory, sorts alphabetically, wraps on advance.

**`DisplayMode`** (`iv-renderer/src/display_mode.rs`):
- `Regular` — window resizes to image native size
- `Fullscreen` — OS fullscreen, image letterboxed
- `FixedSize { w, h }` — window fixed, image letterboxed
- Toggle with `M` key

**`Renderer`** (`iv-renderer/src/renderer.rs`): owns wgpu device/queue/surface. Call `set_image()` after loading, `render()` each frame, `resize()` on window resize.

**`App`** (`iv-app/src/app.rs`): implements `winit::application::ApplicationHandler`.

## Keyboard shortcuts

| Key | Action |
|---|---|
| `→` / `↓` | Next image |
| `←` / `↑` | Previous image |
| `M` | Cycle display mode (Regular → Fullscreen → Fixed 1280×720) |
| `Q` / `Escape` | Quit |

## Building

### With Docker (no Rust needed on host)

```bash
./scripts/build.sh          # produces ./target/release/imageviewer
```

### Running

```bash
./target/release/imageviewer photo.jpg       # open specific file
./target/release/imageviewer ~/Pictures      # open directory
./target/release/imageviewer                 # open current directory
```

With Docker + X11 forwarding:
```bash
./scripts/run.sh ~/Pictures/photo.jpg
```

For software Vulkan (no GPU / Docker):
```bash
LIBGL_ALWAYS_SOFTWARE=1 VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json ./target/release/imageviewer photo.jpg
```

## Current status (MVP)

- [x] Workspace scaffold with all 4 crates
- [x] FormatPlugin trait + PluginRegistry
- [x] ImageList (directory scan, alphabetical, wrap-around navigation)
- [x] image-rs format plugin (JPEG, PNG, GIF, BMP, TIFF, WebP, ICO)
- [x] wgpu renderer with letterbox layout
- [x] Three display modes
- [x] winit event loop (keyboard nav, resize, fullscreen)
- [x] Docker build environment
- [x] Build passing (Docker image: `imageviewer-builder:latest`, binary: `target/release/imageviewer`)

## Planned next phases

1. **Async prefetch cache** — rayon thread pool, preload N±1 images
2. **egui status bar** — filename, index/total, current mode
3. **Config file** — TOML at `~/.config/imageviewer/config.toml`
4. **RAW formats** — `iv-format-raw` crate using LibRaw (LGPL, optional feature)
5. **HEIC/AVIF** — `iv-format-heic` crate using libheif (LGPL, optional feature)
6. **File associations** — `.desktop` + `xdg-mime` (Linux), `Info.plist` + `.app` bundle (macOS)
7. **Zoom/pan** — Viewport struct already stubbed; shader Transform uniform ready
8. **Rotation** — EXIF auto-rotate + manual R key

## Adding a new image format

1. Create `crates/iv-format-<name>/`
2. Implement `FormatPlugin` for your type
3. Add it to `iv-app/Cargo.toml` as an optional dependency
4. Register it in `iv-app/src/main.rs` via `registry.register(MyPlugin)`

## macOS build notes

The Rust code is fully cross-platform. On macOS:
```bash
cargo build --release
```
System deps needed: none beyond Xcode command line tools (Metal backend is used automatically by wgpu).

For distribution: wrap binary in `.app` bundle with `Info.plist` declaring image UTIs, run `codesign --force --deep -s - ImageViewer.app`.
