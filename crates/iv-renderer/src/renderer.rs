use std::sync::Arc;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::window::Window;

use iv_core::format::DecodedImage;

use crate::display_mode::{compute_window_request, DisplayMode};

/// Shared GPU state that can be used by multiple windows (main + settings).
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter:  wgpu::Adapter,
    pub device:   wgpu::Device,
    pub queue:    wgpu::Queue,
}

// ── Vertex layout ─────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv:       [f32; 2],
}

impl Vertex {
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode:    wgpu::VertexStepMode::Vertex,
        attributes:   &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
    };
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct TransformUniform {
    scale_x:     f32,
    scale_y:     f32,
    translate_x: f32,
    translate_y: f32,
}

impl TransformUniform {
    fn identity() -> Self {
        Self { scale_x: 1.0, scale_y: 1.0, translate_x: 0.0, translate_y: 0.0 }
    }
}

// ── Premultiplied alpha ───────────────────────────────────────────────────────

/// Build the sRGB→linear lookup table (256 entries).
fn srgb_table() -> [f32; 256] {
    let mut table = [0.0f32; 256];
    for i in 0..256 {
        let s = i as f32 / 255.0;
        table[i] = if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        };
    }
    table
}

fn linear_to_srgb_u8(l: f32) -> u8 {
    let s = if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// Premultiply alpha in linear space for correct sRGB blending.
/// Modifies pixel data in place. Skips fully opaque pixels (the common case).
fn premultiply_alpha(pixels: &mut [u8]) {
    let table = srgb_table();
    for chunk in pixels.chunks_exact_mut(4) {
        let a = chunk[3];
        if a == 0 {
            chunk[0] = 0;
            chunk[1] = 0;
            chunk[2] = 0;
        } else if a < 255 {
            let af = a as f32 / 255.0;
            chunk[0] = linear_to_srgb_u8(table[chunk[0] as usize] * af);
            chunk[1] = linear_to_srgb_u8(table[chunk[1] as usize] * af);
            chunk[2] = linear_to_srgb_u8(table[chunk[2] as usize] * af);
        }
    }
}

// ── Vertex layout ─────────────────────────────────────────────────────────────

/// Build 4 vertices (TL, TR, BL, BR) for a TriangleStrip unit quad.
///
/// Positions are always the full NDC quad `(-1,-1)` to `(1,1)`. The transform
/// uniform (scale + translate) handles letterboxing, zoom, and pan.
///
/// `rotation` is in 90° CW steps (0 = normal, 1 = 90°CW, 2 = 180°, 3 = 270°CW).
/// UVs are remapped so the correct corner of the source texture lands at each
/// screen corner, producing the visual rotation effect without changing the shader.
///
/// UV derivation (TL, TR, BL, BR of on-screen quad → source UV):
///   0°:   (0,0) (1,0) (0,1) (1,1)  — identity
///   90°CW:(0,1) (0,0) (1,1) (1,0)  — original BL→screen-TL
///   180°: (1,1) (0,1) (1,0) (0,0)
///   270°CW(1,0) (1,1) (0,0) (0,1)  — original TR→screen-TL
fn quad_vertices(rotation: u32) -> [Vertex; 4] {
    let uvs: [[f32; 2]; 4] = match rotation % 4 {
        0 => [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        1 => [[0.0, 1.0], [0.0, 0.0], [1.0, 1.0], [1.0, 0.0]],
        2 => [[1.0, 1.0], [0.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        3 => [[1.0, 0.0], [1.0, 1.0], [0.0, 0.0], [0.0, 1.0]],
        _ => unreachable!(),
    };
    [
        Vertex { position: [-1.0,  1.0], uv: uvs[0] },
        Vertex { position: [ 1.0,  1.0], uv: uvs[1] },
        Vertex { position: [-1.0, -1.0], uv: uvs[2] },
        Vertex { position: [ 1.0, -1.0], uv: uvs[3] },
    ]
}

// ── Viewport ─────────────────────────────────────────────────────────────────

/// Maximum zoom factor (8× the fit-to-window size).
pub const ZOOM_MAX: f32 = 8.0;

/// Zoom/pan transform state and its GPU uniform buffer.
///
/// Owns the single affine transform applied in the vertex shader:
/// `clip_pos = position * scale + translate`, where scale encodes both the
/// letterbox fit and zoom, and translate encodes pan.
///
/// The `Renderer` keeps this in sync by calling `update_image_size` and
/// `update_surface_dims` when the image or window size change.
pub struct Viewport {
    zoom: f32,
    pan:  (f32, f32),
    /// Effective (rotation-aware) image dimensions, or None if no image loaded.
    image_size:   Option<(u32, u32)>,
    surface_dims: (u32, u32),
    /// GPU uniform buffer + bind group (shader group 1).
    transform_buf:        wgpu::Buffer,
    transform_bind_group: wgpu::BindGroup,
    gpu: Arc<GpuContext>,
}

impl Viewport {
    fn new(
        gpu: Arc<GpuContext>,
        transform_buf: wgpu::Buffer,
        transform_bind_group: wgpu::BindGroup,
        surface_dims: (u32, u32),
    ) -> Self {
        Self {
            zoom: 1.0,
            pan:  (0.0, 0.0),
            image_size:   None,
            surface_dims,
            transform_buf,
            transform_bind_group,
            gpu,
        }
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Returns true when zoom is active (> 1.0), meaning the image overflows
    /// the window and arrow keys should pan rather than navigate.
    pub fn is_zoomed(&self) -> bool {
        self.zoom > 1.0
    }

    /// Current zoom level (read-only). Prefer intent methods (`zoom_in`,
    /// `set_zoom`, etc.) over reading this and computing externally.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Minimum zoom: the level at which the image occupies 5% of its native
    /// pixel dimensions. Capped at 1.0 so it never forces zoom above fit-to-window.
    pub fn min_zoom(&self) -> f32 {
        let Some(fs) = self.fit_scale() else { return 1.0 };
        (0.05 / fs).min(1.0)
    }

    /// Current display scale relative to the image's native pixel size.
    /// 1.0 = native, 0.5 = half size, 2.0 = double size.
    pub fn scale(&self) -> f32 {
        let fs = self.fit_scale().unwrap_or(1.0);
        self.zoom * fs
    }

    /// Bind group for shader group 1 (used by `Renderer::render`).
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.transform_bind_group
    }

    // ── Mutations ─────────────────────────────────────────────────────────────

    /// Set zoom level, clamped to [min_zoom(), ZOOM_MAX]. Pan is re-clamped to
    /// the new bounds, keeping the image corner at the window corner on zoom-out.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(self.min_zoom(), ZOOM_MAX);
        self.clamp_pan();
        self.write_transform();
    }

    /// Zoom in by one multiplicative step (e.g. `step = 1.25`).
    pub fn zoom_in(&mut self, step: f32) {
        self.set_zoom(self.zoom * step);
    }

    /// Zoom out by one multiplicative step (e.g. `step = 1.25`).
    pub fn zoom_out(&mut self, step: f32) {
        self.set_zoom(self.zoom / step);
    }

    /// Adjust pan by (dx, dy) in NDC, clamped so the image doesn't leave the screen.
    pub fn adjust_pan(&mut self, dx: f32, dy: f32) {
        self.pan.0 += dx;
        self.pan.1 += dy;
        self.clamp_pan();
        self.write_transform();
    }

    /// Reset zoom to 1.0 and pan to (0, 0).
    pub fn reset_zoom(&mut self) {
        self.zoom = 1.0;
        self.pan  = (0.0, 0.0);
        self.write_transform();
    }

    // ── Update methods (called by Renderer) ───────────────────────────────────

    /// Called when the loaded image changes (or is cleared).
    /// Receives the effective (rotation-aware) image dimensions.
    pub fn update_image_size(&mut self, size: Option<(u32, u32)>) {
        self.image_size = size;
        self.clamp_pan();
        self.write_transform();
    }

    /// Called when the surface is reconfigured (window resize).
    pub fn update_surface_dims(&mut self, w: u32, h: u32) {
        self.surface_dims = (w, h);
        self.clamp_pan();
        self.write_transform();
    }

    // ── Private ───────────────────────────────────────────────────────────────

    /// Scale at zoom=1: the ratio of displayed pixels to native image pixels.
    /// This is the letterbox fit scale — `min(surface_w/img_w, surface_h/img_h)`.
    fn fit_scale(&self) -> Option<f32> {
        let (iw, ih) = self.image_size?;
        let (sw, sh) = self.surface_dims;
        if iw == 0 || ih == 0 || sw == 0 || sh == 0 { return None; }
        Some((sw as f32 / iw as f32).min(sh as f32 / ih as f32))
    }

    /// NDC half-extents of the image quad at the current zoom level.
    /// Encodes letterbox + zoom into separate X/Y scales.
    fn ndc_scale(&self) -> (f32, f32) {
        let Some((iw, ih)) = self.image_size else { return (1.0, 1.0) };
        let (sw, sh) = self.surface_dims;
        if iw == 0 || ih == 0 || sw == 0 || sh == 0 { return (1.0, 1.0); }
        let fs = (sw as f32 / iw as f32).min(sh as f32 / ih as f32);
        let sx = (iw as f32 * fs / sw as f32) * self.zoom;
        let sy = (ih as f32 * fs / sh as f32) * self.zoom;
        (sx, sy)
    }

    /// Clamp pan so the image edges never cross the window edges.
    fn clamp_pan(&mut self) {
        let (sx, sy) = self.ndc_scale();
        let max_x = (sx - 1.0).max(0.0);
        let max_y = (sy - 1.0).max(0.0);
        self.pan.0 = self.pan.0.clamp(-max_x, max_x);
        self.pan.1 = self.pan.1.clamp(-max_y, max_y);
    }

    /// Write the combined transform (letterbox + zoom + pan) to the GPU uniform.
    fn write_transform(&self) {
        let (sx, sy) = self.ndc_scale();
        self.gpu.queue.write_buffer(
            &self.transform_buf,
            0,
            bytemuck::bytes_of(&TransformUniform {
                scale_x:     sx,
                scale_y:     sy,
                translate_x: self.pan.0,
                translate_y: self.pan.1,
            }),
        );
    }
}

// ── Renderer ──────────────────────────────────────────────────────────────────

pub struct Renderer {
    // Drop order: GPU resources must drop before `gpu` (Device/Queue).
    // Rust drops fields in declaration order.
    image_state:    Option<ImageState>,
    egui_renderer:  egui_wgpu::Renderer,
    pipeline:       wgpu::RenderPipeline,
    bind_group_layout:           wgpu::BindGroupLayout,
    #[allow(dead_code)]
    transform_bind_group_layout: wgpu::BindGroupLayout,
    pub viewport:   Viewport,
    surface:        wgpu::Surface<'static>,
    config:         wgpu::SurfaceConfiguration,
    gpu:            Arc<GpuContext>,

    pub display_mode:  DisplayMode,
    pub fit_to_image:  bool,
    pub image_size:    Option<(u32, u32)>,
    /// 0 = 0°, 1 = 90°CW, 2 = 180°, 3 = 270°CW
    pub rotation:      u32,
    screen_size:       (u32, u32),
}

struct ImageState {
    bind_group: wgpu::BindGroup,
    vertex_buf: wgpu::Buffer,
}

impl Renderer {
    /// Returns the GPU context (shared with other windows) alongside Self.
    pub fn new(window: Arc<Window>) -> Result<(Self, Arc<GpuContext>)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone())
            .context("create surface")?;

        let adapter = pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference:       wgpu::PowerPreference::default(),
                compatible_surface:     Some(&surface),
                force_fallback_adapter: false,
            },
        ))
        .context("no suitable GPU adapter")?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label:             None,
                required_features: wgpu::Features::empty(),
                required_limits:   wgpu::Limits::default(),
                memory_hints:      wgpu::MemoryHints::default(),
            },
            None,
        ))
        .context("request device")?;

        let size   = window.inner_size();
        let caps   = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

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
        surface.configure(&device, &config);

        let gpu = Arc::new(GpuContext { instance, adapter, device, queue });

        // ── Image bind group layout ───────────────────────────────────────────

        let bind_group_layout = gpu.device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label:   Some("image_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding:    0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled:   false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding:    1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(
                            wgpu::SamplerBindingType::Filtering,
                        ),
                        count: None,
                    },
                ],
            },
        );

        // ── Transform uniform (group 1) — owned by Viewport ──────────────────

        let transform_bind_group_layout = gpu.device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label:   Some("transform_bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding:    0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty:                 wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size:   None,
                    },
                    count: None,
                }],
            },
        );

        let transform_buf = gpu.device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label:    Some("transform_buf"),
                contents: bytemuck::bytes_of(&TransformUniform::identity()),
                usage:    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            },
        );

        let transform_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("transform_bg"),
            layout:  &transform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding:  0,
                resource: transform_buf.as_entire_binding(),
            }],
        });

        let viewport = Viewport::new(
            gpu.clone(),
            transform_buf,
            transform_bind_group,
            (config.width, config.height),
        );

        // ── Image render pipeline ─────────────────────────────────────────────

        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("image_shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("shaders/image.wgsl").into(),
            ),
        });

        let pipeline_layout =
            gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label:                Some("image_layout"),
                bind_group_layouts:   &[&bind_group_layout, &transform_bind_group_layout],
                push_constant_ranges: &[],
            });

        let pipeline =
            gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label:  Some("image_pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module:              &shader,
                    entry_point:         "vs_main",
                    buffers:             &[Vertex::LAYOUT],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module:              &shader,
                    entry_point:         "fs_main",
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend:      Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample:   wgpu::MultisampleState::default(),
                multiview:     None,
                cache:         None,
            });

        // ── egui renderer ─────────────────────────────────────────────────────

        // 5th arg = dithering (added in wgpu 22 / egui-wgpu 0.29)
        let egui_renderer = egui_wgpu::Renderer::new(&gpu.device, format, None, 1, false);

        let screen_size = window
            .current_monitor()
            .map(|m| { let s = m.size(); (s.width, s.height) })
            .unwrap_or((1920, 1080));

        let renderer = Self {
            surface,
            gpu: gpu.clone(),
            config,
            pipeline,
            bind_group_layout,
            transform_bind_group_layout,
            viewport,
            egui_renderer,
            image_state:    None,
            display_mode:   DisplayMode::Window,
            fit_to_image:   true,
            image_size:     None,
            rotation:       0,
            screen_size,
        };
        Ok((renderer, gpu))
    }

    // ── Public controls ───────────────────────────────────────────────────────

    pub fn set_image(&mut self, image: &mut DecodedImage) {
        self.image_size = Some((image.width, image.height));
        self.viewport.update_image_size(self.effective_image_size());

        // Premultiply alpha in linear space for correct sRGB blending.
        premultiply_alpha(&mut image.pixels);

        let texture = self.gpu.device.create_texture_with_data(
            &self.gpu.queue,
            &wgpu::TextureDescriptor {
                label:            Some("image_texture"),
                size:             wgpu::Extent3d {
                    width: image.width, height: image.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count:  1,
                sample_count:     1,
                dimension:        wgpu::TextureDimension::D2,
                format:           wgpu::TextureFormat::Rgba8UnormSrgb,
                usage:            wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::COPY_DST,
                view_formats:     &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &image.pixels,
        );

        let view    = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label:           Some("image_sampler"),
            address_mode_u:  wgpu::AddressMode::ClampToEdge,
            address_mode_v:  wgpu::AddressMode::ClampToEdge,
            mag_filter:      wgpu::FilterMode::Linear,
            min_filter:      wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("image_bg"),
            layout:  &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding:  0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding:  1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let vertex_buf = self.gpu.device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label:    Some("vertex_buf"),
                contents: bytemuck::cast_slice(&quad_vertices(self.rotation)),
                usage:    wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            },
        );

        self.image_state = Some(ImageState { bind_group, vertex_buf });
    }

    /// Drop the current image so the renderer draws only the clear colour.
    pub fn clear_image(&mut self) {
        self.image_state = None;
        self.image_size  = None;
        self.rotation    = 0;
        self.viewport.update_image_size(None);
    }

    /// Rotate by one 90° step. `clockwise = true` → +90°CW.
    pub fn rotate(&mut self, clockwise: bool) {
        self.rotation = if clockwise {
            (self.rotation + 1) % 4
        } else {
            (self.rotation + 3) % 4
        };
        // Rewrite vertex UVs for the new rotation
        if let Some(state) = &self.image_state {
            self.gpu.queue.write_buffer(
                &state.vertex_buf,
                0,
                bytemuck::cast_slice(&quad_vertices(self.rotation)),
            );
        }
        // Effective image dims changed (w↔h swap) — update viewport transform
        self.viewport.update_image_size(self.effective_image_size());
    }

    pub fn resize(&mut self, new_w: u32, new_h: u32) {
        if new_w == 0 || new_h == 0 { return; }
        self.config.width  = new_w;
        self.config.height = new_h;
        self.surface.configure(&self.gpu.device, &self.config);
        self.viewport.update_surface_dims(new_w, new_h);
    }

    /// Returns the window size to request for the current image + mode.
    /// `None` means don't resize (Fullscreen, or Window+fixed).
    pub fn compute_window_size(&self) -> Option<(u32, u32)> {
        let img = self.effective_image_size()?;
        compute_window_request(
            self.display_mode,
            img,
            self.screen_size,
            self.fit_to_image,
        )
    }

    /// Shared GPU context for other windows to reuse.
    pub fn gpu(&self) -> &Arc<GpuContext> {
        &self.gpu
    }

    // ── Rendering ─────────────────────────────────────────────────────────────

    pub fn render(
        &mut self,
        paint_jobs:    &[egui::ClippedPrimitive],
        tex_delta:     &egui::TexturesDelta,
        screen_desc:   &egui_wgpu::ScreenDescriptor,
    ) -> Result<()> {
        let output = match self.surface.get_current_texture() {
            Ok(t)                           => t,
            Err(wgpu::SurfaceError::Outdated) => return Ok(()),
            Err(e)                          => return Err(e.into()),
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Upload egui texture changes
        for (id, delta) in &tex_delta.set {
            self.egui_renderer.update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
        }
        for id in &tex_delta.free {
            self.egui_renderer.free_texture(id);
        }

        let mut encoder = self.gpu.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("render_encoder") },
        );

        // Upload egui vertex/index buffers into the encoder
        self.egui_renderer.update_buffers(
            &self.gpu.device, &self.gpu.queue, &mut encoder, paint_jobs, screen_desc,
        );

        {
            // forget_lifetime() converts RenderPass<'encoder> → RenderPass<'static>,
            // which is required by egui-wgpu 0.29 / wgpu 22.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
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

            // 1. image quad
            if let Some(state) = &self.image_state {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &state.bind_group, &[]);
                pass.set_bind_group(1, self.viewport.bind_group(), &[]);
                pass.set_vertex_buffer(0, state.vertex_buf.slice(..));
                pass.draw(0..4, 0..1);
            }

            // 2. egui overlay on top
            self.egui_renderer.render(&mut pass, paint_jobs, screen_desc);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Image dimensions accounting for 90°/270° rotation (swaps w↔h).
    fn effective_image_size(&self) -> Option<(u32, u32)> {
        let (w, h) = self.image_size?;
        if self.rotation % 2 == 1 { Some((h, w)) } else { Some((w, h)) }
    }
}
