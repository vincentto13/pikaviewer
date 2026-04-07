use std::fmt;

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
    pub fn new() -> Self { Self::default() }

    pub fn register(&mut self, plugin: impl FormatPlugin + 'static) {
        self.plugins.push(Box::new(plugin));
    }

    pub fn find_for_extension(&self, ext: &str) -> Option<&dyn FormatPlugin> {
        self.plugins
            .iter()
            .find(|p| p.supports_extension(ext))
            .map(|p| p.as_ref())
    }

    pub fn supported_extensions(&self) -> Vec<&str> {
        self.plugins
            .iter()
            .flat_map(|p| p.descriptor().extensions.iter().copied())
            .collect()
    }
}
