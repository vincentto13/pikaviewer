#!/usr/bin/env bash
# package-macos.sh — Build and package PikaViewer.app for macOS (arm64)
#
# Must run on macOS. Requires: Xcode CLT (rustup, cargo, codesign, hdiutil)
# For HEIC support: brew install libheif
#
# Usage:
#   ./scripts/package-macos.sh              # arm64 binary, no DMG
#   ./scripts/package-macos.sh --dmg        # arm64 binary + DMG
#
# Intel Macs run the arm64 binary transparently through Rosetta 2.
set -euo pipefail

# ── Config ────────────────────────────────────────────────────────────────────
APP_NAME="PikaViewer"
BUNDLE_ID="xyz.astrolabius.pikaviewer"
VERSION="0.3.1"
MIN_MACOS="12.0"
BINARY_NAME="pikaviewer"
TARGET="aarch64-apple-darwin"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="$REPO_ROOT/target"
OUT_DIR="$REPO_ROOT/dist/macos"

# ── Args ──────────────────────────────────────────────────────────────────────
MAKE_DMG=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --dmg)      MAKE_DMG=true; shift ;;
        --version)  VERSION="$2"; shift 2 ;;
        *) echo "error: unknown option: $1"; exit 1 ;;
    esac
done

# ── Platform check ────────────────────────────────────────────────────────────
if [[ "$(uname)" != "Darwin" ]]; then
    echo "error: this script must run on macOS"
    exit 1
fi

# ── Feature detection ─────────────────────────────────────────────────────────
FEATURES=()

if pkg-config --atleast-version=1.18 libheif 2>/dev/null; then
    echo "==> libheif $(pkg-config --modversion libheif) found — HEIC/AVIF support enabled"
    FEATURES+=("iv-app/heic")
else
    echo "warning: libheif >= 1.18 not found — HEIC/AVIF support will be disabled"
    echo "         To enable: brew install libheif"
fi

# RAW support: rsraw-sys vendors LibRaw C++ sources, so no system libraw is
# needed. Xcode CLT ships clang + libclang for bindgen. Always enable on macOS.
echo "==> RAW support enabled (vendored LibRaw via rsraw)"
FEATURES+=("iv-app/raw")

