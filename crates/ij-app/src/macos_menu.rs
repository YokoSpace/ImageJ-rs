use crate::action::{AppAction, NativeMenuSpec};
use std::collections::VecDeque;
use std::sync::Mutex;

static ACTIONS: Mutex<VecDeque<AppAction>> = Mutex::new(VecDeque::new());

pub fn queue_action(action: AppAction, ctx: &egui::Context) {
    if let Ok(mut queue) = ACTIONS.lock() {
        queue.push_back(action);
    }
    ctx.request_repaint();
}

pub fn take_actions() -> Vec<AppAction> {
    ACTIONS
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
pub fn sync_menus(_spec: &[NativeMenuSpec], _ctx: &egui::Context) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub use cocoa::sync_menus;

#[cfg(all(target_os = "macos", debug_assertions))]
pub use cocoa::{probe_activate, probe_item, probe_titles};

#[cfg(target_os = "macos")]
mod cocoa {
    use super::super::macos_impl::objc_msgSend;
    use super::*;
    use std::collections::HashMap;
    use std::ffi::{CStr, CString, c_char, c_void};
    type Id = *mut c_void;
    type Sel = *const c_void;

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Sel;
        fn objc_allocateClassPair(base: Id, name: *const c_char, extra: usize) -> Id;
        fn objc_registerClassPair(class: Id);
        fn class_addMethod(
            class: Id,
            selector: Sel,
            imp: unsafe extern "C" fn(),
            types: *const c_char,
        ) -> bool;
    }

    // AppKit objects below are accessed only after NSThread.isMainThread.
    // Integer handles let the callback registry remain Send without claiming
    // that Cocoa objects themselves can be used on arbitrary threads.
    #[derive(Default)]
    struct State {
        menu: usize,
        target: usize,
        roots: Vec<usize>,
        items: Vec<usize>,
        registry: HashMap<isize, (AppAction, usize)>,
        next_tag: isize,
        spec: Vec<NativeMenuSpec>,
        ctx: Option<egui::Context>,
    }
    static STATE: Mutex<Option<State>> = Mutex::new(None);

    unsafe fn selector(name: &CStr) -> Sel {
        unsafe { sel_registerName(name.as_ptr()) }
    }
    unsafe fn class(name: &CStr) -> Id {
        unsafe { objc_getClass(name.as_ptr()) }
    }
    unsafe fn id0(object: Id, name: &CStr) -> Id {
        let send: unsafe extern "C" fn(Id, Sel) -> Id =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { send(object, selector(name)) }
    }
    unsafe fn void0(object: Id, name: &CStr) {
        let send: unsafe extern "C" fn(Id, Sel) =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { send(object, selector(name)) }
    }
    unsafe fn object1(object: Id, name: &CStr, value: Id) {
        let send: unsafe extern "C" fn(Id, Sel, Id) =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { send(object, selector(name), value) }
    }
    unsafe fn integer1(object: Id, name: &CStr, value: isize) {
        let send: unsafe extern "C" fn(Id, Sel, isize) =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { send(object, selector(name), value) }
    }
    unsafe fn boolean1(object: Id, name: &CStr, value: bool) {
        let send: unsafe extern "C" fn(Id, Sel, bool) =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { send(object, selector(name), value) }
    }
    unsafe fn integer0(object: Id, name: &CStr) -> isize {
        let send: unsafe extern "C" fn(Id, Sel) -> isize =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { send(object, selector(name)) }
    }
    unsafe fn main_thread() -> bool {
        let send: unsafe extern "C" fn(Id, Sel) -> bool =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { send(class(c"NSThread"), selector(c"isMainThread")) }
    }
    unsafe fn string(text: &str) -> Id {
        let text = CString::new(text.replace('\0', "")).unwrap();
        let send: unsafe extern "C" fn(Id, Sel, *const c_char) -> Id =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe {
            send(
                class(c"NSString"),
                selector(c"stringWithUTF8String:"),
                text.as_ptr(),
            )
        }
    }

    unsafe extern "C" fn triggered(_this: Id, _cmd: Sel, sender: Id) {
        let tag = unsafe { integer0(sender, c"tag") };
        // Drop the registry lock before requesting an egui repaint.
        let action = STATE.lock().ok().and_then(|state| {
            let state = state.as_ref()?;
            let action = state.registry.get(&tag)?.0;
            Some((action, state.ctx.clone()?))
        });
        if let Some((action, ctx)) = action {
            queue_action(action, &ctx);
        }
    }

    unsafe fn create_target() -> Id {
        unsafe {
            let mut target_class = class(c"ImageJRustMenuTarget");
            if target_class.is_null() {
                target_class =
                    objc_allocateClassPair(class(c"NSObject"), c"ImageJRustMenuTarget".as_ptr(), 0);
                if target_class.is_null() {
                    return std::ptr::null_mut();
                }
                let imp = std::mem::transmute::<
                    unsafe extern "C" fn(Id, Sel, Id),
                    unsafe extern "C" fn(),
                >(triggered);
                if !class_addMethod(
                    target_class,
                    selector(c"imageJMenuAction:"),
                    imp,
                    c"v@:@".as_ptr(),
                ) {
                    return std::ptr::null_mut();
                }
                objc_registerClassPair(target_class);
            }
            // Owned for the lifetime of the process: NSMenuItem's target is weak.
            id0(target_class, c"new")
        }
    }

    unsafe fn update_item(item: Id, spec: &NativeMenuSpec) {
        unsafe {
            boolean1(item, c"setEnabled:", spec.enabled);
            integer1(item, c"setState:", isize::from(spec.checked));
        }
    }

    unsafe fn append(menu: Id, spec: &NativeMenuSpec, state: &mut State, root: bool) {
        unsafe {
            let item = if spec.is_separator {
                let item = id0(class(c"NSMenuItem"), c"separatorItem");
                id0(item, c"retain")
            } else {
                let item = id0(class(c"NSMenuItem"), c"alloc");
                let send: unsafe extern "C" fn(Id, Sel, Id, Sel, Id) -> Id =
                    std::mem::transmute(objc_msgSend as *const ());
                // winit's App menu owns Cmd+Q; File > Quit uses the same app
                // action without installing a competing key equivalent.
                let key = if spec.action == Some(AppAction::Quit) {
                    String::new()
                } else {
                    spec.shortcut
                        .as_ref()
                        .map(|s| s.key.to_lowercase())
                        .unwrap_or_default()
                };
                send(
                    item,
                    selector(c"initWithTitle:action:keyEquivalent:"),
                    string(&spec.label),
                    if spec.action.is_some() {
                        selector(c"imageJMenuAction:")
                    } else {
                        std::ptr::null()
                    },
                    string(&key),
                )
            };
            state.items.push(item as usize);
            update_item(item, spec);
            if let Some(action) = spec.action {
                state.next_tag += 1;
                let tag = state.next_tag;
                state.registry.insert(tag, (action, item as usize));
                integer1(item, c"setTag:", tag);
                object1(item, c"setTarget:", state.target as Id);
                let shift = spec.shortcut.as_ref().is_some_and(|s| s.shift);
                integer1(
                    item,
                    c"setKeyEquivalentModifierMask:",
                    (1 << 20) | if shift { 1 << 17 } else { 0 },
                );
            }
            if !spec.children.is_empty() {
                let submenu = id0(class(c"NSMenu"), c"alloc");
                let send: unsafe extern "C" fn(Id, Sel, Id) -> Id =
                    std::mem::transmute(objc_msgSend as *const ());
                let submenu = send(submenu, selector(c"initWithTitle:"), string(&spec.label));
                boolean1(submenu, c"setAutoenablesItems:", false);
                for child in &spec.children {
                    append(submenu, child, state, false);
                }
                object1(item, c"setSubmenu:", submenu);
                void0(submenu, c"release");
            }
            object1(menu, c"addItem:", item);
            if root {
                state.roots.push(item as usize);
            } else {
                void0(item, c"release");
            }
        }
    }

    fn same_structure(a: &[NativeMenuSpec], b: &[NativeMenuSpec]) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| {
                a.label == b.label
                    && a.shortcut == b.shortcut
                    && a.action == b.action
                    && a.is_separator == b.is_separator
                    && same_structure(&a.children, &b.children)
            })
    }
    fn flatten<'a>(nodes: &'a [NativeMenuSpec], out: &mut Vec<&'a NativeMenuSpec>) {
        for node in nodes {
            out.push(node);
            flatten(&node.children, out);
        }
    }

    pub fn sync_menus(spec: &[NativeMenuSpec], ctx: &egui::Context) -> bool {
        unsafe {
            if !main_thread() {
                return false;
            }
            let app = id0(class(c"NSApplication"), c"sharedApplication");
            let menu = id0(app, c"mainMenu");
            if menu.is_null() {
                return false;
            }
            let Ok(mut guard) = STATE.lock() else {
                return false;
            };
            let state = guard.get_or_insert_with(State::default);
            state.ctx = Some(ctx.clone());
            if state.target == 0 {
                state.target = create_target() as usize;
                if state.target == 0 {
                    return false;
                }
            }
            if state.menu == menu as usize && same_structure(&state.spec, spec) {
                let mut flat = Vec::new();
                flatten(spec, &mut flat);
                for (&item, node) in state.items.iter().zip(flat) {
                    update_item(item as Id, node);
                }
            } else {
                // Retain our own roots and menu so replacement by another
                // library never leaves dangling handles. Remove only our items.
                if state.menu != 0 {
                    for item in state.roots.drain(..) {
                        object1(state.menu as Id, c"removeItem:", item as Id);
                        void0(item as Id, c"release");
                    }
                    void0(state.menu as Id, c"release");
                }
                state.menu = id0(menu, c"retain") as usize;
                state.items.clear();
                state.registry.clear();
                for node in spec {
                    append(menu, node, state, true);
                }
            }
            state.spec = spec.to_vec();
            true
        }
    }

    #[cfg(debug_assertions)]
    pub fn probe_titles() -> Vec<String> {
        unsafe {
            if !main_thread() {
                return Vec::new();
            }
            let menu = id0(
                id0(class(c"NSApplication"), c"sharedApplication"),
                c"mainMenu",
            );
            let count = integer0(menu, c"numberOfItems");
            let send: unsafe extern "C" fn(Id, Sel, isize) -> Id =
                std::mem::transmute(objc_msgSend as *const ());
            (0..count)
                .map(|index| {
                    let item = send(menu, selector(c"itemAtIndex:"), index);
                    let bytes = id0(id0(item, c"title"), c"UTF8String") as *const c_char;
                    if bytes.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(bytes).to_string_lossy().into_owned()
                    }
                })
                .collect()
        }
    }

    #[cfg(debug_assertions)]
    pub fn probe_item(action: AppAction) -> Option<(bool, String)> {
        unsafe {
            if !main_thread() {
                return None;
            }
            let item = STATE.lock().ok().and_then(|guard| {
                let state = guard.as_ref()?;
                Some(
                    state
                        .registry
                        .values()
                        .find(|(registered, _)| *registered == action)?
                        .1,
                )
            })? as Id;
            let send: unsafe extern "C" fn(Id, Sel) -> bool =
                std::mem::transmute(objc_msgSend as *const ());
            let enabled = send(item, selector(c"isEnabled"));
            let bytes = id0(id0(item, c"keyEquivalent"), c"UTF8String") as *const c_char;
            let key = if bytes.is_null() {
                String::new()
            } else {
                CStr::from_ptr(bytes).to_string_lossy().into_owned()
            };
            Some((enabled, key))
        }
    }

    #[cfg(debug_assertions)]
    pub fn probe_activate(action: AppAction) -> bool {
        unsafe {
            if !main_thread() {
                return false;
            }
            let entry = STATE.lock().ok().and_then(|guard| {
                let state = guard.as_ref()?;
                let item = state
                    .registry
                    .values()
                    .find(|(registered, _)| *registered == action)?
                    .1;
                Some((state.target, item))
            });
            if let Some((target, item)) = entry {
                object1(target as Id, c"imageJMenuAction:", item as Id);
                true
            } else {
                false
            }
        }
    }
}
