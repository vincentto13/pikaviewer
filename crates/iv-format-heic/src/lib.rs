use iv_core::format::{DecodedImage, FormatDescriptor, FormatError, FormatPlugin};

pub struct HeicPlugin;

impl FormatPlugin for HeicPlugin {
    fn descriptor(&self) -> &FormatDescriptor {
        &FormatDescriptor {
            name:       "libheif",
            extensions: &["heic", "heif", "avif"],
        }
    }

    fn decode(&self, data: &[u8]) -> Result<DecodedImage, FormatError> {
        decode_impl(data)
    }
}

// ── Real implementation (requires libheif) ────────────────────────────────────

#[cfg(feature = "libheif-rs")]
fn decode_impl(data: &[u8]) -> Result<DecodedImage, FormatError> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

    let ctx = HeifContext::read_from_bytes(data)
        .map_err(|e| FormatError(e.to_string()))?;

    let handle = ctx.primary_image_handle()
        .map_err(|e| FormatError(e.to_string()))?;

    let width  = handle.width();
    let height = handle.height();

    let lib_heif = LibHeif::new();
    let image = lib_heif
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
        .map_err(|e| FormatError(e.to_string()))?;

    let planes = image.planes();
    let plane  = planes.interleaved
        .ok_or_else(|| FormatError("libheif: no interleaved RGBA plane".into()))?;

    // The plane may have per-row padding (stride > width * 4). Strip it.
    let row_bytes = width as usize * 4;
    let pixels = if plane.stride == row_bytes {
        plane.data.to_vec()
    } else {
        let mut out = Vec::with_capacity(row_bytes * height as usize);
        for row in 0..height as usize {
            let start = row * plane.stride;
            out.extend_from_slice(&plane.data[start..start + row_bytes]);
        }
        out
    };

    let has_alpha = handle.has_alpha_channel();
    Ok(DecodedImage { pixels, width, height, has_alpha })
}

// ── Stub (crate present in graph but libheif not available) ───────────────────

#[cfg(not(feature = "libheif-rs"))]
fn decode_impl(_data: &[u8]) -> Result<DecodedImage, FormatError> {
    Err(FormatError(
        "HEIC support requires libheif — rebuild with --features iv-app/heic".into(),
    ))
}
