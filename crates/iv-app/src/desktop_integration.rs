//! Desktop integration: .desktop file management (Linux) and
//! Launch Services default handler (macOS).

/// MIME types for all image formats we support.
const MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/bmp",
    "image/tiff",
    "image/webp",
    "image/x-icon",
    "image/vnd.microsoft.icon",
    "image/heic",
    "image/heif",
    "image/avif",
    #[cfg(feature = "raw")] "image/x-nikon-nef",
    #[cfg(feature = "raw")] "image/x-nikon-nrw",
    #[cfg(feature = "raw")] "image/x-canon-cr2",
    #[cfg(feature = "raw")] "image/x-canon-cr3",
    #[cfg(feature = "raw")] "image/x-canon-crw",
    #[cfg(feature = "raw")] "image/x-sony-arw",
    #[cfg(feature = "raw")] "image/x-sony-sr2",
    #[cfg(feature = "raw")] "image/x-sony-srf",
    #[cfg(feature = "raw")] "image/x-fuji-raf",
    #[cfg(feature = "raw")] "image/x-olympus-orf",
    #[cfg(feature = "raw")] "image/x-panasonic-rw2",
    #[cfg(feature = "raw")] "image/x-pentax-pef",
    #[cfg(feature = "raw")] "image/x-adobe-dng",
];

// ── Linux ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use super::MIME_TYPES;
    use std::path::PathBuf;
    use std::process::Command;

    const DESKTOP_FILENAME: &str = "pikaviewer.desktop";
    const ICON_FILENAME: &str = "pikaviewer.png";

    /// The icon PNG is embedded at compile time from assets/icon.png.
    const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon.png");

    fn applications_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("applications")
    }

    fn icons_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("icons")
    }

    fn desktop_file_path() -> PathBuf {
        applications_dir().join(DESKTOP_FILENAME)
    }

    fn icon_file_path() -> PathBuf {
        icons_dir().join(ICON_FILENAME)
    }

    fn binary_path() -> String {
        std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from("pikaviewer"))
            .to_string_lossy()
            .into_owned()
    }

    fn desktop_file_contents(bin: &str, icon_path: &str) -> String {
        let mime_list = MIME_TYPES.join(";");
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=PikaViewer\n\
             Comment=A fast, cross-platform image viewer\n\
             Exec={bin} %f\n\
             Icon={icon_path}\n\
             Terminal=false\n\
             Categories=Graphics;Viewer;\n\
             MimeType={mime_list};\n"
        )
    }

    fn install_icon() -> anyhow::Result<String> {
        let dir = icons_dir();
        std::fs::create_dir_all(&dir)?;
        let path = icon_file_path();
        std::fs::write(&path, ICON_PNG)?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Install .desktop file and set as default for all supported MIME types.
    pub fn install() -> anyhow::Result<()> {
        let dir = applications_dir();
        std::fs::create_dir_all(&dir)?;

        let icon_path = install_icon()?;
        eprintln!("Installed icon to {icon_path}");

        let path = desktop_file_path();
        let bin = binary_path();
        std::fs::write(&path, desktop_file_contents(&bin, &icon_path))?;
        eprintln!("Installed {}", path.display());

        // Update desktop database
        let _ = Command::new("update-desktop-database")
            .arg(&dir)
            .status();

        set_default()?;

        eprintln!("\nPikaViewer is now the default image viewer.");
        eprintln!("To undo: {bin} --uninstall");
        Ok(())
    }

    /// Remove .desktop file, icon, and MIME associations.
    pub fn uninstall() -> anyhow::Result<()> {
        let path = desktop_file_path();
        if path.exists() {
            std::fs::remove_file(&path)?;
            eprintln!("Removed {}", path.display());
        } else {
            eprintln!("No .desktop file found at {}", path.display());
        }

        let icon = icon_file_path();
        if icon.exists() {
            std::fs::remove_file(&icon)?;
            eprintln!("Removed {}", icon.display());
        }

        let dir = applications_dir();
        let _ = Command::new("update-desktop-database")
            .arg(&dir)
            .status();

        eprintln!("Uninstalled. MIME defaults may need to be reassigned manually.");
        Ok(())
    }

    /// Set `PikaViewer` as the default handler for all supported MIME types.
    pub fn set_default() -> anyhow::Result<()> {
        // Ensure .desktop file + icon exist
        let path = desktop_file_path();
        if !path.exists() {
            let icon_path = install_icon()?;
            let bin = binary_path();
            let dir = applications_dir();
            std::fs::create_dir_all(&dir)?;
            std::fs::write(&path, desktop_file_contents(&bin, &icon_path))?;
        }

        for mime in MIME_TYPES {
            let status = Command::new("xdg-mime")
                .args(["default", DESKTOP_FILENAME, mime])
                .status();
            match status {
                Ok(s) if s.success() => {}
                Ok(s) => log::warn!("xdg-mime default for {mime} exited with {s}"),
                Err(e) => {
                    anyhow::bail!("failed to run xdg-mime: {e}");
                }
            }
        }
        Ok(())
    }
}

