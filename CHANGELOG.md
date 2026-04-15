# Changelog

## [0.3.0] - 2026-04-15

### Features

- Zoom and pan — `+`/`-` keys for multiplicative zoom, arrow keys pan when zoomed in, scale percentage shown in overlay
- Premultiplied alpha blending — correct PNG transparency rendering with sRGB linearize/premultiply pipeline
- EXIF data panel — camera, lens, exposure, ISO, date taken; shown in image info panel (`I` key)
- EXIF auto-rotation — reads orientation tag and applies rotation on load
- Settings saved on exit instead of every resize event
- EventLoopProxy wakeup — worker threads wake main loop directly, replacing polling

### Refactors

- Extracted `Viewport` struct from `Renderer` for zoom/pan state management
- Unified coordinate spaces into a single affine transform
- Introduced `FrameSnapshot` to eliminate `draw_ui` parameter avalanche (12 params → 3)
- Moved premultiply alpha to worker thread with `LazyLock` LUT
- `Arc<DecodedImage>` for zero-copy cache sharing; cache entries kept on access

### Fixes

- Fixed PNG transparency rendering (premultiplied alpha blending)
- Resolved macOS clippy errors (pointer casts, `unnecessary_wraps`)
- Resolved pedantic clippy warnings across all crates

### Other

- CI now runs on all branches (not just main/dev)
- Removed `Cargo.lock` from version control

## [0.2.0] - 2026-04-13

### Features

- Async prefetch cache — background image decoding with LRU cache (5 entries), N±1 prefetch for instant navigation
- Deferred startup — window appears immediately, directory scan runs on background thread; app stays responsive on slow/remote drives
- Loading indicator shown during background decode
- Filename and index update immediately on navigation, before image finishes loading
- "No images found" message when opening an empty folder
- "Folder not found" message when path does not exist

### Fixes

- Fixed stuck "Loading…" when navigating back before prefetch completes (generation bump now clears in-flight set)

### Other

- Debug builds log to `/tmp/pikaviewer.log` at debug level (wgpu/naga silenced)
- `AppStatus` enum replaces separate loading/empty_folder booleans

## [0.1.0] - 2026-04-10

Initial release.

### Features

- GPU-accelerated image rendering via wgpu (Vulkan on Linux, Metal on macOS)
- Supported formats: JPEG, PNG, GIF, BMP, TIFF, WebP, ICO
- Optional HEIC/HEIF/AVIF support via libheif (compile with `--features iv-app/heic`)
- Three display modes: Regular (auto-sized), Fullscreen, Fixed (1280x720)
- Image rotation 90 CW/CCW with correct letterboxing
- EXIF auto-rotation on load
- Image info panel (dimensions, aspect ratio, file size, format)
- File deletion with confirmation dialog
- Keyboard shortcuts help overlay
- Settings window with persistent config (`~/.config/pikaviewer/config.toml`)
- Desktop integration on Linux (`--install` / `--uninstall`, `.desktop` file, `xdg-mime`)
- macOS native Finder "Open With" support via ObjC runtime injection
- macOS `.app` bundle with ad-hoc signing, optional `.dmg`
- Linux `.deb` and `.AppImage` packaging via Docker
- GitHub Actions CI and release workflows
