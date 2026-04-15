use std::{path::PathBuf, sync::{mpsc, Arc}, time::Instant};

use anyhow::Result;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Fullscreen, Window, WindowAttributes, WindowId},
};

use iv_core::{format::PluginRegistry, image_list::ImageList};
use iv_renderer::{DisplayMode, GpuContext, Renderer};

use crate::config::Settings;
use crate::prefetch::{self, ExifData, PrefetchCache, DecodeResult};
use crate::settings_window::SettingsWindow;

const APP_NAME: &str = "PikaViewer";

// Duration for which the mode indicator stays visible before fading.
const MODE_DISPLAY_SECS: f32 = 5.0;
// Fade starts this many seconds before the indicator disappears.
const FADE_SECS: f32 = 1.0;

// Font sizes (logical points)
const FONT_FILENAME: f32 = 16.0;
const FONT_MODE:     f32 = 18.0;
const FONT_INFO:     f32 = 14.0;

// Minimum window size (logical pixels) — ensures room for the info panel.
const MIN_WINDOW_W: u32 = 600;
const MIN_WINDOW_H: u32 = 450;

// Zoom / pan
const ZOOM_STEP: f32 = 1.25;   // multiply / divide per keypress
const PAN_STEP:  f32 = 0.1;    // NDC per press (divided by zoom → constant pixel movement)

// ── Image info ───────────────────────────────────────────────────────────────

/// Metadata about the currently loaded image, shown in the info panel.
struct ImageInfo {
    width:     u32,
    height:    u32,
    file_size: u64,
    file_path: PathBuf,
    exif:      ExifData,
}

// ── App ───────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppStatus {
    Ready,
    Loading,
    EmptyFolder,
    FolderNotFound,
}

type StartupResult = Result<ImageList, String>;

pub struct App {
    pub start_path: Option<PathBuf>,

    // These are dropped explicitly in Drop impl to control ordering
    // relative to winit's Wayland connection teardown.
    settings_window: Option<SettingsWindow>,
    renderer: Option<Renderer>,
    gpu:      Option<Arc<GpuContext>>,
    egui_state: Option<egui_winit::State>,
    window:   Option<Arc<Window>>,

    registry:   Arc<PluginRegistry>,
    image_list: Option<ImageList>,
    prefetch:   Option<PrefetchCache>,
    startup_rx: Option<mpsc::Receiver<StartupResult>>,
    status:     AppStatus,

    egui_ctx:   egui::Context,

    settings:        Settings,

    // Overlay data
    current_filename: String,  // just the bare filename (no index)
    current_index:    String,  // "[pos/total]"
    mode_changed_at:  Option<Instant>,

    show_info:  bool,
    image_info: Option<ImageInfo>,

    pending_delete: bool,
    show_help:      bool,

    modifiers: ModifiersState,
}

impl Drop for App {
    fn drop(&mut self) {
        // Drop prefetch first — dropping request_tx causes worker thread exit.
        // Then wgpu resources in dependency order:
        // 1. prefetch       (worker thread, no GPU deps)
        // 2. settings window (Surface + egui renderer)
        // 3. main renderer   (Surface + Pipeline + textures)
        // 4. GPU context     (Device/Queue)
        // 5. window + egui_state drop naturally after this
        drop(self.prefetch.take());
        drop(self.settings_window.take());
        drop(self.renderer.take());
        drop(self.gpu.take());
    }
}