// ── macOS ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use super::MIME_TYPES;
    use std::ffi::c_void;

    const BUNDLE_ID: &str = "xyz.astrolabius.pikaviewer";

    // Core Foundation types (opaque pointers)
    type CFStringRef = *const c_void;
    type CFAllocatorRef = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFAllocatorDefault: CFAllocatorRef;
        fn CFStringCreateWithBytes(
            alloc: CFAllocatorRef,
            bytes: *const u8,
            num_bytes: isize,
            encoding: u32,
            is_external: u8,
        ) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn LSSetDefaultRoleHandlerForContentType(
            content_type: CFStringRef,
            role: u32,
            handler_bundle_id: CFStringRef,
        ) -> i32;
    }

    // kCFStringEncodingUTF8 = 0x08000100
    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    // kLSRolesAll = 0xFFFFFFFF
    const K_LS_ROLES_ALL: u32 = 0xFFFF_FFFF;

    /// Create a CFStringRef from a Rust &str. Caller must CFRelease it.
    unsafe fn cfstr(s: &str) -> CFStringRef {
        CFStringCreateWithBytes(
            kCFAllocatorDefault,
            s.as_ptr(),
            s.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
            0,
        )
    }

    /// Map MIME types to macOS UTI content types.
    fn uti_for_mime(mime: &str) -> Option<&'static str> {
        match mime {
            "image/jpeg"                 => Some("public.jpeg"),
            "image/png"                  => Some("public.png"),
            "image/gif"                  => Some("com.compuserve.gif"),
            "image/bmp"                  => Some("com.microsoft.bmp"),
            "image/tiff"                 => Some("public.tiff"),
            "image/webp"                 => Some("org.webmproject.webp"),
            "image/x-icon"
            | "image/vnd.microsoft.icon" => Some("com.microsoft.ico"),
            "image/heic"                 => Some("public.heic"),
            "image/heif"                 => Some("public.heif"),
            "image/avif"                 => Some("public.avif"),
            "image/x-nikon-nef"          => Some("com.nikon.raw-image"),
            "image/x-nikon-nrw"          => Some("com.nikon.nrw-raw-image"),
            "image/x-canon-cr2"          => Some("com.canon.cr2-raw-image"),
            "image/x-canon-cr3"          => Some("com.canon.cr3-raw-image"),
            "image/x-canon-crw"          => Some("com.canon.crw-raw-image"),
            "image/x-sony-arw"           => Some("com.sony.arw-raw-image"),
            "image/x-sony-sr2"           => Some("com.sony.sr2-raw-image"),
            "image/x-sony-srf"           => Some("com.sony.raw-image"),
            "image/x-fuji-raf"           => Some("com.fuji.raw-image"),
            "image/x-olympus-orf"        => Some("com.olympus.raw-image"),
            "image/x-panasonic-rw2"      => Some("com.panasonic.raw-image"),
            "image/x-pentax-pef"         => Some("com.pentax.raw-image"),
            "image/x-adobe-dng"          => Some("com.adobe.raw-image"),
            _                            => None,
        }
    }

    /// Set `PikaViewer` as the default handler for all supported image UTIs.
    pub fn set_default() {
        unsafe {
            let bundle_cf = cfstr(BUNDLE_ID);

            for mime in MIME_TYPES {
                let Some(uti) = uti_for_mime(mime) else { continue };
                let uti_cf = cfstr(uti);

                let status = LSSetDefaultRoleHandlerForContentType(
                    uti_cf, K_LS_ROLES_ALL, bundle_cf,
                );
                if status != 0 {
                    log::warn!(
                        "LSSetDefaultRoleHandlerForContentType({uti}) returned {status}"
                    );
                }

                CFRelease(uti_cf);
            }

            CFRelease(bundle_cf);
        }
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub use linux::{install, uninstall};

/// Set `PikaViewer` as the default image viewer for the current platform.
#[allow(clippy::unnecessary_wraps)] // Returns Result on Linux/other, trivial on macOS
pub fn set_default() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    { linux::set_default() }

    #[cfg(target_os = "macos")]
    { macos::set_default(); Ok(()) }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    { anyhow::bail!("set-as-default is not supported on this platform") }
}
