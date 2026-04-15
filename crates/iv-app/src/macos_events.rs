//! macOS file-open handler via NSApplicationDelegate injection.
//!
//! When macOS opens a file via a .app's "Open With" association, NSApplication's
//! `finishLaunching` processes queued Apple Events and calls `application:openURLs:`
//! on the delegate. winit's delegate doesn't implement this, so macOS reports
//! "cannot open files in X format".
//!
//! **Timing is critical.** `finishLaunching` processes open-document events BEFORE
//! posting `applicationDidFinishLaunching:` (which winit maps to `resumed()`).
//! So any injection in `resumed()` is too late.
//!
//! Strategy:
//! - `register()` is called between `EventLoop::new()` and `run_app()`.
//!   At this point NSApplication exists. We check if the delegate is already set
//!   (winit may set it during EventLoop::new or early in run_app setup) and patch
//!   its class. We ALSO register a notification observer for
//!   `NSApplicationWillFinishLaunchingNotification` which fires at the TOP of
//!   `finishLaunching`, before Apple Events are processed — giving us a second
//!   chance to patch the class if it wasn't ready earlier.

use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::sync::Mutex;

/// Path received from the most recent file-open event. Consumed by
/// App::about_to_wait() which calls load_path() and loads the image.
pub(crate) static PENDING_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

// ── ObjC runtime FFI ──────────────────────────────────────────────────────────

