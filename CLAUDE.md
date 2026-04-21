# PikaViewer — Project Guide

## What this is

A cross-platform image viewer built in Rust. Target platforms: **Linux** (built here), **macOS** (built by user), Windows (future). Goal: replace the OS default image viewer with full file-association support.

## Tech stack

| Layer | Crate | Purpose |
|---|---|---|
| Window + events | `winit 0.30` | Cross-platform windowing, keyboard input |
| GPU rendering | `wgpu 22` | Vulkan/Metal/DX12 backend, image as textured quad |
| UI overlay | `egui 0.29` + `egui-wgpu 0.29` + `egui-winit 0.29` | Filename/mode overlays, settings window |
| Image decoding | `image 0.25` | JPEG, PNG, GIF, BMP, TIFF, WebP, ICO, QOI |
| HEIC/AVIF | `libheif-rs 1` (optional) | HEIC, HEIF, AVIF via system libheif |
| RAW | `rsraw 0.1` (optional) | NEF, CR2/CR3, ARW, RAF, ORF, RW2, PEF, DNG via vendored LibRaw |
| EXIF | `kamadak-exif 0.5` | Orientation auto-rotation, camera/lens/exposure metadata |
| Config | `serde` + `toml` + `dirs` | TOML settings at OS-appropriate config path |
| App entry | `clap 4` | CLI arg parsing |
| Blocking async | `pollster` | `block_on` for wgpu adapter init |

## Workspace structure

```
crates/
  iv-core/          # FormatPlugin trait, PluginRegistry, ImageList, premultiplied alpha — zero UI deps
  iv-formats/       # image-rs plugin (wraps the `image` crate)
  iv-format-heic/   # libheif plugin — NOT a workspace member (only compiled with --features iv-app/heic)
  iv-format-raw/    # LibRaw plugin — NOT a workspace member (only compiled with --features iv-app/raw)
  iv-renderer/      # wgpu renderer, Viewport (zoom/pan), display modes, WGSL shader, egui-wgpu
  iv-app/           # main binary: winit event loop, App struct, egui overlays, prefetch cache, settings
```

## Key abstractions

**`FormatPlugin` trait** (`iv-core/src/format.rs`): extension point for new image formats.
- Implement `descriptor()` → extensions list
- Implement `decode(&[u8]) → Result<DecodedImage, FormatError>`
- `DecodedImage` includes `has_alpha: bool` for premultiplied alpha optimization
- Register with `PluginRegistry::register()`

**`ImageList`** (`iv-core/src/image_list.rs`): scans a directory, sorts alphabetically, wraps on advance.
- `peek_offset(delta)` — peek without moving cursor (used by prefetch)
- `remove_current()` — for file deletion

**`DisplayMode`** (`iv-renderer/src/display_mode.rs`):
- `Window` — behavior depends on `fit_to_image` setting:
  - `fit_to_image=true`: window resizes to image (capped at screen)
  - `fit_to_image=false`: window stays at fixed user-chosen size, image letterboxed
- `Fullscreen` — OS borderless fullscreen, image letterboxed
- Toggle with `M` key; mode name shown in overlay for 5 s then fades

**`Viewport`** (`iv-renderer/src/renderer.rs`): zoom/pan state for the image quad.
- Zoom via `+`/`-` keys (multiplicative steps, clamped to `[min_zoom, ZOOM_MAX]`)
- Pan via arrow keys when zoomed in
- Transform uniform uploaded to GPU via bind group

