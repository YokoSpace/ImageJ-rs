use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static QUIT_AUTHORIZED: AtomicBool = AtomicBool::new(false);
static NATIVE_QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static ANY_DIRTY: AtomicBool = AtomicBool::new(false);
static EGUI_CTX: Mutex<Option<egui::Context>> = Mutex::new(None);
static INSTALLED: AtomicBool = AtomicBool::new(false);

pub const NS_TERMINATE_CANCEL: usize = 0;
pub const NS_TERMINATE_NOW: usize = 1;
pub const NS_TERMINATE_LATER: usize = 2;

/// Checks and clears the flag indicating whether a native OS quit was requested.
pub fn poll_native_quit_requested() -> bool {
    NATIVE_QUIT_REQUESTED.swap(false, Ordering::SeqCst)
}

/// Marks native quit as requested.
pub fn request_native_quit() {
    NATIVE_QUIT_REQUESTED.store(true, Ordering::SeqCst);
    if let Ok(guard) = EGUI_CTX.lock() {
        if let Some(ctx) = guard.as_ref() {
            ctx.request_repaint();
        }
    }
}

/// Sets whether quit has been confirmed and authorized by the app logic.
pub fn set_quit_authorized(authorized: bool) {
    QUIT_AUTHORIZED.store(authorized, Ordering::SeqCst);
}

/// Checks whether quit has been confirmed and authorized by the app logic.
pub fn is_quit_authorized() -> bool {
    QUIT_AUTHORIZED.load(Ordering::SeqCst)
}

/// Updates the cached dirty state of open documents.
pub fn set_dirty_state(dirty: bool) {
    ANY_DIRTY.store(dirty, Ordering::SeqCst);
}

/// Returns whether any open document is currently marked dirty.
pub fn is_dirty_state() -> bool {
    ANY_DIRTY.load(Ordering::SeqCst)
}

/// Registers the egui Context so the native bridge can request repaints on the main thread.
pub fn register_egui_ctx(ctx: &egui::Context) {
    if let Ok(mut guard) = EGUI_CTX.lock() {
        *guard = Some(ctx.clone());
    }
}

/// Returns whether the native termination bridge has been installed.
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::SeqCst)
}

