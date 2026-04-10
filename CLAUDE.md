# PikaViewer — Project Guide

## What this is

A cross-platform image viewer built in Rust. Target platforms: **Linux** (built here), **macOS** (built by user), Windows (future). Goal: replace the OS default image viewer with full file-association support.

## Tech stack

| Layer | Crate | Purpose |
|---|---|---|
| Window + events | `winit 0.30` | Cross-platform windowing, keyboard input |
| GPU rendering | `wgpu 22` | Vulkan/Metal/DX12 backend, image as textured quad |
| UI overlay | `egui 0.29` + `egui-wgpu 0.29` + `egui-winit 0.29` | Filename/mode overlays |
| Image decoding | `image 0.25` | JPEG, PNG, GIF, BMP, TIFF, WebP, ICO |
| App entry | `clap 4` | CLI arg parsing |
| Blocking async | `pollster` | `block_on` for wgpu adapter init |

## Workspace structure

```
crates/
  iv-core/        # FormatPlugin trait, PluginRegistry, ImageList — zero UI deps
  iv-formats/     # image-rs plugin (wraps the `image` crate)
  iv-renderer/    # wgpu renderer, display mode logic, WGSL shader, egui-wgpu
  iv-app/         # main binary: winit event loop, App struct, egui overlays
```

## Key abstractions

**`FormatPlugin` trait** (`iv-core/src/format.rs`): extension point for new image formats.
- Implement `descriptor()` → extensions list
- Implement `decode(&[u8]) → Result<DecodedImage, FormatError>`
- Register with `PluginRegistry::register()`
- Future RAW (NEF) and HEIC support = new crates implementing this trait

**`ImageList`** (`iv-core/src/image_list.rs`): scans a directory, sorts alphabetically, wraps on advance.

**`DisplayMode`** (`iv-renderer/src/display_mode.rs`):
- `Regular` — window resizes to image native size; capped at screen; never smaller than 600×450; position clamped to screen; image letterboxed into actual window (not requested size)
- `Fullscreen` — OS borderless fullscreen, image letterboxed
- `FixedSize { w, h }` — window fixed at 1280×720, image letterboxed
- Toggle with `M` key; mode name shown in overlay for 5 s then fades

**`Renderer`** (`iv-renderer/src/renderer.rs`): owns wgpu device/queue/surface + egui-wgpu renderer.
- `set_image()` — uploads RGBA8 texture, rebuilds vertex buffer
- `rotate(clockwise)` — remaps UV coords in vertex buffer (no shader change); swaps dims for letterbox on 90°/270°
- `render(paint_jobs, tex_delta, screen_desc)` — draws image quad then egui overlay
- `resize(w, h)` — reconfigures surface and refreshes vertex buffer

**`App`** (`iv-app/src/app.rs`): implements `winit::application::ApplicationHandler`.
Owns `egui::Context` + `egui_winit::State`. Pre-extracts overlay data before `egui_ctx.run()` to avoid borrow conflicts.
- `clamped_size(window, w, h)` — converts `MIN_WINDOW_W/H` to physical px via scale factor and applies `.max()`; used before every `request_inner_size` call
- `render_frame` checks `viewport_output.repaint_delay == Duration::ZERO` to schedule immediate winit redraws when egui requests them

## Keyboard shortcuts

| Key | Action |
|---|---|
| `→` / `↓` / `Space` / `.` | Next image |
| `←` / `↑` / `Shift+Space` / `,` | Previous image |
| `M` | Cycle display mode (Regular → Fullscreen → Fixed 1280×720) |
| `R` / `]` | Rotate 90° clockwise (resets on next image) |
| `L` / `[` | Rotate 90° counter-clockwise (resets on next image) |
| `I` | Toggle image info panel |
| `D` | Delete current image (with confirmation) |
| `H` | Show keyboard shortcuts help |
| `Cmd/Ctrl + W` | Close window |
| `Q` / `Escape` | Quit |

## Overlay UI

- **Top-left**: `filename.ext  [pos/total]` — always visible, 16 pt bold
  - Width capped at 80% of window; filename truncated with `…` before extension
  - Uses `ctx.screen_rect().width()` for width (DPI-correct); `ui.style_mut().wrap_mode` prevents egui from wrapping
- **Top-right**: mode name — 18 pt bold, fades out 5 s after mode change (1 s fade)
- **Bottom-right**: image info panel (toggled with `I`) — filename, dimensions, ratio, file size, format; `movable(false)` so anchor is enforced every frame
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
# The script auto-detects libheif via pkg-config and enables --features iv-app/heic
```

The script builds arm64 only. Intel Macs run it via Rosetta 2 (all supported Intel Macs have it).
x86_64 universal binaries were dropped because Homebrew's libheif is arm64-only, making
cross-compilation impractical for C library dependencies.

The script:
1. Auto-detects `libheif >= 1.18` via `pkg-config` and enables HEIC/AVIF support if found
2. Builds `aarch64-apple-darwin` release binary
3. Creates `PikaViewer.app` bundle with `Info.plist` (file associations for all supported formats)
4. Ad-hoc signs with `codesign --force --deep -s -` (no Apple Developer account needed)
5. Optionally wraps in a `.dmg` with `/Applications` symlink for drag-install UX

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

## Repository

`ssh://git@gitlab.astrolabius.xyz:2424/vinci/pikaviewer/pikaviewer.git` — branch `dev`

## Current status

- [x] Workspace scaffold (iv-core, iv-formats, iv-renderer, iv-app)
- [x] FormatPlugin trait + PluginRegistry
- [x] ImageList — alphabetical directory scan, wrap-around navigation
- [x] image-rs plugin (JPEG, PNG, GIF, BMP, TIFF, WebP, ICO)
- [x] wgpu renderer — letterbox, aspect ratio always preserved
- [x] Three display modes (Regular / Fullscreen / FixedSize)
- [x] Image rotation 90° CW/CCW via UV remapping
- [x] egui overlay — filename (truncated, single-line, 80% max) + mode indicator with fade
- [x] Window title bar with filename and index
- [x] Regular mode: window never bigger than screen, never goes off-screen
- [x] Docker build (`rust:latest`, `--context default`)
- [x] macOS packaging script — universal binary, `Info.plist` file associations, ad-hoc signing, optional DMG
- [x] macOS Finder "Open With" — ObjC runtime injection of `application:openURLs:` into winit's delegate
- [x] Runtime file loading via `load_path()` — opens file + initializes directory listing
- [x] Image info panel (`I` key) — dimensions, ratio, file size, format; egui::Window bottom-right
- [x] Minimum window size 600×450 (logical) enforced on creation and every resize
- [x] Regular mode aspect ratio preserved when window is larger than image (min-size letterbox)
- [x] Pushed to GitLab dev branch

## Planned next phases

1. **Async prefetch cache** — rayon thread pool, preload N±1 images
2. **Config file** — TOML at `~/.config/pikaviewer/config.toml`
3. **RAW formats** — `iv-format-raw` crate using LibRaw (LGPL, optional feature)
4. **HEIC/AVIF** — `iv-format-heic` crate using libheif (LGPL, optional feature)
5. **File associations** — `.desktop` + `xdg-mime` (Linux); macOS done via `Info.plist` in package script
6. **Zoom/pan** — Viewport struct; shader Transform uniform already designed for it
7. **EXIF auto-rotation** — read from `ImageMetadata`, apply on load

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