impl App {
    pub fn new(start_path: Option<PathBuf>, registry: Arc<PluginRegistry>) -> Self {
        let egui_ctx = egui::Context::default();
        egui_ctx.set_visuals(egui::Visuals {
            window_shadow: egui::epaint::Shadow::NONE,
            ..egui::Visuals::dark()
        });

        let settings = Settings::load_or_create();

        Self {
            start_path,
            window:   None,
            renderer: None,
            gpu:      None,
            registry,
            image_list: None,
            prefetch:   None,
            startup_rx: None,
            status:     AppStatus::Ready,
            egui_ctx,
            egui_state: None,
            settings,
            settings_window: None,
            current_filename: String::new(),
            current_index:    String::new(),
            mode_changed_at:  None,
            show_info:        false,
            image_info:       None,
            pending_delete:   false,
            show_help:        false,
            modifiers:        ModifiersState::default(),
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn load_current(&mut self) {
        let Some(list) = self.image_list.as_ref() else { return };
        let Some(path) = list.current() else { return };
        let path = path.to_path_buf();

        // Try the prefetch cache first
        if let Some(ref mut cache) = self.prefetch {
            if let Some(entry) = cache.get(&path) {
                log::debug!("cache hit: {}", path.display());
                self.status = AppStatus::Ready;
                let result = DecodeResult {
                    path: path.clone(),
                    image: Ok(entry.image),
                    exif: entry.exif,
                    file_size: entry.file_size,
                };
                self.apply_decode_result(result);
                self.trigger_prefetch();
                return;
            }
            // Cache miss — request background decode
            log::debug!("cache miss: {}", path.display());
            cache.request(path.clone());
            cache.waiting_for_current = true;
            self.status = AppStatus::Loading;

            // Update overlay immediately so filename/index reflect the target
            let filename = path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let index = format!("[{}/{}]", list.position(), list.len());
            self.current_filename = filename.clone();
            self.current_index = index.clone();
            if let Some(w) = &self.window {
                w.set_title(&format!("{APP_NAME} - {filename} {index}"));
                w.request_redraw();
            }
        }

    }

    /// Apply a completed decode result: set texture, EXIF rotation, update overlays.
    fn apply_decode_result(&mut self, result: DecodeResult) {
        let mut image = match result.image {
            Ok(img) => img,
            Err(e) => {
                log::error!("decode {}: {e}", result.path.display());
                self.status = AppStatus::Ready;
                return;
            }
        };

        let Some(renderer) = self.renderer.as_mut() else { return };
        let list = self.image_list.as_ref();

        renderer.rotation = prefetch::orientation_rotation(result.exif.orientation);

        let filename = result.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| result.path.display().to_string());

        self.image_info = Some(ImageInfo {
            width:     image.width,
            height:    image.height,
            file_size: result.file_size,
            file_path: result.path,
            exif:      result.exif,
        });

        renderer.set_image(&mut image);
        log::debug!("loaded {}×{} {}", image.width, image.height, filename);

        let index = list.map(|l| format!("[{}/{}]", l.position(), l.len()))
            .unwrap_or_default();
        self.current_filename = filename.clone();
        self.current_index    = index.clone();
        self.status = AppStatus::Ready;

        if let Some(w) = &self.window {
            w.set_title(&format!("{APP_NAME} - {filename} {index}"));

            if let Some((new_w, new_h)) = renderer.compute_window_size() {
                let _ = w.request_inner_size(clamped_size(w, new_w, new_h));
                clamp_to_screen(w);
            }
            w.request_redraw();
        }
    }

    /// Request prefetch of adjacent images (N+1, N-1).
    fn trigger_prefetch(&mut self) {
        let (Some(list), Some(cache)) =
            (self.image_list.as_ref(), self.prefetch.as_mut())
        else { return };

        if let Some(next) = list.peek_offset(1) {
            cache.request(next.to_path_buf());
        }
        if let Some(prev) = list.peek_offset(-1) {
            cache.request(prev.to_path_buf());
        }
    }

    /// Load a new image (and its directory) at runtime — used by macOS Apple
    /// Events when Finder asks us to open a file after the window is up.
    #[cfg(target_os = "macos")]
    fn load_path(&mut self, path: PathBuf) {
        if let Some(r) = self.renderer.as_mut() { r.rotation = 0; }
        if let Some(c) = self.prefetch.as_mut() { c.bump_generation(); }

        let registry = Arc::clone(&self.registry);
        let (tx, rx) = mpsc::channel();
        self.startup_rx = Some(rx);
        self.status = AppStatus::Loading;

        std::thread::Builder::new()
            .name("startup-scan".into())
            .spawn(move || {
                let path = path.canonicalize().unwrap_or(path);
                let result = if path.is_file() {
                    ImageList::from_file(&path, &registry)
                } else {
                    ImageList::from_directory(&path, &registry)
                };
                let _ = tx.send(result.map_err(|e| e.to_string()));
            })
            .expect("failed to spawn startup thread");
    }

    fn navigate(&mut self, delta: i64) {
        log::debug!("navigate delta={delta}");
        if let Some(r) = self.renderer.as_mut() { r.rotation = 0; r.viewport.reset_zoom(); }
        if let Some(c) = self.prefetch.as_mut() { c.bump_generation(); }
        if let Some(l) = self.image_list.as_mut() { l.advance(delta); }
        self.load_current();
    }

    fn toggle_mode(&mut self) {
        log::debug!("toggle display mode");
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.viewport.reset_zoom();
            renderer.display_mode = renderer.display_mode.next();
            if let Some(w) = &self.window {
                match renderer.display_mode {
                    DisplayMode::Fullscreen => {
                        w.set_fullscreen(Some(Fullscreen::Borderless(None)));
                    }
                    DisplayMode::Window => {
                        w.set_fullscreen(None);
                    }
                }
            }
        }
        self.mode_changed_at = Some(Instant::now());
        self.load_current();
    }

    fn zoom_in(&mut self) {
        if let Some(r) = self.renderer.as_mut() {
            r.viewport.zoom_in(ZOOM_STEP);
        }
        if let Some(w) = &self.window { w.request_redraw(); }
    }

    fn zoom_out(&mut self) {
        if let Some(r) = self.renderer.as_mut() {
            r.viewport.zoom_out(ZOOM_STEP);
        }
        if let Some(w) = &self.window { w.request_redraw(); }
    }

