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

/// Returns the window size to request for the given image and mode.
/// `None` means don't resize (Fullscreen, or fixed-size window mode).
///
/// `screen_size` is the monitor's usable size (used only in Window+fit_to_image
/// mode to cap the window).
pub fn compute_window_request(
    mode: DisplayMode,
    image_size: (u32, u32),
    screen_size: (u32, u32),
    fit_to_image: bool,
) -> Option<(u32, u32)> {
    match mode {
        DisplayMode::Window if fit_to_image => {
            let w = image_size.0.min(screen_size.0);
            let h = image_size.1.min(screen_size.1);
            Some((w, h))
        }
        _ => None,
    }
}
