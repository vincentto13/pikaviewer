# Architectural Issues — Code Review

Started 2026-04-14 from the brutal critique of the zoom/pan implementation.
Updated 2026-04-22 after the 0.3.x shipping cycle and a third full sweep.

## Resolved

First review:
- **#1 — Renderer god struct**: Viewport extracted (zoom/pan state + GPU transform). ImageSlot deferred — low value until image state grows richer.
- **#2 — Two coordinate spaces**: Unified into single affine transform. QuadNdc/Layout/letterbox() removed.
- **#3 — Implicit state sync**: Resolved as side effect of #2.
- **#5 — `draw_ui` parameter avalanche**: FrameSnapshot captures overlay state per frame. 12 params → 3.
- **#6 — Zoom state leaks**: `zoom`/`pan` private, accessed via intent methods.

Second review (0.3.0):
- **#8 — Premultiply alpha on main thread**: moved to prefetch worker; `srgb_table()` now `LazyLock`; `DecodedImage::has_alpha` skips the loop for JPEG.
- **#9 — `PrefetchCache::get()` removes the entry**: entries now stay in cache via `Arc<DecodedImage>`; clones are cheap pointer bumps.
- **#11 — `about_to_wait` polling**: replaced with `EventLoopProxy::send_event(AppEvent::DecodeReady)` from worker threads.
- **#12 — Settings saved on every resize event**: config now written on exit only.

Third review (0.3.2):
- **#21 — Single prefetch worker**: `WORKER_COUNT = 2` in `prefetch.rs:11`. Backward navigation pre-warms in parallel.

## Remaining

### #4 — App god struct [MEDIUM] — partial

FrameSnapshot handles `draw_ui`. The keyboard handler still snapshots `is_zoomed`/`zoom` into locals (`app.rs:1279-1280`) to work around borrow checker issues. A `RunningState` substruct (window + renderer + egui) would kill both this and #18.

### #7 — Logical vs physical pixels [LOWER]

`PAN_STEP = 0.1` (NDC), `config.width` is physical pixels. DPI factors cancel correctly but by coincidence. Would matter for explicit HiDPI or multi-monitor with mixed scale factors. Not urgent.

### #13 — Renderer has split personality [MEDIUM]

`Renderer` owns `display_mode`, `fit_to_image`, `rotation`, `screen_size` alongside GPU resources. App-level state leaks through the rendering boundary. Splitting into `Renderer` (GPU) + `ViewState` (app) would clarify ownership.

### #14 — `screen_size` is stale [BUG]

`renderer.rs:281` holds `screen_size`, captured once at `Renderer::new()` (line 461) and never updated. Used by `compute_window_size()` for fit-to-image. Window moved to a different monitor or the monitor resolution changes = wrong window size request.

**Fix**: either refresh in `resize()`/on `WindowEvent::Moved`, or delete the field and query `window.current_monitor().unwrap().size()` at call time.

### #15 — No error recovery from GPU loss [LOWER]

`renderer.rs:606-610` special-cases `SurfaceError::Outdated` but lets `Lost` fall into the `Err(e)` arm. The outer code logs and drops it; next frame tries again and fails again. Blank window forever, no recovery. Needs surface reconfigure on Lost.

### #16 — `image_size` duplicated [LOWER]

`Renderer.image_size` (pre-rotation) and `Viewport.image_size` (post-rotation) can diverge. Currently OK because `set_image()` updates both, but the invariant is implicit and fragile.

### #17 — SettingsWindow duplicates wgpu boilerplate [LOW]

~80 lines of surface creation + render loop shared with Renderer. Extract `WindowSurface` struct.

### #18 — Keyboard handler is a 75-line match [MODERATE]

`app.rs` keyboard match with inline logic, `if let Some(r)` / `if let Some(w)` repeated 8+ times. After `resumed()`, renderer and window are always `Some`. `RunningState` struct removes the Option noise and fixes #4 simultaneously.

### #19 — `load_current` has hidden control flow [SUBTLE]

On cache miss, sets filename/index optimistically, then sets them again after decode. If decode fails, the optimistic filename stays visible with no error shown to the user. `waiting_for_current` mitigates this (status flips Ready) but the stale name remains.

### #20 — EXIF orientation handles only 3 of 8 values [LOW]

`prefetch.rs:99-106` — orientations 2, 4, 5, 7 (mirror transforms) silently fall through to `_ => 0`. Uncommon but technically wrong. Fix requires a flip in the renderer (extra UV remap) or on the decoded pixels.

## New issues (third review, 2026-04-22)

### #22 — `file_size` shows 0 when `metadata()` fails but `read()` succeeds [LOW]

`prefetch.rs:288-289` — `file_size = fs::metadata().map_or(0, |m| m.len())`. If metadata fails (perms, racing unlink) but the subsequent `fs::read()` succeeds, the info panel shows `0 B` for a valid image. Trivial fix: set `file_size = data.len() as u64` after the successful read (authoritative anyway).

### #23 — `SurfaceError::Lost` not distinguished from other errors [BUG]

Same location as #15 (`renderer.rs:608`). Worth calling out separately because the fix is tiny — one `Err(Lost) => { self.surface.configure(...); Ok(()) }` arm — and it converts a silent hang into graceful recovery.

### #24 — Windows platform stubs are misleading [LOW]

`desktop_integration.rs` has a `#[cfg(not(...))]` no-op stub for Windows. `main.rs` `--install` flow runs but does nothing on Windows. CLAUDE.md lists Windows as "future" but the CLI doesn't say so. Either implement or print "unsupported on Windows" instead of silent success.

### #25 — Worker thread panic = silent stall [LOWER]

`prefetch.rs:272` uses `unwrap_or_else(PoisonError::into_inner)` to recover from a poisoned lock, but if a worker panics mid-`recv()`, the thread exits and is never restarted. With `WORKER_COUNT = 2`, one panic halves throughput silently; two panics = permanent "Loading…". Options: `parking_lot::Mutex` (no poisoning), or a supervisor that respawns workers. Low risk in practice (decode code paths don't panic), but the failure mode is invisible.

### #26 — `evict_if_needed` doesn't pin the current image [LOWER]

`prefetch.rs:247-258` evicts by `min last_used`. The currently-displayed image has the highest `last_used`, so it's never the victim — but only as long as the user navigates frequently. If user stays on an image and prefetch fills the cache with neighbors (5 entries, current + N±1 + N±2), the LRU order is stable. Edge case: if cache capacity drops below "current + visible prefetch radius", current could be evicted between navigation and redraw. Not observed in practice at `max_entries = 5`, but the invariant isn't enforced. Cheap fix: always skip the current path in `evict_if_needed`.