**`Renderer`** (`iv-renderer/src/renderer.rs`): owns wgpu device/queue/surface + egui-wgpu renderer.
- `set_image()` — uploads RGBA8 texture, rebuilds vertex buffer
- `rotate(clockwise)` — remaps UV coords in vertex buffer (no shader change); swaps dims for letterbox on 90°/270°
- `render(paint_jobs, tex_delta, screen_desc)` — draws image quad then egui overlay
- `resize(w, h)` — reconfigures surface and refreshes vertex buffer
- `GpuContext` — shared `Instance`/`Adapter`/`Device`/`Queue` (Arc'd for multi-window)

**`PrefetchCache`** (`iv-app/src/prefetch.rs`): async background image decoding.
- Worker thread pool decodes N+1/N-1 images ahead
- LRU eviction (5 entries)
- Uses `EventLoopProxy` to wake main thread on completion (no polling)
- Extracts EXIF data during decode (orientation, camera metadata)
- `Arc<DecodedImage>` for zero-copy cache sharing

**`App`** (`iv-app/src/app.rs`): implements `winit::application::ApplicationHandler`.
Owns `egui::Context` + `egui_winit::State`. Pre-extracts overlay data before `egui_ctx.run()` to avoid borrow conflicts.
- `clamped_size(window, w, h)` — converts `MIN_WINDOW_W/H` to physical px via scale factor
- `render_frame` checks `viewport_output.repaint_delay == Duration::ZERO` to schedule immediate winit redraws
- Registry built in `main::build_registry()` with feature-gated plugins
- Settings saved on exit (not on every resize)

**`SettingsWindow`** (`iv-app/src/settings_window.rs`): separate winit window with its own wgpu surface.
- Shares `Arc<GpuContext>` with main window
- egui checkbox for `fit_to_image` setting
- `Ctrl/Cmd + ,` shortcut to open

## Keyboard shortcuts

| Key | Action |
|---|---|
| `→` / `↓` | Next image (pan right/down when zoomed) |
| `←` / `↑` | Previous image (pan left/up when zoomed) |
| `Space` | Next image / reset zoom when zoomed |
| `Shift+Space` | Previous image / reset zoom when zoomed |
| `.` | Next image |
| `,` | Previous image |
| `+` / `=` | Zoom in |
| `-` | Zoom out / reset |
| `M` | Cycle display mode (Window / Fullscreen) |
| `R` / `]` | Rotate 90° clockwise (resets on next image) |
| `L` / `[` | Rotate 90° counter-clockwise (resets on next image) |
| `I` | Toggle image info panel |
| `D` | Delete current image (with confirmation) |
| `H` | Show keyboard shortcuts help |
| `Ctrl/Cmd + ,` | Open settings |
| `Cmd/Ctrl + W` | Close window |
| `Q` / `Escape` | Quit |

## Overlay UI

- **Top-left**: `filename.ext  [pos/total]` — always visible, 16 pt bold
  - Width capped at 80% of window; filename truncated with `…` before extension
  - Uses `ctx.screen_rect().width()` for width (DPI-correct); `ui.style_mut().wrap_mode` prevents egui from wrapping
- **Top-right**: mode name — 18 pt bold, fades out 5 s after mode change (1 s fade)
- **Bottom-right**: image info panel (toggled with `I`) — file info + EXIF data (camera, lens, exposure, date); `movable(false)` so anchor is enforced every frame
- **Center**: loading indicator while prefetch/decode is in progress
- **Window title**: `PikaViewer - filename.ext [pos/total]`
- Minimum window size: 600×450 logical px — set via `LogicalSize` in `WindowAttributes` AND enforced in `clamped_size()` before every `request_inner_size` call

## Building

### Docker (no Rust needed on host)

**Always use `--context default`** — the active Docker context points to a swarm node; the local engine is `default`.

```bash
./scripts/build.sh          # builds image + extracts binary to target/release/pikaviewer
```

Manual:
```bash
docker --context default build -t pikaviewer-builder:latest .
docker --context default create --name iv-extract pikaviewer-builder:latest
docker --context default cp iv-extract:/usr/local/bin/pikaviewer target/release/pikaviewer
docker --context default rm iv-extract
```

The Dockerfile builds with `--features iv-app/heic,iv-app/raw` (HEIC + RAW enabled in Docker builds).

### Running

```bash
./target/release/pikaviewer photo.jpg       # open specific file
./target/release/pikaviewer ~/Pictures      # open directory
./target/release/pikaviewer                 # open current directory
```

Software Vulkan (no GPU, or inside Docker):
```bash
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json ./target/release/pikaviewer photo.jpg
```

### macOS (native build, no Docker)

Cross-compilation from Linux is not supported (requires Apple SDK + Metal framework linking).
Must build natively on macOS. No Docker needed or useful here.

**Prerequisites:** Xcode CLT only (`xcode-select --install`). wgpu selects Metal automatically.

```bash
# Quick native build (current arch only)
cargo build --release

# Full package: arm64 .app bundle + optional DMG
./scripts/package-macos.sh           # → dist/macos/PikaViewer.app
./scripts/package-macos.sh --dmg     # → dist/macos/PikaViewer-0.1.0.dmg

# With HEIC support (requires: brew install libheif)
# The script auto-detects libheif >= 1.18 via pkg-config and enables --features iv-app/heic
```

The script builds arm64 only. Intel Macs run it via Rosetta 2 (all supported Intel Macs have it).
x86_64 universal binaries were dropped because Homebrew's libheif is arm64-only, making
cross-compilation impractical for C library dependencies.

The script:
1. Auto-detects `libheif >= 1.18` via `pkg-config` and enables HEIC/AVIF support if found
2. Builds `aarch64-apple-darwin` release binary
3. Creates `PikaViewer.app` bundle with `Info.plist` (file associations for all supported formats)
4. Bundles Homebrew dylibs into `Frameworks/` with `@rpath` rewriting for self-contained `.app`
5. Ad-hoc signs with `codesign --force --deep -s -` (no Apple Developer account needed)
6. Optionally wraps in a `.dmg` with `/Applications` symlink for drag-install UX

**First launch:** Gatekeeper will warn about unsigned app. Right-click → Open to bypass once, or:
```bash
xattr -cr dist/macos/PikaViewer.app
```

**Setting as default viewer** (requires `brew install duti`):
```bash
duti -s xyz.astrolabius.pikaviewer public.jpeg all
duti -s xyz.astrolabius.pikaviewer public.png all
# ... (full list printed by package-macos.sh on completion)
```
Or: Finder → Get Info on any image → Open with → Change All.

## HEIC/AVIF support

`iv-format-heic` is an **optional crate** that wraps libheif-rs (LGPL-3.0). It is intentionally
**not** a workspace member — this way `cargo build` without `--features iv-app/heic` never
requires libheif to be installed. The feature flag in `iv-app/Cargo.toml`:

```toml
[features]
heic = ["dep:iv-format-heic", "iv-format-heic/libheif-rs"]
```

The crate has a stub fallback: when compiled without the `libheif-rs` sub-feature, `decode()`
returns an error message telling the user to rebuild with `--features iv-app/heic`.

Pre-built packages (Docker, macOS DMG) enable the feature by default.

## RAW support

`iv-format-raw` is an **optional crate** that wraps `rsraw` (MIT wrapper) over LibRaw
(LGPL-2.1/CDDL-1.0). LibRaw C++ sources are **vendored inside `rsraw-sys`** and compiled by
`cc` at build time, so there is no system `libraw` to install — only a C++ toolchain and
libclang (for `bindgen`). It is intentionally **not** a workspace member so that the default
`cargo build` stays pure-MIT. The feature flag in `iv-app/Cargo.toml`:

```toml
[features]
raw = ["dep:iv-format-raw", "iv-format-raw/rsraw"]
```

The crate has a stub fallback: when compiled without the `rsraw` sub-feature, `decode()`
returns an error message telling the user to rebuild with `--features iv-app/raw`.

Supported formats: NEF/NRW (Nikon), CR2/CR3/CRW (Canon), ARW/SR2/SRF (Sony), RAF (Fuji),
ORF (Olympus), RW2 (Panasonic), PEF (Pentax), DNG (Adobe), plus generic `.raw`.

Implementation notes:
- `user_flip = 0` is set on the LibRaw params struct before `process()` so LibRaw does not
  auto-rotate. The existing EXIF pipeline in `prefetch.rs` reads orientation and the renderer
  applies rotation — keeping this uniform across all formats avoids double-rotation.
- `set_use_camera_wb(true)` is enabled so colors match the camera's in-body rendering.
- Output is 8-bit RGB from `process::<BIT_DEPTH_8>()`, expanded to RGBA8 with `has_alpha: false`.
- First-time build of the `raw` feature compiles ~50 C++ files and adds a few MB to the binary.
  Subsequent builds are cached. RAW decode itself takes ~1-2s per 24MP file; the existing
  prefetch + parallel workers absorb the latency during navigation.

Pre-built packages (Docker, macOS DMG) enable the feature by default.

## Repository

GitHub: `github.com:vincentto13/pikaviewer.git`
GitLab: `ssh://git@gitlab.astrolabius.xyz:2424/vinci/pikaviewer/pikaviewer.git`

## Current status

- [x] Workspace scaffold (iv-core, iv-formats, iv-renderer, iv-app)
- [x] FormatPlugin trait + PluginRegistry
- [x] ImageList — alphabetical directory scan, wrap-around navigation
- [x] image-rs plugin (JPEG, PNG, GIF, BMP, TIFF, WebP, ICO, QOI)
- [x] wgpu renderer — letterbox, aspect ratio always preserved
- [x] Two display modes (Window with fit_to_image toggle / Fullscreen)
- [x] Zoom/pan — Viewport struct, shader Transform uniform, keyboard-driven
- [x] Image rotation 90° CW/CCW via UV remapping
- [x] egui overlay — filename (truncated, single-line, 80% max) + mode indicator with fade
- [x] Window title bar with filename and index
- [x] Window mode: window never bigger than screen, never goes off-screen
- [x] Docker build (`rust:latest`, `--context default`)
- [x] macOS packaging script — arm64, `Info.plist` file associations, ad-hoc signing, optional DMG
- [x] macOS Finder "Open With" — ObjC runtime injection of `application:openURLs:` into winit's delegate
- [x] Runtime file loading via `load_path()` — opens file + initializes directory listing
- [x] Image info panel (`I` key) — dimensions, ratio, file size, format + EXIF data
- [x] Minimum window size 600×450 (logical) enforced on creation and every resize
- [x] Aspect ratio preserved in all modes (letterbox into actual window size)
- [x] EXIF data panel — camera, lens, exposure, date taken; shown in info panel
- [x] EXIF auto-rotation — reads orientation tag, applies rotation on load
- [x] Config file — TOML at `~/.config/pikaviewer/config.toml`
- [x] Settings window — separate winit window, shared GPU context, checkbox UI
- [x] HEIC/AVIF — `iv-format-heic` crate using libheif (optional feature, stub fallback)
- [x] RAW — `iv-format-raw` crate using vendored LibRaw via `rsraw` (optional feature, stub fallback)
- [x] File associations — `.desktop` + `xdg-mime` (Linux); macOS done via `Info.plist`
- [x] Linux packaging — `.deb` + `.AppImage` via Docker
- [x] macOS packaging — `.app` bundle + `.dmg` with icon generation, dylib bundling
- [x] GitHub CI — check/clippy/test + release workflow for v* tags
- [x] Async prefetch cache — background decode thread, LRU cache (5 entries), N±1 prefetch, loading indicator
- [x] Premultiplied alpha — sRGB linearize → premultiply → sRGB encode, with LazyLock LUT
- [x] EventLoopProxy wakeup — worker threads wake main loop directly (no polling)
- [x] File deletion — `D` key with confirmation dialog, moves to trash on supported platforms
- [x] Pedantic clippy — `#[must_use]`, safe `From` upcasts, doc backtick fixes

## macOS file-open mechanism (`macos_events.rs`)

winit 0.30 does not expose macOS `application:openURLs:` via its API. When Finder opens a file
with PikaViewer, macOS delivers the path via Apple Events → `application:openURLs:` on the
NSApplicationDelegate — which winit's delegate doesn't implement.

Solution: raw ObjC runtime injection via `class_replaceMethod` on winit's delegate class.

**Timing constraint:** `NSApplication.finishLaunching` processes Apple Events *before*
`applicationDidFinishLaunching:` (winit's `resumed()`). So injection must happen earlier.

**Two-pronged approach** (in `register()`, called between `EventLoop::new()` and `run_app()`):
1. Immediate patch — winit sets its delegate during `EventLoop::new()`, so it's usually ready
2. `NSApplicationWillFinishLaunchingNotification` observer — fires at the *top* of `finishLaunching`, before Apple Events

The injected handler stores the path in `PENDING_FILE` (a `Mutex<Option<PathBuf>>`), which
`App::about_to_wait()` polls each frame and calls `load_path()` to open the image.

**If upgrading winit:** check if the new version exposes `open_urls` natively (winit 0.31 does not as of beta2). If so, this module can be replaced. egui-winit must also be compatible.

## Known wgpu 22 / egui 0.29 API notes (for future contributors)

- `DeviceDescriptor` requires `memory_hints: wgpu::MemoryHints::default()`
- `RenderPipelineDescriptor` requires `cache: None`
- `VertexState` / `FragmentState` require `compilation_options: wgpu::PipelineCompilationOptions::default()`
- egui-wgpu `Renderer::new` takes 5 args (last = `dithering: bool` → `false`)
- Must call `.forget_lifetime()` on `RenderPass` before passing to `egui_renderer.render()`
- `egui_winit::State::new` takes 6 args: ctx, viewport_id, display_target, pixels_per_point, theme, max_texture_side
- `FullOutput` has no `repaint_after`; use `full_output.viewport_output.get(&ViewportId::ROOT).map(|vo| vo.repaint_delay)`
- `ui.style_mut().wrap = Some(false)` is deprecated → use `wrap_mode = Some(egui::TextWrapMode::Extend)`
- `egui::Window::anchor()` is only a default position when movable; add `.movable(false)` to enforce it every frame

## Adding a new image format

1. Create `crates/iv-format-<name>/`
2. Implement `FormatPlugin` for your type
3. Add to `iv-app/Cargo.toml` as optional dep
4. Register in `iv-app/src/main.rs` via `registry.register(MyPlugin)`