    fn open_settings(&mut self, event_loop: &ActiveEventLoop) {
        if self.settings_window.is_some() {
            return; // already open
        }
        let Some(gpu) = self.gpu.clone() else { return };
        match SettingsWindow::open(event_loop, gpu) {
            Ok(sw) => { self.settings_window = Some(sw); }
            Err(e) => log::error!("open settings: {e}"),
        }
    }

    fn close_settings(&mut self) {
        if let Some(sw) = self.settings_window.take() {
            if sw.dirty {
                if let Some(r) = self.renderer.as_mut() {
                    r.fit_to_image = self.settings.window.fit_to_image;
                }
                self.settings.save();
                self.load_current();
            }
        }
    }

    fn rotate_image(&mut self, clockwise: bool) {
        log::debug!("rotate {}", if clockwise { "CW" } else { "CCW" });
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.viewport.reset_zoom();
            renderer.rotate(clockwise);
            if let Some(w) = &self.window {
                if let Some((nw, nh)) = renderer.compute_window_size() {
                    let _ = w.request_inner_size(clamped_size(w, nw, nh));
                    clamp_to_screen(w);
                }
                w.request_redraw();
            }
        }
    }

    fn delete_current(&mut self) {
        let Some(list) = self.image_list.as_mut() else { return };
        let Some(removed) = list.remove_current() else { return };

        if let Err(e) = std::fs::remove_file(&removed) {
            log::error!("delete {:?}: {e}", removed);
            return;
        }
        log::info!("deleted: {:?}", removed);

        if let Some(c) = self.prefetch.as_mut() { c.invalidate(&removed); }

        if list.is_empty() {
            // No images left — clear the display
            if let Some(r) = self.renderer.as_mut() {
                r.clear_image();
            }
            self.current_filename.clear();
            self.current_index.clear();
            self.image_info = None;
            if let Some(w) = &self.window {
                w.set_title(APP_NAME);
                w.request_redraw();
            }
        } else {
            self.load_current();
        }
    }

    fn render_frame(&mut self) -> Result<()> {
        let (Some(renderer), Some(egui_state), Some(window)) =
            (self.renderer.as_mut(), self.egui_state.as_mut(), self.window.as_ref())
        else { return Ok(()) };

        let pixels_per_point = window.scale_factor() as f32;

        let snap = FrameSnapshot {
            filename:       self.current_filename.clone(),
            index:          self.current_index.clone(),
            scale_pct:      renderer.image_size.map(|_| (renderer.viewport.scale() * 100.0).round().max(1.0) as u32),
            mode_changed:   self.mode_changed_at,
            mode_label:     renderer.display_mode.label(),
            show_info:      self.show_info,
            pending_delete: self.pending_delete,
            show_help:      self.show_help,
            status:         self.status,
        };
        let image_info = self.image_info.as_ref();

        let raw_input   = egui_state.take_egui_input(window);
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            draw_ui(ctx, &snap, image_info);
        });

        egui_state.handle_platform_output(window, full_output.platform_output);

        // If egui itself wants another frame (e.g. to settle a freshly-shown
        // window's anchor position), schedule a winit redraw immediately.
        let wants_repaint = full_output.viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|vo| vo.repaint_delay == std::time::Duration::ZERO)
            .unwrap_or(false);
        if wants_repaint {
            window.request_redraw();
        }

        let paint_jobs  = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [window.inner_size().width, window.inner_size().height],
            pixels_per_point,
        };

        renderer.render(&paint_jobs, &full_output.textures_delta, &screen_desc)
    }
}

// ── Window helpers ────────────────────────────────────────────────────────────

/// Clamp physical pixel dimensions to the minimum window size (in logical px),
/// converting via the window's current scale factor.
fn clamped_size(window: &Window, w: u32, h: u32) -> PhysicalSize<u32> {
    let scale = window.scale_factor();
    let min_w = (MIN_WINDOW_W as f64 * scale).round() as u32;
    let min_h = (MIN_WINDOW_H as f64 * scale).round() as u32;
    PhysicalSize::new(w.max(min_w), h.max(min_h))
}

/// Move the window so it stays fully within its current monitor.
/// No-op on compositors that don't support querying/setting window position
/// (e.g. Wayland in most configurations).
fn clamp_to_screen(window: &Window) {
    let Some(monitor)   = window.current_monitor() else { return };
    let Ok(outer_pos)   = window.outer_position()  else { return };
    let screen_origin   = monitor.position();   // PhysicalPosition<i32>
    let screen_size     = monitor.size();        // PhysicalSize<u32>
    let inner           = window.inner_size();   // approximate for outer size

    let max_x = screen_origin.x + screen_size.width  as i32 - inner.width  as i32;
    let max_y = screen_origin.y + screen_size.height as i32 - inner.height as i32;

    let cx = outer_pos.x.max(screen_origin.x).min(max_x.max(screen_origin.x));
    let cy = outer_pos.y.max(screen_origin.y).min(max_y.max(screen_origin.y));

    if cx != outer_pos.x || cy != outer_pos.y {
        window.set_outer_position(winit::dpi::PhysicalPosition::new(cx, cy));
    }
}

