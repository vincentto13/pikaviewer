use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

// ── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct FormatError(pub String);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for FormatError {}

impl From<String> for FormatError {
    fn from(s: String) -> Self { Self(s) }
}

// ── Decoded output ───────────────────────────────────────────────────────────

/// Raw RGBA8 pixel data produced by a format plugin.
pub struct DecodedImage {
    /// RGBA8, row-major, top-to-bottom.
    pub pixels: Vec<u8>,
    pub width:  u32,
    pub height: u32,
    /// True if the image has any non-opaque pixels. Set by format plugins.
    /// When false, `premultiply_alpha()` is a no-op.
    pub has_alpha: bool,
}

// ── Premultiplied alpha ─────────────────────────────────────────────────────

/// sRGB → linear lookup table (256 entries), built once.
static SRGB_TO_LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| {
    let mut table = [0.0f32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let s = i as f32 / 255.0;
        *entry = if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        };
    }
    table
});

fn linear_to_srgb_u8(l: f32) -> u8 {
    #[allow(clippy::unreadable_literal)]
    let s = if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

impl DecodedImage {
    /// Premultiply alpha in linear space for correct sRGB blending.
    /// No-op if `has_alpha` is false (the common case for JPEG).
    pub fn premultiply_alpha(&mut self) {
        if !self.has_alpha { return; }
        let table = &*SRGB_TO_LINEAR;
        for chunk in self.pixels.chunks_exact_mut(4) {
            let a = chunk[3];
            if a == 0 {
                chunk[0] = 0;
                chunk[1] = 0;
                chunk[2] = 0;
            } else if a < 255 {
                let af = f32::from(a) / 255.0;
                chunk[0] = linear_to_srgb_u8(table[chunk[0] as usize] * af);
                chunk[1] = linear_to_srgb_u8(table[chunk[1] as usize] * af);
                chunk[2] = linear_to_srgb_u8(table[chunk[2] as usize] * af);
            }
        }
    }
}

// ── Plugin trait ─────────────────────────────────────────────────────────────

pub struct FormatDescriptor {
    pub name:       &'static str,
    pub extensions: &'static [&'static str],
}

pub trait FormatPlugin: Send + Sync {
    fn descriptor(&self) -> &FormatDescriptor;

    fn supports_extension(&self, ext: &str) -> bool {
        self.descriptor()
            .extensions
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext))
    }

    fn decode(&self, data: &[u8]) -> Result<DecodedImage, FormatError>;
}

// ── Registry ─────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<Box<dyn FormatPlugin>>,
}

impl PluginRegistry {
    #[must_use]
    pub fn new() -> Self { Self::default() }

    pub fn register(&mut self, plugin: impl FormatPlugin + 'static) {
        self.plugins.push(Box::new(plugin));
    }

    pub fn find_for_extension(&self, ext: &str) -> Option<&dyn FormatPlugin> {
        self.plugins
            .iter()
            .find(|p| p.supports_extension(ext))
            .map(AsRef::as_ref)
    }

    #[must_use]
    pub fn supported_extensions(&self) -> Vec<&str> {
        self.plugins
            .iter()
            .flat_map(|p| p.descriptor().extensions.iter().copied())
            .collect()
    }

    /// Whether any registered plugin can decode a file at `path`, based on
    /// its extension. Case-insensitive; returns `false` for paths with no
    /// extension or with non-UTF-8 extensions.
    #[must_use]
    pub fn supports_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| self.plugins.iter().any(|p| p.supports_extension(ext)))
    }
}
