/// How the application window and image are sized relative to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// Window mode — behavior depends on `fit_to_image` setting:
    /// - fit_to_image=true:  window resizes to image (min 400×300, capped at screen)
    /// - fit_to_image=false: window stays at a fixed size, image letterboxed
    Window,
    /// OS borderless fullscreen; image is letterboxed to fit.
    Fullscreen,
}

impl DisplayMode {
    /// Toggle between Window and Fullscreen.
    pub fn next(self) -> Self {
        match self {
            Self::Window     => Self::Fullscreen,
            Self::Fullscreen => Self::Window,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Window     => "Window",
            Self::Fullscreen => "Fullscreen",
        }
    }
}

/// Compute what the window size should be and where the image quad sits (NDC).
pub struct Layout {
    /// Desired window size. `None` = don't resize (Fullscreen handled by winit,
    /// or fixed-size window mode where we keep the current size).
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

/// `screen_size` is the monitor's usable size (used only in Window+fit_to_image
/// mode to cap the window).
pub fn compute_layout(
    mode: DisplayMode,
    image_size: (u32, u32),
    window_size: (u32, u32),
    screen_size: (u32, u32),
    fit_to_image: bool,
) -> Layout {
    match mode {
        DisplayMode::Window if fit_to_image => {
            // Ask the window to be the image's native size, capped at screen.
            let w = image_size.0.min(screen_size.0);
            let h = image_size.1.min(screen_size.1);
            // Letterbox into the *actual* window size, not the requested size.
            // They differ when a minimum window size is enforced: the window
            // ends up larger than the image, so the image must be centered.
            Layout {
                request_size: Some((w, h)),
                quad: letterbox(image_size, window_size),
            }
        }
        DisplayMode::Window => {
            // Fixed window size — don't resize, just letterbox.
            Layout {
                request_size: None,
                quad: letterbox(image_size, window_size),
            }
        }
        DisplayMode::Fullscreen => {
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
