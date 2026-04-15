use std::sync::Arc;

use anyhow::Result;
use winit::{
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

use iv_renderer::GpuContext;
use crate::config::Settings;

const SETTINGS_TITLE: &str = "PikaViewer — Settings";
const SETTINGS_W: u32 = 360;
const SETTINGS_H: u32 = 260;

pub struct SettingsWindow {
    // Drop order: GPU resources before gpu context, window last-ish.
    egui_renderer: egui_wgpu::Renderer,
    egui_state:    egui_winit::State,
    egui_ctx:      egui::Context,
    surface:       wgpu::Surface<'static>,
    config:        wgpu::SurfaceConfiguration,
    window:        Arc<Window>,
    gpu:           Arc<GpuContext>,

    /// Whether the settings changed and need saving.
    pub dirty: bool,

    /// Status message after "Set as Default" attempt.
    default_status: Option<(String, bool)>, // (message, is_error)
}

impl SettingsWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        gpu: Arc<GpuContext>,
    ) -> Result<Self> {
        let attrs = WindowAttributes::default()
            .with_title(SETTINGS_TITLE)
            .with_inner_size(LogicalSize::new(SETTINGS_W, SETTINGS_H))
            .with_resizable(false);

        let window = Arc::new(
            event_loop.create_window(attrs)
                .map_err(|e| anyhow::anyhow!("create settings window: {e}"))?
        );

        let surface = gpu.instance.create_surface(window.clone())
            .map_err(|e| anyhow::anyhow!("create settings surface: {e}"))?;

        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps.formats.iter().copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(caps.formats[0]);

        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage:                         wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width:                         size.width.max(1),
            height:                        size.height.max(1),
            present_mode:                  wgpu::PresentMode::AutoVsync,
            alpha_mode:                    caps.alpha_modes[0],
            view_formats:                  vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&gpu.device, &config);

        let egui_ctx = egui::Context::default();
        egui_ctx.set_visuals(egui::Visuals::dark());

        let egui_renderer = egui_wgpu::Renderer::new(&gpu.device, format, None, 1, false);

        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::from_hash_of("settings"),
            event_loop,
            None, None, None,
        );

        Ok(Self {
            window,
            surface,
            config,
            egui_renderer,
            egui_state,
            egui_ctx,
            gpu,
            dirty: false,
            default_status: None,
        })
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// Handle a winit event for this window. Returns `true` if the window
    /// should be closed (user pressed close or Escape).
    pub fn handle_event(&mut self, event: &WindowEvent, settings: &mut Settings) -> bool {
        let response = self.egui_state.on_window_event(&self.window, event);

        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event: winit::event::KeyEvent {
                    logical_key: winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape),
                    state: winit::event::ElementState::Pressed,
                    ..
                },
                ..
            } => return true,

            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    self.config.width  = size.width;
                    self.config.height = size.height;
                    self.surface.configure(&self.gpu.device, &self.config);
                }
                self.window.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                if let Err(e) = self.render(settings) {
                    log::error!("settings render: {e}");
                }
            }

            _ => {
                // Redraw after any input event so egui can react (clicks, hovers, etc.)
                if response.repaint {
                    self.window.request_redraw();
                }
            }
        }
        false
    }

    fn render(&mut self, settings: &mut Settings) -> Result<()> {
        let raw_input = self.egui_state.take_egui_input(&self.window);
        let mut fit = settings.window.fit_to_image;

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame {
                    fill: egui::Color32::from_gray(30),
                    inner_margin: egui::Margin::same(20.0),
                    ..egui::Frame::default()
                })
                .show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.add_space(12.0);

                    let prev = fit;
                    ui.checkbox(&mut fit, "Fit window to image size");

                    if fit != prev {
                        self.dirty = true;
                    }

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);

                    if ui.button("Set as Default Image Viewer").clicked() {
                        match crate::desktop_integration::set_default() {
                            Ok(()) => {
                                self.default_status = Some((
                                    "PikaViewer set as default!".to_string(),
                                    false,
                                ));
                            }
                            Err(e) => {
                                self.default_status = Some((
                                    format!("Failed: {e}"),
                                    true,
                                ));
                            }
                        }
                    }

                    if let Some((msg, is_err)) = &self.default_status {
                        let color = if *is_err {
                            egui::Color32::from_rgb(255, 100, 100)
                        } else {
                            egui::Color32::from_rgb(100, 255, 100)
                        };
                        ui.label(egui::RichText::new(msg).color(color).size(12.0));
                    }
                });
        });

        if self.dirty {
            settings.window.fit_to_image = fit;
        }

        self.egui_state.handle_platform_output(&self.window, full_output.platform_output);

        let pixels_per_point = self.window.scale_factor() as f32;
        let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };

        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
        }
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        let output = match self.surface.get_current_texture() {
            Ok(t)                              => t,
            Err(wgpu::SurfaceError::Outdated)  => return Ok(()),
            Err(e)                             => return Err(e.into()),
        };
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.gpu.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("settings_encoder") },
        );

        self.egui_renderer.update_buffers(
            &self.gpu.device, &self.gpu.queue, &mut encoder, &paint_jobs, &screen_desc,
        );

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("settings_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view:           &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load:  wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            })
            .forget_lifetime();

            self.egui_renderer.render(&mut pass, &paint_jobs, &screen_desc);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // Schedule repaint if egui wants one
        let wants_repaint = full_output.viewport_output
            .get(&egui::ViewportId::from_hash_of("settings"))
            .or_else(|| full_output.viewport_output.values().next())
            .is_some_and(|vo| vo.repaint_delay == std::time::Duration::ZERO);
        if wants_repaint {
            self.window.request_redraw();
        }

        Ok(())
    }
}