#[link(name = "objc")]
#[allow(clashing_extern_declarations)]
unsafe extern "C" {
    #[link_name = "objc_msgSend"]
    fn msg0(recv: *mut c_void, sel: *const c_void) -> *mut c_void;

    // 4-arg variant — same C symbol, different Rust signature (objc_msgSend is variadic)
    #[link_name = "objc_msgSend"]
    fn msg4(
        recv: *mut c_void, sel: *const c_void,
        a1: *mut c_void, a2: *const c_void, a3: *mut c_void, a4: *mut c_void,
    ) -> *mut c_void;

    fn sel_registerName(name: *const c_char) -> *const c_void;
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn object_getClass(obj: *mut c_void) -> *mut c_void;

    fn class_replaceMethod(
        cls: *mut c_void, sel: *const c_void,
        imp: *const c_void, types: *const c_char,
    ) -> *const c_void;

    fn objc_allocateClassPair(
        superclass: *mut c_void,
        name: *const c_char,
        extra_bytes: usize,
    ) -> *mut c_void;

    fn objc_registerClassPair(cls: *mut c_void);

    fn class_addMethod(
        cls: *mut c_void, sel: *const c_void,
        imp: *const c_void, types: *const c_char,
    ) -> bool;
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    // NSString* constant: @"NSApplicationWillFinishLaunchingNotification"
    static NSApplicationWillFinishLaunchingNotification: *mut c_void;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn sel(name: &[u8]) -> *const c_void {
    debug_assert_eq!(*name.last().unwrap(), 0);
    unsafe { sel_registerName(name.as_ptr().cast::<c_char>()) }
}

fn store_path(path: PathBuf) {
    if let Ok(mut lock) = PENDING_FILE.lock() {
        *lock = Some(path);
    }
}

// ── Injected delegate methods ─────────────────────────────────────────────────

unsafe extern "C" fn impl_open_urls(
    _this: *mut c_void, _sel: *const c_void, _app: *mut c_void,
    urls: *mut c_void,
) {
    let url = msg0(urls, sel(b"firstObject\0"));
    if url.is_null() { return; }
    let ns_path = msg0(url, sel(b"path\0"));
    if ns_path.is_null() { return; }
    let utf8 = msg0(ns_path, sel(b"UTF8String\0")) as *const c_char;
    if utf8.is_null() { return; }
    if let Ok(s) = CStr::from_ptr(utf8).to_str() {
        if !s.is_empty() {
            store_path(PathBuf::from(s));
        }
    }
}

unsafe extern "C" fn impl_open_files(
    _this: *mut c_void, _sel: *const c_void, _app: *mut c_void,
    files: *mut c_void,
) {
    let ns_str = msg0(files, sel(b"firstObject\0"));
    if ns_str.is_null() { return; }
    let utf8 = msg0(ns_str, sel(b"UTF8String\0")) as *const c_char;
    if utf8.is_null() { return; }
    if let Ok(s) = CStr::from_ptr(utf8).to_str() {
        if !s.is_empty() {
            store_path(PathBuf::from(s));
        }
    }
}

// ── Patching logic ───────────────────────────────────────────────────────────

/// Try to patch the delegate class. Returns true if successful.
unsafe fn try_patch_delegate() -> bool {
    let ns_app_cls = objc_getClass(c"NSApplication".as_ptr());
    if ns_app_cls.is_null() { return false; }

    let app = msg0(ns_app_cls, sel(b"sharedApplication\0"));
    if app.is_null() { return false; }

    let delegate = msg0(app, sel(b"delegate\0"));
    if delegate.is_null() { return false; }

    let cls = object_getClass(delegate);
    if cls.is_null() { return false; }

    let enc = c"v@:@@".as_ptr();
    class_replaceMethod(cls, sel(b"application:openURLs:\0"), impl_open_urls as _, enc);
    class_replaceMethod(cls, sel(b"application:openFiles:\0"), impl_open_files as _, enc);
    true
}

// ── Notification observer ─────────────────────────────────────────────────────

/// Callback for NSApplicationWillFinishLaunchingNotification.
/// This fires at the TOP of finishLaunching, BEFORE open-document events are
/// processed. If we didn't patch earlier (delegate wasn't set), we patch here.
unsafe extern "C" fn will_finish_launching(
    _this: *mut c_void, _sel: *const c_void, _notification: *mut c_void,
) {
    try_patch_delegate();
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Register file-open handlers. Call between EventLoop::new() and run_app().
///
/// Two-pronged approach:
/// 1. Try to patch the delegate class immediately (works if winit already set it).
/// 2. Register a WillFinishLaunching notification observer as a fallback — this
///    fires at the top of finishLaunching, before Apple Events are processed.
pub(crate) fn register() {
    // Attempt 1: patch now if delegate already exists
    unsafe { try_patch_delegate() };

    // Attempt 2: register notification observer for WillFinishLaunching.
    // This fires even if the delegate was already patched (harmless double-patch).
    unsafe {
        // Create a tiny ObjC class with a handler method
        let nsobject = objc_getClass(c"NSObject".as_ptr());
        if nsobject.is_null() { return; }

        let cls_name = c"IVWillFinishObserver".as_ptr();
        let cls = objc_allocateClassPair(nsobject, cls_name, 0);
        if cls.is_null() {
            // Class may already exist from a previous call — that's fine,
            // the first registration is what matters.
            return;
        }

        // Add handler method: -(void)onNotification:(NSNotification*)note
        class_addMethod(
            cls,
            sel(b"onNotification:\0"),
            will_finish_launching as _,
            c"v@:@".as_ptr(),
        );
        objc_registerClassPair(cls);

        // Allocate & init an instance
        let instance = msg0(cls, sel(b"alloc\0"));
        let instance = msg0(instance, sel(b"init\0"));
        if instance.is_null() { return; }

        // [[NSNotificationCenter defaultCenter]
        //     addObserver:instance
        //     selector:@selector(onNotification:)
        //     name:NSApplicationWillFinishLaunchingNotification
        //     object:nil]
        let nc_cls = objc_getClass(c"NSNotificationCenter".as_ptr());
        if nc_cls.is_null() { return; }
        let center = msg0(nc_cls, sel(b"defaultCenter\0"));
        if center.is_null() { return; }

        msg4(
            center,
            sel(b"addObserver:selector:name:object:\0"),
            instance,                                          // observer
            sel(b"onNotification:\0").cast_mut(),                // selector
            NSApplicationWillFinishLaunchingNotification,       // name
            std::ptr::null_mut(),                              // object (nil)
        );

    }
}
