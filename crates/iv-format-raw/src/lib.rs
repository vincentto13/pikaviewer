use iv_core::format::{DecodedImage, FormatDescriptor, FormatError, FormatPlugin};

pub struct RawPlugin;

impl FormatPlugin for RawPlugin {
    fn descriptor(&self) -> &FormatDescriptor {
        &FormatDescriptor {
            name: "libraw",
            extensions: &[
                "nef", "nrw",          // Nikon
                "cr2", "cr3", "crw",   // Canon
                "arw", "sr2", "srf",   // Sony
                "raf",                 // Fujifilm
                "orf",                 // Olympus
                "rw2",                 // Panasonic / Leica
                "pef",                 // Pentax
                "dng",                 // Adobe / universal
                "raw",                 // generic
            ],
        }
    }

    fn decode(&self, data: &[u8]) -> Result<DecodedImage, FormatError> {
        decode_impl(data)
    }
}

// ── Real implementation (requires vendored LibRaw via rsraw) ────────────────

#[cfg(feature = "rsraw")]
fn decode_impl(data: &[u8]) -> Result<DecodedImage, FormatError> {
    use rsraw::{ImageFormat, RawImage, BIT_DEPTH_8};

    let mut raw = RawImage::open(data).map_err(|e| FormatError(e.to_string()))?;

    // Disable LibRaw's auto-rotation. rsraw 0.1 has no safe setter for
    // user_flip, so we reach through the AsMut impl; the app's EXIF pipeline
    // handles orientation uniformly across formats.
    raw.as_mut().params.user_flip = 0;

    raw.set_use_camera_wb(true);
    raw.unpack().map_err(|e| FormatError(e.to_string()))?;

    let processed = raw
        .process::<BIT_DEPTH_8>()
        .map_err(|e| FormatError(e.to_string()))?;

    if processed.image_format() != ImageFormat::Bitmap {
        return Err(FormatError(
            "libraw returned a non-bitmap image (unexpected embedded JPEG path)".into(),
        ));
    }
    if processed.colors() != 3 || processed.bits() != 8 {
        return Err(FormatError(format!(
            "libraw returned unexpected format: colors={}, bits={}",
            processed.colors(),
            processed.bits()
        )));
    }

    let width = processed.width();
    let height = processed.height();
    let rgb: &[u8] = &processed;

    let expected = width as usize * height as usize * 3;
    if rgb.len() < expected {
        return Err(FormatError(format!(
            "libraw pixel buffer too small: got {}, expected {}",
            rgb.len(),
            expected
        )));
    }

    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for px in rgb[..expected].chunks_exact(3) {
        pixels.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }

    Ok(DecodedImage {
        pixels,
        width,
        height,
        has_alpha: false,
    })
}

// ── Stub (crate present in graph but rsraw not enabled) ─────────────────────

#[cfg(not(feature = "rsraw"))]
fn decode_impl(_data: &[u8]) -> Result<DecodedImage, FormatError> {
    Err(FormatError(
        "RAW support requires LibRaw — rebuild with --features iv-app/raw".into(),
    ))
}
