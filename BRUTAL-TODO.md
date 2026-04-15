# Architectural Issues — Code Review

From the brutal critique of the zoom/pan implementation (2026-04-14).
Updated 2026-04-15 after refactor merge and second full review.

## Resolved (first review)

- **#1 — Renderer god struct**: Viewport extracted (zoom/pan state + GPU transform). ImageSlot deferred — low value until image state grows richer (animation, multi-page, undo).
- **#2 — Two coordinate spaces**: Unified into single affine transform (scale_x/y + translate). Unit quad, letterbox computed in Viewport. QuadNdc/Layout/letterbox() removed.
- **#3 — Implicit state sync**: Resolved as side effect of #2. quad_half_w/h eliminated, transform computed from source-of-truth (image_size, surface_dims, zoom, pan). refresh_vertex_buf() removed.
- **#5 — draw_ui parameter avalanche**: FrameSnapshot struct captures overlay state per frame. 12 params → 3.
- **#6 — Zoom state leaks**: Resolved as side effect of #1. zoom/pan private, accessed via intent methods (zoom_in, set_zoom, adjust_pan).

## Remaining (first review)

### #4 — App god struct [MEDIUM] — partially addressed

FrameSnapshot handles the draw_ui side. The keyboard handler still snapshots
is_zoomed/zoom into locals to work around borrow checker issues.

InputRouter or mode-based dispatch would help when mouse-drag pan or additional
input modes arrive. Not worth doing preemptively.

### #7 — No separation between logical and physical pixels [LOWER]

PAN_STEP is NDC, config.width is physical pixels. DPI factors cancel correctly
but by coincidence, not by design. Would matter for explicit HiDPI support or
multi-monitor with different scale factors.

Fix: explicit coordinate types or document which space each value is in.
Not urgent — the unified Viewport transform reduced the surface area where
this matters.

## New issues (second review, 2026-04-15)

### #8 — Premultiply alpha on main thread [MEDIUM]

`premultiply_alpha()` runs on every `set_image()` call on the main thread.
`srgb_table()` is rebuilt every call. Should be `LazyLock`. The premultiply
should run in the prefetch worker thread. A `has_alpha` hint on `DecodedImage`
would let us skip the entire loop for JPEG (always opaque).

### #9 — PrefetchCache::get() removes the entry [MEDIUM]

`get()` does `cache.remove()` — destructive. Navigate away and back = guaranteed
cache miss + full re-decode. Current image is never in cache. Fix: keep entry in
cache, only eviction removes it.

### #10 — DecodedImage ownership / allocation churn [LOWER]

Every navigation: allocate ~96MB → decode → move → premultiply → upload → free.
Tied to #9 — if cache keeps entries, this is partially addressed. Consider
reusing a pixel buffer.

### #11 — about_to_wait polls every frame [MEDIUM]

Polls `startup_rx` and `prefetch.poll()` unconditionally every idle cycle.
Creates a spin loop when `status == Loading`. Fix: use `EventLoopProxy` to send
a wakeup from the worker thread when decode completes.

### #12 — Settings saved on every resize event [LOW]

Every `Resized` event in fixed-size mode writes config.toml to disk. Window
resize generates dozens of events per second. Fix: debounce — save on close or
after idle timer.

### #13 — Renderer has split personality [MEDIUM]

Renderer holds both GPU rendering state AND application-level state
(`display_mode`, `fit_to_image`, `rotation`, `screen_size`). These leak through
the rendering boundary. Consider separating application state from GPU state.

### #14 — screen_size is stale [BUG]

`screen_size` is captured once during `Renderer::new()` and never updated.
Monitor changes or moving to a different monitor use stale dimensions for
`compute_window_size()`.

### #15 — No error recovery from GPU loss [LOWER]

`SurfaceError::Lost` is propagated as error, logged, and ignored. Renderer is
then broken for all subsequent frames. Need surface recreation or graceful exit.

### #16 — image_size duplicated [LOWER]

`Renderer.image_size` (pre-rotation) and `Viewport.image_size` (post-rotation)
can diverge if rotation changes without `viewport.update_image_size()`. Invariant
is implicit and fragile.

### #17 — SettingsWindow duplicates wgpu boilerplate [LOW]

~80 lines of surface creation + render loop code duplicated between Renderer and
SettingsWindow. Extract a shared `WindowSurface` struct.

### #18 — Keyboard handler is a 75-line match [MODERATE]

Flat match with inline logic, `if let Some(r)` / `if let Some(w)` repeated 8+
times. After `resumed()`, renderer and window are always `Some`. Consider a
`RunningState` struct to eliminate Option noise.

### #19 — load_current has hidden control flow [SUBTLE]

On cache miss, sets filename/index optimistically, then sets them again after
decode. If decode fails, optimistic filename stays with no error shown to user.

### #20 — EXIF orientation handles only 3 of 8 values [LOW]

Orientations 2, 4, 5, 7 (mirror transforms) are silently treated as 0. Uncommon
in practice but technically incorrect.

### #21 — Single prefetch worker thread [LOWER]

One worker means N-1 prefetch waits for N+1 to complete. For large images
(100-200ms each), backward navigation isn't pre-warmed. Consider 2-3 workers.