// ── Overlay UI ────────────────────────────────────────────────────────────────

/// All overlay state captured once per frame, avoiding parameter threading.
/// Owns cloned strings because `egui_ctx.run()` borrows `self.egui_ctx`,
/// preventing the snapshot from holding references to other `self` fields.
struct FrameSnapshot {
    filename:       String,
    index:          String,
    scale_pct:      Option<u32>,
    mode_changed:   Option<Instant>,
    mode_label:     &'static str,
    show_info:      bool,
    pending_delete: bool,
    show_help:      bool,
    status:         AppStatus,
}

fn draw_ui(ctx: &egui::Context, snap: &FrameSnapshot, image_info: Option<&ImageInfo>) {
    // Use egui's own screen rect — always correct regardless of DPI scaling.
    let win_w = ctx.screen_rect().width();

    // Frame inner margins: left(10) + right(10) = 20 logical px.
    const FRAME_H_MARGIN: f32 = 20.0;
    // 80 % of window width is the outer overlay limit; subtract frame margins
    // to get the budget for the text content itself.
    let text_budget = win_w * 0.80 - FRAME_H_MARGIN;

    // ── Filename + index — top-left, always visible ───────────────────────────
    if !snap.filename.is_empty() {
        let font_id = egui::FontId::proportional(FONT_FILENAME);

        // Reserve pixels for the index + scale suffix " [pos/total] 58%", fit
        // the rest with the filename (truncating from the right with … before extension).
        let scale_str = snap.scale_pct.map(|p| format!(" {p}%")).unwrap_or_default();
        let index_suffix = format!("  {}{scale_str}", snap.index);
        let index_w = ctx.fonts(|f| {
            f.layout_no_wrap(index_suffix.clone(), font_id.clone(), egui::Color32::WHITE)
                .size().x
        });
        let name_budget = (text_budget - index_w).max(0.0);
        let short_name  = truncate_filename(ctx, &snap.filename, &font_id, name_budget);
        let label_text  = format!("{short_name}{index_suffix}");

        egui::Area::new(egui::Id::new("filename_overlay"))
            .anchor(egui::Align2::LEFT_TOP, [10.0, 10.0])
            .interactable(false)
            .show(ctx, |ui| {
                overlay_label(ui, &label_text, 255, FONT_FILENAME);
            });
    }

    // ── Mode indicator — top-right, fades after MODE_DISPLAY_SECS ────────────
    if let Some(changed_at) = snap.mode_changed {
        let elapsed = changed_at.elapsed().as_secs_f32();
        if elapsed < MODE_DISPLAY_SECS {
            let alpha = if elapsed < MODE_DISPLAY_SECS - FADE_SECS {
                255u8
            } else {
                ((MODE_DISPLAY_SECS - elapsed) / FADE_SECS * 255.0) as u8
            };

            egui::Area::new(egui::Id::new("mode_overlay"))
                .anchor(egui::Align2::RIGHT_TOP, [-10.0, 10.0])
                .interactable(false)
                .show(ctx, |ui| {
                    overlay_label(ui, snap.mode_label, alpha, FONT_MODE);
                });

            ctx.request_repaint();
        }
    }

    // ── Info panel — toggled with I key ──────────────────────────────────────
    if snap.show_info {
        if let Some(info) = image_info {
            draw_info_panel(ctx, info);
        }
    }

    // ── Delete confirmation dialog — centred ────────────────────────────────
    if snap.pending_delete {
        let frame = egui::Frame {
            fill:         egui::Color32::from_rgba_unmultiplied(0, 0, 0, 240),
            inner_margin: egui::Margin::same(20.0),
            rounding:     egui::Rounding::same(8.0),
            stroke:       egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
            ..egui::Frame::default()
        };

        egui::Window::new("Confirm Delete")
            .id(egui::Id::new("delete_confirm"))
            .frame(frame)
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Delete \"{}\"?", snap.filename))
                            .color(egui::Color32::WHITE)
                            .size(18.0)
                            .strong(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Press Y / Enter to confirm, N / Esc to cancel")
                            .color(egui::Color32::from_gray(180))
                            .size(15.0),
                    );
                });
            });
    }

    // ── Help overlay — centred, lists all shortcuts ─────────────────────────
    if snap.show_help {
        let frame = egui::Frame {
            fill:         egui::Color32::from_rgba_unmultiplied(0, 0, 0, 240),
            inner_margin: egui::Margin::same(20.0),
            rounding:     egui::Rounding::same(8.0),
            stroke:       egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
            ..egui::Frame::default()
        };

        egui::Window::new("Keyboard Shortcuts")
            .id(egui::Id::new("help_overlay"))
            .frame(frame)
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Keyboard Shortcuts")
                        .color(egui::Color32::WHITE)
                        .size(20.0)
                        .strong(),
                );
                ui.add_space(10.0);

                // (keys, separator, description)
                // "/" = alternatives, "+" = combo
                let shortcuts: &[(&[&str], &str, &str)] = &[
                    (&["Right", "Down"],                "/", "Next image (pan right/down when zoomed)"),
                    (&["Left", "Up"],                   "/", "Prev image (pan left/up when zoomed)"),
                    (&["."],                            "/", "Next image"),
                    (&[","],                            "/", "Previous image"),
                    (&["Space"],                        "/", "Next image / reset zoom when zoomed"),
                    (&["Shift+Space"],                  "/", "Prev image / reset zoom when zoomed"),
                    (&["+", "="],                       "/", "Zoom in"),
                    (&["-"],                            "/", "Zoom out / reset"),
                    (&["M"],                            "/", "Cycle display mode"),
                    (&["R", "]"],                       "/", "Rotate 90\u{00b0} clockwise"),
                    (&["L", "["],                       "/", "Rotate 90\u{00b0} counter-clockwise"),
                    (&["I"],                            "/", "Toggle image info panel"),
                    (&["D"],                            "/", "Delete current image"),
                    (&["H"],                            "/", "Show this help"),
                    (&["Ctrl/\u{2318}", ","],           "+", "Open settings"),
                    (&["Ctrl/\u{2318}", "W"],           "+", "Close window"),
                    (&["Q", "Escape"],                  "/", "Quit"),
                ];

                egui::Grid::new("help_grid")
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        for (keys, sep, desc) in shortcuts {
                            ui.horizontal(|ui| {
                                for (i, key) in keys.iter().enumerate() {
                                    if i > 0 {
                                        ui.label(
                                            egui::RichText::new(*sep)
                                                .color(egui::Color32::from_gray(100))
                                                .size(14.0),
                                        );
                                    }
                                    key_pill(ui, key);
                                }
                            });
                            ui.label(
                                egui::RichText::new(*desc)
                                    .color(egui::Color32::from_gray(170))
                                    .size(15.0),
                            );
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("Press any key to close")
                        .color(egui::Color32::from_gray(120))
                        .size(13.0)
                        .italics(),
                );
            });
    }

    // ── Status indicator — centred ─────────────────────────────────────────────
    let status_text = match snap.status {
        AppStatus::Loading       => Some("Loading\u{2026}"),
        AppStatus::EmptyFolder   => Some("No images found"),
        AppStatus::FolderNotFound => Some("Folder not found"),
        AppStatus::Ready         => None,
    };
    if let Some(text) = status_text {
        egui::Area::new(egui::Id::new("status_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .interactable(false)
            .show(ctx, |ui| {
                overlay_label(ui, text, 200, 20.0);
            });
        if snap.status == AppStatus::Loading {
            ctx.request_repaint();
        }
    }
}

/// Draw a single key label inside a rounded pill (bright background rectangle).
fn key_pill(ui: &mut egui::Ui, text: &str) {
    let font = egui::FontId::proportional(14.0);
    let text_color = egui::Color32::from_gray(230);
    let bg_color   = egui::Color32::from_gray(55);
    let rounding   = egui::Rounding::same(4.0);
    let padding    = egui::vec2(6.0, 2.0);

    let galley = ui.fonts(|f| {
        f.layout_no_wrap(text.to_string(), font, text_color)
    });
    let desired = galley.size() + padding * 2.0;
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    ui.painter().rect_filled(rect, rounding, bg_color);
    ui.painter().galley(rect.min + padding, galley, text_color);
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    let label_color = egui::Color32::from_gray(160);
    let value_color = egui::Color32::WHITE;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(label_color).size(FONT_INFO));
        ui.label(egui::RichText::new(value).color(value_color).size(FONT_INFO).strong());
    });
}

