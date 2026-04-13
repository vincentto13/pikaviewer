# Changelog

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
