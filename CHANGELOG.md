# Changelog

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