fn draw_info_panel(ctx: &egui::Context, info: &ImageInfo) {
    let frame = egui::Frame {
        fill:         egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
        inner_margin: egui::Margin::same(12.0),
        rounding:     egui::Rounding::same(6.0),
        stroke:       egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
        ..egui::Frame::default()
    };

    egui::Window::new("Image Info")
        .id(egui::Id::new("info_panel"))
        .frame(frame)
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .movable(false)
        .anchor(egui::Align2::RIGHT_BOTTOM, [-10.0, -10.0])
        .show(ctx, |ui| {
            let filename = info.file_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "—".to_string());
            info_row(ui, "File:", &filename);
            info_row(ui, "Dimensions:", &format!("{} × {}", info.width, info.height));
            let (rw, rh) = simplify_ratio(info.width, info.height);
            info_row(ui, "Ratio:", &format!("{rw}:{rh}"));
            info_row(ui, "Size:", &format_file_size(info.file_size));
            let ext = info.file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("—")
                .to_ascii_uppercase();
            info_row(ui, "Format:", &ext);

            // ── EXIF section — only shown when at least one field is present ──
            let exif = &info.exif;
            let has_exif = exif.camera_make.is_some()
                || exif.camera_model.is_some()
                || exif.lens_model.is_some()
                || exif.exposure_time.is_some()
                || exif.f_number.is_some()
                || exif.iso.is_some()
                || exif.focal_length.is_some()
                || exif.date_taken.is_some();

            if has_exif {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);

                // Camera: "Make Model" combined, or whichever field is present
                match (&exif.camera_make, &exif.camera_model) {
                    (Some(make), Some(model)) => info_row(ui, "Camera:", &format!("{make} {model}")),
                    (Some(make), None)        => info_row(ui, "Camera:", make),
                    (None, Some(model))       => info_row(ui, "Camera:", model),
                    (None, None)              => {}
                }

                if let Some(lens) = &exif.lens_model {
                    info_row(ui, "Lens:", lens);
                }

                // Exposure summary: shutter / aperture / ISO / focal length on one line
                let iso_str = exif.iso.as_deref().map(|s| format!("ISO {s}"));
                let mut parts: Vec<&str> = Vec::new();
                if let Some(v) = &exif.exposure_time { parts.push(v); }
                if let Some(v) = &exif.f_number      { parts.push(v); }
                if let Some(v) = &iso_str            { parts.push(v); }
                if let Some(v) = &exif.focal_length  { parts.push(v); }
                if !parts.is_empty() {
                    info_row(ui, "Exposure:", &parts.join("  "));
                }

                if let Some(date) = &exif.date_taken {
                    info_row(ui, "Taken:", date);
                }
            }
        });
}

