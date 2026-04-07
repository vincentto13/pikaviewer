use std::{path::PathBuf, sync::Arc, time::Instant};

use anyhow::Result;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{Key, NamedKey},
    window::{Fullscreen, Window, WindowAttributes, WindowId},
};

use iv_core::{format::PluginRegistry, image_list::ImageList};
use iv_formats::default_registry;
use iv_renderer::{DisplayMode, Renderer};

// Duration for which the mode indicator is shown before fading out.
const MODE_DISPLAY_SECS: f32 = 5.0;
// The fade starts this many seconds before the indicator disappears.
const FADE_SECS: f32 = 1.0;

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub start_path: Option<PathBuf>,

    window:   Option<Arc<Window>>,
    renderer: Option<Renderer>,

    registry:   PluginRegistry,
    image_list: Option<ImageList>,

    // egui state (created in resumed, after the window exists)
    egui_ctx:   egui::Context,
    egui_state: Option<egui_winit::State>,

    // Overlay data
    current_filename: String,
    mode_changed_at:  Option<Instant>,
}

impl App {
    pub fn new(start_path: Option<PathBuf>) -> Self {
        let egui_ctx = egui::Context::default();
        // Transparent/dark style — we only want overlay labels, no panels/frames.
        egui_ctx.set_visuals(egui::Visuals {
            window_shadow: egui::epaint::Shadow::NONE,
            ..egui::Visuals::dark()
        });

        Self {
            start_path,
            window:   None,
            renderer: None,
            registry: default_registry(),
            image_list: None,
            egui_ctx,
            egui_state: None,
            current_filename: String::new(),
            mode_changed_at: None,
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn load_current(&mut self) {
        let (Some(list), Some(renderer)) =
            (self.image_list.as_mut(), self.renderer.as_mut())
        else { return };

        let Some(path) = list.current() else { return };
        let path = path.to_path_buf();

        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => { log::error!("read {}: {e}", path.display()); return; }
        };

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let plugin = match self.registry.find_for_extension(ext) {
            Some(p) => p,
            None => { log::error!("no plugin for '{ext}'"); return; }
        };

        match plugin.decode(&data) {
            Ok(image) => {
                renderer.set_image(&image);
                self.current_filename = format!(
                    "{}  [{}/{}]",
                    filename,
                    list.position(),
                    list.len()
                );

                if let Some(w) = &self.window {
                    if let Some((new_w, new_h)) = renderer.compute_window_size() {
                        let _ = w.request_inner_size(PhysicalSize::new(new_w, new_h));
                    }
                    w.request_redraw();
                }
            }
            Err(e) => log::error!("decode {}: {e}", path.display()),
        }
    }

    fn navigate(&mut self, delta: i64) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.rotation = 0;  // reset rotation on image change
        }
        if let Some(list) = self.image_list.as_mut() {
            list.advance(delta);
        }
        self.load_current();
    }

    fn toggle_mode(&mut self) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.display_mode = renderer.display_mode.next();

            if let Some(w) = &self.window {
                match renderer.display_mode {
                    DisplayMode::Fullscreen => {
                        w.set_fullscreen(Some(Fullscreen::Borderless(None)));
                    }
                    _ => {
                        w.set_fullscreen(None);
                    }
                }
            }
        }
        self.mode_changed_at = Some(Instant::now());
        self.load_current();
    }

    fn rotate_image(&mut self, clockwise: bool) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.rotate(clockwise);
            // In Regular mode, the window might need to resize for swapped dims.
            if let Some(w) = &self.window {
                if let Some((nw, nh)) = renderer.compute_window_size() {
                    let _ = w.request_inner_size(PhysicalSize::new(nw, nh));
                }
                w.request_redraw();
            }
        }
    }

    fn render_frame(&mut self) -> Result<()> {
        let (Some(renderer), Some(egui_state), Some(window)) =
            (self.renderer.as_mut(), self.egui_state.as_mut(), self.window.as_ref())
        else { return Ok(()) };

        // Pre-extract everything the UI closure needs so the closure
        // doesn't capture `self` (which is also borrowed by egui_ctx.run).
        let filename     = self.current_filename.clone();
        let mode_changed = self.mode_changed_at;
        let mode_label   = renderer.display_mode.label();

        let raw_input   = egui_state.take_egui_input(window);
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            draw_ui(ctx, &filename, mode_changed, mode_label);
        });

        egui_state.handle_platform_output(window, full_output.platform_output);

        let pixels_per_point = window.scale_factor() as f32;
        let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [
                window.inner_size().width,
                window.inner_size().height,
            ],
            pixels_per_point,
        };

        renderer.render(&paint_jobs, &full_output.textures_delta, &screen_desc)
    }
}