/// Resets bridge state (useful for tests).
pub fn reset_bridge_state() {
    QUIT_AUTHORIZED.store(false, Ordering::SeqCst);
    NATIVE_QUIT_REQUESTED.store(false, Ordering::SeqCst);
    ANY_DIRTY.store(false, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    macos_impl::reset_previous_imp();
}

#[cfg(not(target_os = "macos"))]
pub fn install_macos_termination_bridge(_ctx: Option<&egui::Context>) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub use macos_impl::*;

/// Platform limits of the native quit guard (macOS):
///
/// * Only `-[NSApplicationDelegate applicationShouldTerminate:]` is hooked.
///   Force Quit (`kill -9`, Activity Monitor force quit), process crashes,
///   `SIGKILL` and shutdown/logout paths that bypass AppKit termination do not
///   call it; unsaved documents cannot be protected on those routes.
/// * The hook is attached to the Objective-C delegate class present at install
///   time. If a library replaces `[NSApp delegate]` with a new class later, the
///   bridge re-attaches on the next frame, but methods swizzled after that are
///   not observed.
/// * The pre-existing `applicationShouldTerminate:` IMP (including an inherited
///   one) is captured once and chained when quit is authorized. An IMP installed
///   on the same selector after the bridge was attached is not chained.
/// * Non-macOS builds have no native termination callback; they rely on the
///   eframe root-viewport close route, which requires the same dirty-document
///   confirmation before `ViewportCommand::Close` is emitted.
#[cfg(target_os = "macos")]
mod macos_impl {
    use super::*;
    use std::ffi::{c_char, c_void};

    type ShouldTerminateImp =
        unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) -> usize;

    /// The delegate's original `applicationShouldTerminate:` implementation, if
    /// any, so authorized quits preserve native/framework behavior instead of
    /// silently replacing it.
    static PREVIOUS_SHOULD_TERMINATE: std::sync::atomic::AtomicPtr<c_void> =
        std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

    #[link(name = "objc")]
    unsafe extern "C" {
        pub fn objc_getClass(name: *const c_char) -> *mut c_void;
        pub fn sel_registerName(name: *const c_char) -> *const c_void;
        pub fn class_replaceMethod(
            cls: *mut c_void,
            name: *const c_void,
            imp: unsafe extern "C" fn(),
            types: *const c_char,
        ) -> *mut c_void;
        pub fn class_getInstanceMethod(cls: *mut c_void, name: *const c_void) -> *mut c_void;
        pub fn method_getImplementation(method: *mut c_void) -> *mut c_void;
        pub fn object_getClass(obj: *mut c_void) -> *mut c_void;
        pub fn objc_msgSend(receiver: *mut c_void, sel: *const c_void, ...) -> *mut c_void;
    }

    pub(super) fn reset_previous_imp() {
        PREVIOUS_SHOULD_TERMINATE.store(std::ptr::null_mut(), Ordering::SeqCst);
    }

    /// The Objective-C method implementation for `-[NSApplicationDelegate applicationShouldTerminate:]`.
    /// Signature: `(NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender`
    pub unsafe extern "C" fn application_should_terminate_imp(
        this: *mut c_void,
        cmd: *const c_void,
        sender: *mut c_void,
    ) -> usize {
        if is_quit_authorized() {
            let previous = PREVIOUS_SHOULD_TERMINATE.load(Ordering::SeqCst);
            if !previous.is_null() {
                // Preserve the delegate's own decision (e.g. NSTerminateCancel
                // for a framework veto) rather than clobbering it.
                let previous_imp: ShouldTerminateImp = unsafe { std::mem::transmute(previous) };
                return unsafe { previous_imp(this, cmd, sender) };
            }
            NS_TERMINATE_NOW
        } else {
            request_native_quit();
            NS_TERMINATE_CANCEL
        }
    }

    /// Attaches `applicationShouldTerminate:` to the given Objective-C class.
    /// Returns false when the class is invalid or the method could not be
    /// resolved to the bridge afterwards.
    pub unsafe fn inject_termination_hook(cls: *mut c_void) -> bool {
        if cls.is_null() {
            return false;
        }
        let our_imp = application_should_terminate_imp as *const ();
        unsafe {
            let sel = sel_registerName(b"applicationShouldTerminate:\0".as_ptr().cast());
            let types = b"Q@:@\0".as_ptr().cast();

            // Use the resolved method (including an inherited one) so a
            // subclass delegate does not lose its superclass implementation.
            let existing_method = class_getInstanceMethod(cls, sel);
            let existing_imp = if existing_method.is_null() {
                std::ptr::null_mut()
            } else {
                method_getImplementation(existing_method)
            };
            if existing_imp as *const () == our_imp {
                // Already hooked, either directly or through inheritance.
                return true;
            }

            let replaced = class_replaceMethod(
                cls,
                sel,
                std::mem::transmute::<ShouldTerminateImp, unsafe extern "C" fn()>(
                    application_should_terminate_imp,
                ),
                types,
            );
            let previous = if replaced.is_null() {
                existing_imp
            } else {
                replaced
            };
            if !previous.is_null() && previous as *const () != our_imp {
                PREVIOUS_SHOULD_TERMINATE.store(previous, Ordering::SeqCst);
            }

            let installed = class_getInstanceMethod(cls, sel);
            !installed.is_null() && method_getImplementation(installed) as *const () == our_imp
        }
    }

    /// Installs the macOS termination bridge on winit's `WinitApplicationDelegate`
    /// and/or the current `[NSApp delegate]`.
    pub fn install_macos_termination_bridge(ctx: Option<&egui::Context>) -> bool {
        if let Some(c) = ctx {
            register_egui_ctx(c);
        }

        let mut hooked = false;

        unsafe {
            // 1. Hook class by known name "WinitApplicationDelegate"
            let winit_cls = objc_getClass(b"WinitApplicationDelegate\0".as_ptr().cast());
            if !winit_cls.is_null() {
                if inject_termination_hook(winit_cls) {
                    hooked = true;
                }
            }

            // 2. Hook current [NSApp delegate]'s class dynamically
            let nsapp_cls = objc_getClass(b"NSApplication\0".as_ptr().cast());
            if !nsapp_cls.is_null() {
                type MsgSendNoArgs =
                    unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void;
                let msg_send: MsgSendNoArgs = std::mem::transmute(objc_msgSend as *const ());

                let shared_app_sel = sel_registerName(b"sharedApplication\0".as_ptr().cast());
                let app = msg_send(nsapp_cls, shared_app_sel);
                if !app.is_null() {
                    let delegate_sel = sel_registerName(b"delegate\0".as_ptr().cast());
                    let delegate = msg_send(app, delegate_sel);
                    if !delegate.is_null() {
                        let delegate_cls = object_getClass(delegate);
                        if !delegate_cls.is_null() && inject_termination_hook(delegate_cls) {
                            hooked = true;
                        }
                    }
                }
            }
        }

        if hooked {
            INSTALLED.store(true, Ordering::SeqCst);
        }
        hooked
    }
}