fn simplify_ratio(w: u32, h: u32) -> (u32, u32) {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    if w == 0 || h == 0 { return (w, h); }
    let g = gcd(w, h);
    (w / g, h / g)
}

fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Truncate `filename` so it fits within `max_w` logical pixels at `font_id`.
/// Preserves the file extension; inserts `…` before the extension.
///
/// e.g. "very_long_photo_name.jpg" → "very_long_ph….jpg"
fn truncate_filename(
    ctx:      &egui::Context,
    filename: &str,
    font_id:  &egui::FontId,
    max_w:    f32,
) -> String {
    ctx.fonts(|fonts| {
        let measure = |s: &str| -> f32 {
            fonts
                .layout_no_wrap(s.to_string(), font_id.clone(), egui::Color32::WHITE)
                .size()
                .x
        };

        if measure(filename) <= max_w {
            return filename.to_string();
        }

        // Split stem and extension (e.g. "photo" + ".jpg")
        let (stem, ext) = match filename.rfind('.') {
            Some(i) => (&filename[..i], &filename[i..]),
            None    => (filename, ""),
        };

        // Suffix is "…ext" — if even that doesn't fit, return just "…"
        let suffix   = format!("\u{2026}{ext}");   // …ext
        let suffix_w = measure(&suffix);
        if suffix_w >= max_w {
            return "\u{2026}".to_string();
        }

        let stem_budget = max_w - suffix_w;
        let stem_chars: Vec<char> = stem.chars().collect();

        // Binary search: largest prefix of stem that fits in stem_budget
        let mut lo = 0usize;
        let mut hi = stem_chars.len();
        while lo < hi {
            let mid  = (lo + hi).div_ceil(2);
            let s: String = stem_chars[..mid].iter().collect();
            if measure(&s) <= stem_budget { lo = mid; } else { hi = mid - 1; }
        }

        let truncated: String = stem_chars[..lo].iter().collect();
        format!("{truncated}\u{2026}{ext}")
    })
}

/// Semi-transparent pill label. `font_size` in logical points; text is bold,
/// single-line (wrapping disabled — caller is responsible for pre-truncation).
fn overlay_label(ui: &mut egui::Ui, text: &str, alpha: u8, font_size: f32) {
    let bg = egui::Color32::from_rgba_unmultiplied(0, 0, 0, (alpha as u16 * 160 / 255) as u8);
    let fg = egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
    egui::Frame {
        fill:         bg,
        inner_margin: egui::Margin { left: 10.0, right: 10.0, top: 5.0, bottom: 5.0 },
        rounding:     egui::Rounding::same(5.0),
        ..egui::Frame::default()
    }
    .show(ui, |ui| {
        // Disable egui's word-wrap so the label stays on a single line.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        ui.label(
            egui::RichText::new(text)
                .color(fg)
                .size(font_size)
                .strong(),
        );
    });
}