// ── Overlay UI (free fn to avoid borrow conflicts with App) ──────────────────

fn draw_ui(
    ctx:          &egui::Context,
    filename:     &str,
    mode_changed: Option<Instant>,
    mode_label:   &str,
) {
    // Filename — top-left, always visible
    if !filename.is_empty() {
        egui::Area::new(egui::Id::new("filename_overlay"))
            .anchor(egui::Align2::LEFT_TOP, [10.0, 10.0])
            .interactable(false)
            .show(ctx, |ui| overlay_label(ui, filename, 255));
    }

    // Mode indicator — top-right, fades after MODE_DISPLAY_SECS
    if let Some(changed_at) = mode_changed {
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
                .show(ctx, |ui| overlay_label(ui, mode_label, alpha));

            ctx.request_repaint();
        }
    }
}

// ── Small egui helper ─────────────────────────────────────────────────────────

/// Draw a semi-transparent pill label with `alpha` (0–255).
fn overlay_label(ui: &mut egui::Ui, text: &str, alpha: u8) {
    let bg  = egui::Color32::from_rgba_unmultiplied(0, 0, 0, (alpha as u16 * 160 / 255) as u8);
    let fg  = egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
    egui::Frame {
        fill:         bg,
        inner_margin: egui::Margin { left: 8.0, right: 8.0, top: 4.0, bottom: 4.0 },
        rounding:     egui::Rounding::same(4.0),
        ..egui::Frame::default()
    }
    .show(ui, |ui| {
        ui.label(egui::RichText::new(text).color(fg).size(14.0));
    });
}

// ── winit ApplicationHandler ──────────────────────────────────────────────────

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = WindowAttributes::default()
            .with_title("Image Viewer")
            .with_inner_size(PhysicalSize::new(800u32, 600u32));

        let window = match event_loop.create_window(attrs) {
            Ok(w)  => Arc::new(w),
            Err(e) => { log::error!("create window: {e}"); event_loop.exit(); return; }
        };

        let renderer = match Renderer::new(window.clone()) {
            Ok(r)  => r,
            Err(e) => { log::error!("init renderer: {e}"); event_loop.exit(); return; }
        };

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
        self.egui_state = Some(egui_state);

        // Build the image list from the start path.
        if let Some(path) = self.start_path.clone() {
            let result = if path.is_file() {
                ImageList::from_file(&path, &self.registry)
            } else {
                ImageList::from_directory(&path, &self.registry)
            };
            match result {
                Ok(list) => {
                    if list.is_empty() { log::warn!("no supported images found"); }
                    self.image_list = Some(list);
                    self.load_current();
                }
                Err(e) => log::error!("build image list: {e}"),
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Let egui process the event first.
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
                    w.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                if let Err(e) = self.render_frame() {
                    log::error!("render: {e}");
                }
            }

            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, .. },
                ..
            } => match logical_key {
                Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::ArrowDown) => {
                    self.navigate(1);
                }
                Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowUp) => {
                    self.navigate(-1);
                }
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Character(ref c) => match c.as_str() {
                    "q" | "Q" => event_loop.exit(),
                    "m" | "M" => self.toggle_mode(),
                    "r"       => self.rotate_image(true),   // clockwise
                    "l"       => self.rotate_image(false),  // counterclockwise
                    _ => {}
                },
                _ => {}
            },

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Keep repainting while the mode indicator is animating.
        let needs_repaint = self.mode_changed_at
            .map(|t| t.elapsed().as_secs_f32() < MODE_DISPLAY_SECS)
            .unwrap_or(false);

        if needs_repaint {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}
