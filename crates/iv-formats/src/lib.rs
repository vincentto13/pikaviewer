use image::GenericImageView;
use iv_core::format::{
    DecodedImage, FormatDescriptor, FormatError, FormatPlugin, PluginRegistry,
};

// ── image-rs plugin ──────────────────────────────────────────────────────────

pub struct ImageRsPlugin;

impl FormatPlugin for ImageRsPlugin {
    fn descriptor(&self) -> &FormatDescriptor {
        &FormatDescriptor {
            name: "image-rs",
            extensions: &[
                "jpg", "jpeg", "png", "gif", "bmp",
                "tiff", "tif", "webp", "ico", "qoi",
            ],
        }
    }

    fn decode(&self, data: &[u8]) -> Result<DecodedImage, FormatError> {
        let img = image::load_from_memory(data)
            .map_err(|e| FormatError(e.to_string()))?;
        let has_alpha = img.color().has_alpha();
        let (width, height) = img.dimensions();
        let pixels = img.into_rgba8().into_raw();
        Ok(DecodedImage { pixels, width, height, has_alpha })
    }
}

// ── Registry builder ─────────────────────────────────────────────────────────

/// Create a registry with all built-in plugins registered.
pub fn default_registry() -> PluginRegistry {
    let mut r = PluginRegistry::new();
    r.register(ImageRsPlugin);
    r
}