// ── winit ApplicationHandler ──────────────────────────────────────────────────

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Use saved window size when in fixed-size mode, otherwise default.
        let initial_size = if self.settings.window.fit_to_image {
            PhysicalSize::new(800u32, 600u32)
        } else {
            PhysicalSize::new(self.settings.window.width, self.settings.window.height)
        };

        let attrs = WindowAttributes::default()
            .with_title(APP_NAME)
            .with_inner_size(initial_size)
            .with_min_inner_size(LogicalSize::new(MIN_WINDOW_W, MIN_WINDOW_H));

        let window = match event_loop.create_window(attrs) {
            Ok(w)  => Arc::new(w),
            Err(e) => { log::error!("create window: {e}"); event_loop.exit(); return; }
        };

        let (mut renderer, gpu) = match Renderer::new(window.clone()) {
            Ok(r)  => r,
            Err(e) => { log::error!("init renderer: {e}"); event_loop.exit(); return; }
        };
        renderer.fit_to_image = self.settings.window.fit_to_image;

        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            None,  // native_pixels_per_point
            None,  // theme
            None,  // max_texture_side
        );

        self.window     = Some(window);
        self.renderer   = Some(renderer);
        self.gpu        = Some(gpu);
        self.egui_state = Some(egui_state);
        self.prefetch   = Some(PrefetchCache::new(Arc::clone(&self.registry)));

        if let Some(path) = self.start_path.clone() {
            let registry = Arc::clone(&self.registry);
            let (tx, rx) = mpsc::channel();
            self.startup_rx = Some(rx);
            self.status = AppStatus::Loading;

            std::thread::Builder::new()
                .name("startup-scan".into())
                .spawn(move || {
                    let path = path.canonicalize().unwrap_or(path);
                    let result = if path.is_file() {
                        ImageList::from_file(&path, &registry)
                    } else {
                        ImageList::from_directory(&path, &registry)
                    };
                    let _ = tx.send(result.map_err(|e| e.to_string()));
                })
                .expect("failed to spawn startup thread");
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // ── Settings window events ───────────────────────────────────────────
        if self.settings_window.as_ref().map(|sw| sw.window_id()) == Some(window_id) {
            let should_close = self.settings_window.as_mut()
                .map(|sw| sw.handle_event(&event, &mut self.settings))
                .unwrap_or(false);
            if should_close {
                self.close_settings();
            }
            return;
        }

        // ── Main window events ───────────────────────────────────────────────
        if let (Some(state), Some(window)) =
            (self.egui_state.as_mut(), self.window.as_ref())
        {
            let _ = state.on_window_event(window, &event);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
                if let Some(w) = &self.window {
                    if self.renderer.as_ref()
                        .map(|r| r.display_mode == DisplayMode::Window && r.fit_to_image)
                        .unwrap_or(false)
                    {
                        clamp_to_screen(w);
                    }
                    w.request_redraw();
                }

                // Save window size for fixed-size mode restoration.
                if !self.settings.window.fit_to_image && size.width > 0 && size.height > 0 {
                    self.settings.window.width  = size.width;
                    self.settings.window.height = size.height;
                    self.settings.save();
                }
            }

            WindowEvent::RedrawRequested => {
                if let Err(e) = self.render_frame() {
                    log::error!("render: {e}");
                }
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            WindowEvent::KeyboardInput {
                event: KeyEvent { ref logical_key, state: ElementState::Pressed, .. },
                ..
            } if self.show_help => {
                // Any key dismisses help.  If the key is H (toggle), just
                // close — don't propagate, or it immediately re-opens.
                self.show_help = false;
                if let Some(w) = &self.window { w.request_redraw(); }
                let is_h = matches!(logical_key, Key::Character(c) if c == "h" || c == "H");
                if !is_h {
                    self.window_event(event_loop, window_id, event);
                }
            }

            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, .. },
                ..
            } if self.pending_delete => {
                // Confirmation mode — only accept confirm/cancel keys
                let confirm = matches!(logical_key, Key::Named(NamedKey::Enter))
                    || matches!(&logical_key, Key::Character(c) if c == "y" || c == "Y");
                let cancel  = matches!(logical_key, Key::Named(NamedKey::Escape))
                    || matches!(&logical_key, Key::Character(c) if c == "n" || c == "N");

                if confirm {
                    self.pending_delete = false;
                    self.delete_current();
                } else if cancel {
                    self.pending_delete = false;
                    if let Some(w) = &self.window { w.request_redraw(); }
                }
            }

            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, .. },
                ..
            } => {
                // Snapshot before match — may go stale if an arm mutates the
                // renderer (e.g. navigate → reset_zoom), but no arm reads these
                // values after such a mutation.
                let is_zoomed = self.renderer.as_ref().map(|r| r.viewport.is_zoomed()).unwrap_or(false);
                let zoom      = self.renderer.as_ref().map(|r| r.viewport.zoom()).unwrap_or(1.0);
                match logical_key {
                Key::Named(NamedKey::ArrowRight) => {
                    if is_zoomed {
                        if let Some(r) = self.renderer.as_mut() { r.viewport.adjust_pan(-(PAN_STEP / zoom), 0.0); }
                        if let Some(w) = &self.window { w.request_redraw(); }
                    } else { self.navigate(1); }
                }
                Key::Named(NamedKey::ArrowDown) => {
                    if is_zoomed {
                        if let Some(r) = self.renderer.as_mut() { r.viewport.adjust_pan(0.0, PAN_STEP / zoom); }
                        if let Some(w) = &self.window { w.request_redraw(); }
                    } else { self.navigate(1); }
                }
                Key::Named(NamedKey::ArrowLeft) => {
                    if is_zoomed {
                        if let Some(r) = self.renderer.as_mut() { r.viewport.adjust_pan(PAN_STEP / zoom, 0.0); }
                        if let Some(w) = &self.window { w.request_redraw(); }
                    } else { self.navigate(-1); }
                }
                Key::Named(NamedKey::ArrowUp) => {
                    if is_zoomed {
                        if let Some(r) = self.renderer.as_mut() { r.viewport.adjust_pan(0.0, -(PAN_STEP / zoom)); }
                        if let Some(w) = &self.window { w.request_redraw(); }
                    } else { self.navigate(-1); }
                }
                Key::Named(NamedKey::Space) => {
                    if is_zoomed {
                        if let Some(r) = self.renderer.as_mut() { r.viewport.reset_zoom(); }
                        if let Some(w) = &self.window { w.request_redraw(); }
                    } else if self.modifiers.shift_key() {
                        self.navigate(-1);
                    } else {
                        self.navigate(1);
                    }
                }
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Character(ref c) => match c.as_str() {
                    "q" | "Q" => event_loop.exit(),
                    "w" | "W" if self.modifiers.super_key() || self.modifiers.control_key() => {
                        event_loop.exit();
                    }
                    "m" | "M" => self.toggle_mode(),
                    "r"       => self.rotate_image(true),
                    "l"       => self.rotate_image(false),
                    "i"       => {
                        self.show_info = !self.show_info;
                        if let Some(w) = &self.window { w.request_redraw(); }
                    }
                    "d" | "D" => {
                        if self.image_list.as_ref().map(|l| !l.is_empty()).unwrap_or(false) {
                            self.pending_delete = true;
                            if let Some(w) = &self.window { w.request_redraw(); }
                        }
                    }
                    "h" | "H" => {
                        self.show_help = true;
                        if let Some(w) = &self.window { w.request_redraw(); }
                    }
                    "."       => self.navigate(1),
                    // Ctrl/Cmd + , opens settings; plain , navigates
                    ","       => {
                        if self.modifiers.control_key() || self.modifiers.super_key() {
                            self.open_settings(event_loop);
                        } else {
                            self.navigate(-1);
                        }
                    }
                    "]"       => self.rotate_image(true),
                    "["       => self.rotate_image(false),
                    "+" | "=" => self.zoom_in(),
                    "-"        => self.zoom_out(),
                    _ => {}
                },
                _ => {}
                } // match logical_key
            },

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // macOS: pick up files delivered via kAEOpenDocuments Apple Event.
        #[cfg(target_os = "macos")]
        {
            let path = crate::macos_events::PENDING_FILE
                .lock().ok()
                .and_then(|mut g| g.take());
            if let Some(path) = path {
                self.load_path(path);
            }
        }

        // Poll startup thread (directory scan on background thread)
        if self.startup_rx.is_some() {
            let result = self.startup_rx.as_ref().unwrap().try_recv().ok();
            if let Some(result) = result {
                self.startup_rx = None; // one-shot
                match result {
                    Ok(list) => {
                        if list.is_empty() {
                            log::warn!("no supported images found");
                            self.status = AppStatus::EmptyFolder;
                            if let Some(w) = &self.window { w.request_redraw(); }
                        } else {
                            log::info!("directory scan complete: {} images", list.len());
                            self.image_list = Some(list);
                            self.load_current();
                        }
                    }
                    Err(e) => {
                        log::error!("build image list: {e}");
                        self.status = AppStatus::FolderNotFound;
                        if let Some(w) = &self.window { w.request_redraw(); }
                    }
                }
            }
        }

        // Poll background decode results — collect first to avoid borrow conflict
        let mut current_result: Option<DecodeResult> = None;
        if let Some(ref mut cache) = self.prefetch {
            let results = cache.poll();
            let current_path = self.image_list.as_ref()
                .and_then(|l| l.current())
                .map(|p| p.to_path_buf());

            for result in results {
                if current_path.as_ref() == Some(&result.path) {
                    cache.waiting_for_current = false;
                    current_result = Some(result);
                } else {
                    cache.insert(result);
                }
            }
        }
        if let Some(result) = current_result {
            self.apply_decode_result(result);
            self.trigger_prefetch();
        }

        let needs_repaint = self.mode_changed_at
            .map(|t| t.elapsed().as_secs_f32() < MODE_DISPLAY_SECS)
            .unwrap_or(false);

        // Repaint if mode indicator is fading or we're waiting for a decode
        if needs_repaint || self.status == AppStatus::Loading {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

