# Code Review — 2026-04-07

Critical review of the full codebase. Items are ordered by impact.

## Bugs and correctness issues

### 1. Synchronous I/O on the main thread
**Files**: `iv-app/src/app.rs` — `load_current()`
**Severity**: High

`fs::metadata()`, `fs::read()`, and `plugin.decode()` all run on the UI thread. Large images (10+ MB JPEG, 50+ MB TIFF) freeze the window. macOS may show "not responding" spinner.

**Fix**: Decode on a background thread, send result back via channel. Already planned as "async prefetch cache".

### 2. Only first URL processed from Finder multi-select
**File**: `iv-app/src/macos_events.rs` — `impl_open_urls()`
**Severity**: Medium

`firstObject` on the URL array ignores additional files. If user selects 5 images and opens with PikaViewer, 4 are silently dropped.

**Fix**: Iterate the NSArray, pick the first file and use its directory (consistent with current single-image-opens-directory behavior), or support opening multiple files.

### 3. `ImageList` does not handle directory changes
**File**: `iv-core/src/image_list.rs`
**Severity**: Medium

Directory is scanned once and cached. Files added/deleted after scan cause stale paths, misleading position counter, and silent errors on navigation.

**Fix**: Consider re-scanning on navigation error, or watching with `notify` crate.

### 4. `resumed()` can be called multiple times
**File**: `iv-app/src/app.rs` — `resumed()`
**Severity**: Low (desktop), High (mobile)

winit may call `resumed()` more than once. Each call overwrites window/renderer/egui_state. Old GPU resources leak, old `Arc<Window>` may dangle.

**Fix**: Guard with `if self.window.is_some() { return; }` or properly tear down old state.

### 5. `SurfaceError::Lost` not handled
**File**: `iv-renderer/src/renderer.rs` — `render()`
**Severity**: Low (rare on desktop)

Only `Outdated` is handled gracefully. `Lost` (display disconnect, device loss on Windows) propagates as a fatal error.

**Fix**: Reconfigure the surface on `Lost`, similar to `Outdated`.

### 6. `Renderer::screen_size` captured once, never updated
**File**: `iv-renderer/src/renderer.rs` — constructor
**Severity**: Low

Read from `current_monitor()` at init. Moving window to a different-resolution monitor makes the Regular-mode size cap wrong.

**Fix**: Re-query on monitor change or on each `compute_window_size()` call.

## UX issues

### 7. `R`/`L`/`I` key bindings are case-sensitive
**File**: `iv-app/src/app.rs` — key handling
**Severity**: Low

`"m" | "M"` and `"q" | "Q"` handle both cases, but `"r"`, `"l"`, `"i"` are lowercase only. Caps Lock or accidental Shift makes them stop working.

**Fix**: Add uppercase variants: `"r" | "R"`, `"l" | "L"`, `"i" | "I"`.

### 8. `simplify_ratio` produces unhelpful results for most photos
**File**: `iv-app/src/app.rs` — `simplify_ratio()`
**Severity**: Low

3024x4032 → 3:4 (good). 3024x4033 → 3024:4033 (useless). Most real photo dimensions don't simplify cleanly.

**Fix**: Use approximate common ratios (16:9, 4:3, 3:2, 1:1) with a tolerance, or show decimal (e.g. "1.33:1").

### 9. `clamp_to_screen` uses `inner_size` as proxy for outer size
**File**: `iv-app/src/app.rs` — `clamp_to_screen()`
**Severity**: Low

Title bar height (~22-28px on macOS) is not accounted for. Window title bar can end up partially off-screen.

### 10. No feedback during long image loads
**File**: `iv-app/src/app.rs` — `load_current()`
**Severity**: Low (until async loading is added)

No spinner, progress indicator, or timeout. OOM on extremely large images causes `abort()` with no user feedback.

## Resource management

### 11. GPU resources re-created on every image load
**File**: `iv-renderer/src/renderer.rs` — `set_image()`
**Severity**: Low

Texture, sampler, bind group, and vertex buffer are all re-created per image. The sampler never changes and could be created once. Under rapid key-repeat navigation, deferred GPU deallocation could spike VRAM.

### 12. Observer instance leaked in ObjC runtime
**File**: `iv-app/src/macos_events.rs` — `register()`
**Severity**: Negligible

`IVWillFinishObserver` instance is never released. One-time ~16-byte leak. Retained by NSNotificationCenter so it's not a use-after-free — just never freed.

## Dead code

### 13. `format_name` field in `ImageInfo` is unused
**File**: `iv-app/src/app.rs`

`ImageInfo.format_name` is set to `"image-rs"` (the plugin name, not format name) but never displayed. The info panel uses the file extension instead.

## Architecture notes

### What's good
- Clean workspace separation: `iv-core` (zero UI deps) → `iv-formats` (pluggable) → `iv-renderer` (GPU) → `iv-app` (glue)
- `FormatPlugin` trait is well-designed for extensibility
- UV-remapping for rotation avoids shader complexity
- `macos_events.rs` ObjC injection is well-documented; two-pronged timing is sound
- Letterbox math is correct and well-commented

### What to watch
- **`Renderer` pub fields** (`display_mode`, `image_size`, `rotation`) are directly mutated from `App`, bypassing invariant maintenance. Setting `rotation = 0` in `navigate()` doesn't call `refresh_vertex_buf()` — works only because `set_image()` rebuilds the buffer afterward. This coupling is implicit.
- **`App` is a flat struct** mixing windowing, UI state, image management, and overlay data. Will become unwieldy as features grow (zoom, EXIF, config). Consider extracting `ImageManager` or `ViewerState`.
- **`compute_layout` dual use** — called both for "what size should the window be" and "where does the quad go." The `request_size` vs `window_size` distinction already caused a bug (Regular mode stretching). The two call sites need different things from the same function.
- **`msg0`/`msg4` typed aliases** work correctly on both x86_64 and ARM64 for current signatures. Adding ObjC calls with different return types (BOOL, NSInteger) requires new aliases — document this constraint.
