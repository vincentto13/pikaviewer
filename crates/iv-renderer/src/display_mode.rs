/// How the application window and image are sized relative to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// Window auto-resizes to the image's native dimensions (capped at screen).
    Regular,
    /// Window enters OS fullscreen; image is letterboxed to fit.
    Fullscreen,
    /// Window stays at a fixed user-configured size; image is letterboxed.
    FixedSize { width: u32, height: u32 },
}

impl DisplayMode {
    /// Cycle to the next mode in order.
    pub fn next(self) -> Self {
        match self {
            Self::Regular                 => Self::Fullscreen,
            Self::Fullscreen              => Self::FixedSize { width: 1280, height: 720 },
            Self::FixedSize { .. }        => Self::Regular,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Regular         => "Regular",
            Self::Fullscreen      => "Fullscreen",
            Self::FixedSize { .. } => "Fixed",
        }
    }
}

/// Compute what the window size should be and where the image quad sits (NDC).
pub struct Layout {
    /// Desired window size. `None` = don't resize (FixedSize/Fullscreen handled
    /// by winit fullscreen API; FixedSize is set at creation time).
    pub request_size: Option<(u32, u32)>,
    /// Image quad in NDC (clip space): left, right, bottom, top.
    /// x: -1 = left edge, +1 = right edge
    /// y: -1 = bottom edge, +1 = top edge
    pub quad: QuadNdc,
}

#[derive(Clone, Copy)]
pub struct QuadNdc {
    pub left:   f32,
    pub right:  f32,
    pub bottom: f32,
    pub top:    f32,
}

impl QuadNdc {
    pub fn full_screen() -> Self {
        Self { left: -1.0, right: 1.0, bottom: -1.0, top: 1.0 }
    }
}

/// `screen_size` is the monitor's usable size (used only in Regular mode to cap
/// the window).
pub fn compute_layout(
    mode: DisplayMode,
    image_size: (u32, u32),
    window_size: (u32, u32),
    screen_size: (u32, u32),
) -> Layout {
    match mode {
        DisplayMode::Regular => {
            // Ask the window to be the image's native size, capped at screen.
            let w = image_size.0.min(screen_size.0);
            let h = image_size.1.min(screen_size.1);
            Layout {
                request_size: Some((w, h)),
                quad: letterbox(image_size, (w, h)),
            }
        }
        DisplayMode::Fullscreen | DisplayMode::FixedSize { .. } => {
            // Window size is controlled externally (OS fullscreen or fixed);
            // just letterbox the image into whatever size the window currently is.
            Layout {
                request_size: None,
                quad: letterbox(image_size, window_size),
            }
        }
    }
}

/// Fit `image_size` inside `window_size` preserving aspect ratio, centered.
/// Returns the quad in NDC.
fn letterbox(image_size: (u32, u32), window_size: (u32, u32)) -> QuadNdc {
    let (iw, ih) = (image_size.0 as f32, image_size.1 as f32);
    let (ww, wh) = (window_size.0 as f32, window_size.1 as f32);

    if ww == 0.0 || wh == 0.0 { return QuadNdc::full_screen(); }

    let scale   = (ww / iw).min(wh / ih);
    let draw_w  = iw * scale;
    let draw_h  = ih * scale;

    // Pixel-space corners (Y down)
    let px0 = (ww - draw_w) / 2.0;
    let py0 = (wh - draw_h) / 2.0;
    let px1 = px0 + draw_w;
    let py1 = py0 + draw_h;

    // Convert to NDC: x = px/ww*2-1, y = 1-py/wh*2 (flip Y)
    QuadNdc {
        left:   px0 / ww * 2.0 - 1.0,
        right:  px1 / ww * 2.0 - 1.0,
        bottom: 1.0 - py1 / wh * 2.0,
        top:    1.0 - py0 / wh * 2.0,
    }
}