if [[ ${#FEATURES[@]} -gt 0 ]]; then
    FEATURE_FLAG="--features $(IFS=,; echo "${FEATURES[*]}")"
else
    FEATURE_FLAG=""
fi

# Track HEIC separately for the dylib-bundling step below.
if printf '%s\n' "${FEATURES[@]}" | grep -qx 'iv-app/heic'; then
    HEIC_ENABLED=true
else
    HEIC_ENABLED=false
fi

echo "==> Building $APP_NAME $VERSION (target: $TARGET)"
cd "$REPO_ROOT"

# ── Compile ───────────────────────────────────────────────────────────────────
rustup target add "$TARGET" 2>/dev/null || true
echo "==> cargo build --release --target $TARGET $FEATURE_FLAG"
cargo build --release --target "$TARGET" $FEATURE_FLAG

FINAL_BINARY="$BUILD_DIR/$TARGET/release/$BINARY_NAME"

# ── App bundle ────────────────────────────────────────────────────────────────
APP_BUNDLE="$OUT_DIR/$APP_NAME.app"
echo "==> Creating $APP_BUNDLE"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

cp "$FINAL_BINARY" "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"

# ── Icon ─────────────────────────────────────────────────────────────────────
ICNS="$REPO_ROOT/assets/icon.icns"
if [[ ! -f "$ICNS" ]]; then
    echo "==> Generating icon.icns from icon.png"
    "$SCRIPT_DIR/make-icns.sh"
fi
cp "$ICNS" "$APP_BUNDLE/Contents/Resources/icon.icns"
echo "    Copied icon.icns"

# ── Credits.html (from Credits.md) ───────────────────────────────────────────
CREDITS_MD="$REPO_ROOT/assets/Credits.md"
if [[ -f "$CREDITS_MD" ]]; then
    # Convert simple markdown links [text](url) to <a> tags, wrap in styled HTML
    CREDITS_BODY=$(sed -E \
        -e 's|\[([^]]+)\]\(([^)]+)\)|<a href="\2">\1</a>|g' \
        -e 's|^$|<br>|' \
        "$CREDITS_MD")
    cat > "$APP_BUNDLE/Contents/Resources/Credits.html" << CREDITSEOF
<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="font-family: -apple-system, Helvetica Neue, sans-serif; font-size: 11px; color: #999; text-align: center;">
${CREDITS_BODY}
</body>
</html>
CREDITSEOF
    echo "    Generated Credits.html from Credits.md"
fi

# ── Bundle Homebrew dylibs (HEIC) ─────────────────────────────────────────────
# When HEIC is enabled the binary dynamically links against Homebrew's libheif
# and its transitive deps.  We copy them into Frameworks/ and rewrite all load
# paths to use @rpath so the .app is fully self-contained.
if [[ "$HEIC_ENABLED" == true ]]; then
    echo "==> Bundling libheif dylibs"
    FRAMEWORKS="$APP_BUNDLE/Contents/Frameworks"
    mkdir -p "$FRAMEWORKS"

    # Full transitive closure of Homebrew dylibs required by the binary.
    BREW="/opt/homebrew/opt"
    DYLIBS=(
        "$BREW/libheif/lib/libheif.1.dylib"
        "$BREW/x265/lib/libx265.215.dylib"
        "$BREW/libde265/lib/libde265.0.dylib"
        "$BREW/aom/lib/libaom.3.dylib"
        "$BREW/libvmaf/lib/libvmaf.3.dylib"
        "$BREW/webp/lib/libsharpyuv.0.dylib"
    )

    # Copy each dylib into Frameworks/
    for dylib in "${DYLIBS[@]}"; do
        if [[ ! -f "$dylib" ]]; then
            echo "error: expected dylib not found: $dylib"
            exit 1
        fi
        cp "$dylib" "$FRAMEWORKS/"
    done

    # Add @rpath pointing to Frameworks/ on the main binary
    install_name_tool -add_rpath "@executable_path/../Frameworks" \
        "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"

    # Rewrite the main binary's references from absolute Homebrew paths to @rpath
    for dylib in "${DYLIBS[@]}"; do
        name="$(basename "$dylib")"
        install_name_tool -change "$dylib" "@rpath/$name" \
            "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME" 2>/dev/null || true
    done

    # For each bundled dylib: set its own install name to @rpath/name, then
    # rewrite any references it has to other Homebrew dylibs.
    for dylib in "${DYLIBS[@]}"; do
        name="$(basename "$dylib")"
        bundled="$FRAMEWORKS/$name"
        chmod u+w "$bundled"
        install_name_tool -id "@rpath/$name" "$bundled"
        for dep in "${DYLIBS[@]}"; do
            dep_name="$(basename "$dep")"
            install_name_tool -change "$dep" "@rpath/$dep_name" "$bundled" 2>/dev/null || true
        done
    done

    echo "    Bundled ${#DYLIBS[@]} dylibs into Frameworks/"
fi

# ── Info.plist ────────────────────────────────────────────────────────────────
cat > "$APP_BUNDLE/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
    "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>

    <!-- Identity -->
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleExecutable</key>
    <string>${BINARY_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleSignature</key>
    <string>????</string>
    <key>LSMinimumSystemVersion</key>
    <string>${MIN_MACOS}</string>

    <!-- Icon -->
    <key>CFBundleIconFile</key>
    <string>icon</string>

    <!-- Copyright (shown in About window) -->
    <key>NSHumanReadableCopyright</key>
    <string>Copyright © 2024-2026 astrolabius.xyz. All rights reserved.</string>

    <!-- HiDPI / Retina -->
    <key>NSHighResolutionCapable</key>
    <true/>

    <!-- Show in Dock (not a background agent) -->
    <key>LSUIElement</key>
    <false/>

    <!-- Accept file drops on the Dock icon -->
    <key>NSServices</key>
    <array/>

    <!--
        File associations.
        LSHandlerRank "Alternate" means we appear in "Open With" but don't
        override the current system default. To make PikaViewer the default
        for any type, run:
            duti -s xyz.astrolabius.pikaviewer public.jpeg all
        or use Finder → Get Info → Open with → Change All.
    -->
    <key>CFBundleDocumentTypes</key>
    <array>

        <dict>
            <key>CFBundleTypeName</key>
            <string>JPEG Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.jpeg</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>PNG Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.png</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>GIF Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.compuserve.gif</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>BMP Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.microsoft.bmp</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>TIFF Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.tiff</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>WebP Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>org.webmproject.webp</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>Windows Icon</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.microsoft.ico</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>HEIC Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.heic</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>HEIF Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.heif</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>AVIF Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.avif</string>
            </array>
        </dict>

        <!-- RAW formats (LibRaw) -->

        <dict>
            <key>CFBundleTypeName</key>
            <string>Nikon RAW Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.nikon.raw-image</string>
                <string>com.nikon.nrw-raw-image</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>Canon RAW Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.canon.cr2-raw-image</string>
                <string>com.canon.cr3-raw-image</string>
                <string>com.canon.crw-raw-image</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>Sony RAW Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.sony.arw-raw-image</string>
                <string>com.sony.raw-image</string>
                <string>com.sony.sr2-raw-image</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>Fujifilm RAW Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.fuji.raw-image</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>Olympus RAW Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.olympus.raw-image</string>
                <string>com.olympus.or-raw-image</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>Panasonic RAW Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.panasonic.raw-image</string>
                <string>com.panasonic.rw2-raw-image</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>Pentax RAW Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.pentax.raw-image</string>
            </array>
        </dict>

        <dict>
            <key>CFBundleTypeName</key>
            <string>Adobe DNG Image</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>com.adobe.raw-image</string>
            </array>
        </dict>

    </array>

    <!--
        UTImportedTypeDeclarations: teach macOS about types it doesn't know natively.
        - WebP: system-defined only on macOS 14+; declare it for 12/13 compat.
        - AVIF: system-defined only on macOS 13+; declare it for macOS 12 compat.
        - HEIC/HEIF: system-defined from macOS 11+, no import needed.
        - ICO:  system-defined on all relevant macOS versions, no import needed.
    -->
    <key>UTImportedTypeDeclarations</key>
    <array>

        <dict>
            <key>UTTypeIdentifier</key>
            <string>org.webmproject.webp</string>
            <key>UTTypeDescription</key>
            <string>WebP Image</string>
            <key>UTTypeConformsTo</key>
            <array>
                <string>public.image</string>
                <string>public.data</string>
            </array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key>
                <array>
                    <string>webp</string>
                </array>
                <key>public.mime-type</key>
                <string>image/webp</string>
            </dict>
        </dict>

        <dict>
            <key>UTTypeIdentifier</key>
            <string>public.avif</string>
            <key>UTTypeDescription</key>
            <string>AVIF Image</string>
            <key>UTTypeConformsTo</key>
            <array>
                <string>public.image</string>
                <string>public.data</string>
            </array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key>
                <array>
                    <string>avif</string>
                </array>
                <key>public.mime-type</key>
                <string>image/avif</string>
            </dict>
        </dict>

    </array>

</dict>
</plist>
PLIST

# ── Ad-hoc code sign ──────────────────────────────────────────────────────────
# "-s -" = ad-hoc identity. No Apple Developer account required.
# Gatekeeper will still warn on first launch; right-click → Open to bypass once.
echo "==> Signing (ad-hoc)"
codesign --force --deep -s - "$APP_BUNDLE"
codesign --verify --verbose "$APP_BUNDLE"

# ── Optional DMG ─────────────────────────────────────────────────────────────
if [[ "$MAKE_DMG" == true ]]; then
    DMG_PATH="$OUT_DIR/${APP_NAME}-${VERSION}.dmg"
    echo "==> Creating DMG: $DMG_PATH"

    STAGING="$OUT_DIR/.dmg-staging"
    rm -rf "$STAGING"
    mkdir -p "$STAGING"
    cp -R "$APP_BUNDLE" "$STAGING/"
    # Symlink to /Applications for the classic drag-install UX
    ln -s /Applications "$STAGING/Applications"

    hdiutil create \
        -volname "$APP_NAME $VERSION" \
        -srcfolder "$STAGING" \
        -ov -format UDZO \
        "$DMG_PATH"

    rm -rf "$STAGING"
    echo "==> DMG ready: $DMG_PATH"
fi

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "Done."
echo "  Bundle : $APP_BUNDLE"
[[ "$MAKE_DMG" == true ]] && echo "  DMG    : $OUT_DIR/${APP_NAME}-${VERSION}.dmg"
echo ""
echo "To install: drag $APP_NAME.app to /Applications"
echo ""
echo "To set as default viewer for a type (requires 'duti' — brew install duti):"
echo "  duti -s $BUNDLE_ID public.jpeg all"
echo "  duti -s $BUNDLE_ID public.png all"
echo "  duti -s $BUNDLE_ID com.compuserve.gif all"
echo "  duti -s $BUNDLE_ID com.microsoft.bmp all"
echo "  duti -s $BUNDLE_ID public.tiff all"
echo "  duti -s $BUNDLE_ID org.webmproject.webp all"
echo "  duti -s $BUNDLE_ID com.microsoft.ico all"
echo "  duti -s $BUNDLE_ID public.heic all"
echo "  duti -s $BUNDLE_ID public.heif all"
echo "  duti -s $BUNDLE_ID public.avif all"
echo "  duti -s $BUNDLE_ID com.nikon.raw-image all       # NEF"
echo "  duti -s $BUNDLE_ID com.canon.cr2-raw-image all   # CR2"
echo "  duti -s $BUNDLE_ID com.canon.cr3-raw-image all   # CR3"
echo "  duti -s $BUNDLE_ID com.sony.arw-raw-image all    # ARW"
echo "  duti -s $BUNDLE_ID com.fuji.raw-image all        # RAF"
echo "  duti -s $BUNDLE_ID com.olympus.raw-image all     # ORF"
echo "  duti -s $BUNDLE_ID com.panasonic.raw-image all   # RW2"
echo "  duti -s $BUNDLE_ID com.pentax.raw-image all      # PEF"
echo "  duti -s $BUNDLE_ID com.adobe.raw-image all       # DNG"
echo ""
echo "Or use Finder → Get Info → Open with → Change All for each type."
