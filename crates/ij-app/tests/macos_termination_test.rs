use egui::{Context, RawInput, ViewportCommand, ViewportId};
use ij_app::app::{CloseTarget, ImageJApp};
use ij_app::macos_bridge::{
    self, NS_TERMINATE_CANCEL, NS_TERMINATE_NOW, is_quit_authorized, poll_native_quit_requested,
    request_native_quit, reset_bridge_state, set_dirty_state, set_quit_authorized,
};
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn native_termination_bridge_lifecycle_with_dirty_document() {
    let _guard = TEST_LOCK.lock().unwrap();
    reset_bridge_state();
    let ctx = Context::default();
    macos_bridge::register_egui_ctx(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("dirty_mac_quit", 16, 16).unwrap();
    assert!(app.is_dirty(id));
    set_dirty_state(true);

    // 1. Native OS (Cmd-Q or App Menu Quit) invokes request_native_quit
    request_native_quit();
    assert!(!is_quit_authorized(), "Quit must not be pre-authorized");

    // 2. Main loop / frame observes native quit request
    let root_input = RawInput::default();
    let full_output = ctx.run(root_input, |ctx| {
        app.update_ui(ctx);
    });

    let commands = &full_output.viewport_output[&ViewportId::ROOT].commands;

    // Must NOT immediately close or quit
    assert!(
        !commands.contains(&ViewportCommand::Close),
        "Dirty native quit must NOT emit ViewportCommand::Close"
    );
    assert!(
        !app.quit_app,
        "quit_app must be false while prompt is pending"
    );
    assert!(
        !is_quit_authorized(),
        "Termination must not be authorized yet"
    );

    // Dirty prompt must be displayed with Quit target
    assert!(
        app.pending_close.is_some(),
        "Pending close prompt must be shown"
    );
    let pending = app.pending_close.as_ref().unwrap();
    assert_eq!(pending.target, CloseTarget::Quit);
    assert_eq!(pending.current_prompt, Some(id));
    assert_eq!(
        app.open_window_count(),
        1,
        "Unsaved document must be preserved"
    );

    // 3. User chooses Cancel: app remains alive with document preserved
    app.confirm_pending_close_cancel();
    assert!(app.pending_close.is_none());
    assert!(!app.quit_app);
    assert_eq!(app.open_window_count(), 1);
    assert!(!is_quit_authorized());

    // 4. Native OS requests quit again, this time user chooses Discard ("Don't Save")
    request_native_quit();
    let _ = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });

    assert!(app.pending_close.is_some());
    let finished = app.confirm_pending_close_discard();
    assert!(finished, "Discarding single dirty document finishes close");
    assert!(
        app.quit_app,
        "quit_app must be true after resolving all dirty docs"
    );

    // Next frame: app emits ViewportCommand::Close and sets quit authorized
    let second_output = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });

    let second_commands = &second_output.viewport_output[&ViewportId::ROOT].commands;
    assert!(
        second_commands.contains(&ViewportCommand::Close),
        "Resolved quit must emit ViewportCommand::Close"
    );
    assert!(
        is_quit_authorized(),
        "Quit must now be authorized for native termination"
    );
    assert_eq!(app.open_window_count(), 0);
}

#[test]
fn native_termination_bridge_lifecycle_clean_app_quits_immediately() {
    let _guard = TEST_LOCK.lock().unwrap();
    reset_bridge_state();
    let ctx = Context::default();
    macos_bridge::register_egui_ctx(&ctx);

    let mut app = ImageJApp::new();
    assert_eq!(app.open_window_count(), 0);
    assert!(!app.any_dirty());

    // Native quit request on clean app
    request_native_quit();

    let output = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });

    assert!(app.quit_app, "Clean native quit must set quit_app = true");
    assert!(
        is_quit_authorized(),
        "Clean quit must authorize native termination"
    );

    let commands = &output.viewport_output[&ViewportId::ROOT].commands;
    assert!(
        commands.contains(&ViewportCommand::Close),
        "Clean native quit must emit ViewportCommand::Close"
    );
    assert!(
        !commands.contains(&ViewportCommand::CancelClose),
        "Clean native quit must NOT emit CancelClose"
    );
}

#[test]
fn repeated_native_quit_while_prompt_open_is_deterministic() {
    let _guard = TEST_LOCK.lock().unwrap();
    reset_bridge_state();
    let ctx = Context::default();
    macos_bridge::register_egui_ctx(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("repeated_native_quit", 16, 16).unwrap();
    assert!(app.is_dirty(id));

    request_native_quit();
    let _ = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });
    let first = app.pending_close.clone().expect("prompt must be open");
    assert_eq!(first.target, CloseTarget::Quit);
    assert_eq!(first.current_prompt, Some(id));

    // A second native quit (Cmd-Q again, dock quit, etc.) while the modal is
    // open must neither rescan nor replace the existing prompt.
    request_native_quit();
    let output = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });
    let commands = &output.viewport_output[&ViewportId::ROOT].commands;

    assert!(
        commands.contains(&ViewportCommand::CancelClose),
        "Repeated native quit must stay deferred"
    );
    assert!(!commands.contains(&ViewportCommand::Close));
    assert!(!app.quit_app);
    assert!(!is_quit_authorized());
    assert_eq!(app.pending_close.as_ref(), Some(&first));
    assert_eq!(app.open_window_count(), 1);

    // The prompt still resolves normally afterwards.
    assert!(app.confirm_pending_close_discard());
    let output = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });
    let commands = &output.viewport_output[&ViewportId::ROOT].commands;
    assert!(commands.contains(&ViewportCommand::Close));
    assert!(is_quit_authorized());
}

#[cfg(target_os = "macos")]
static PREEXISTING_TERMINATION_CALLS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "macos")]
unsafe extern "C" fn preexisting_should_terminate(
    _this: *mut std::ffi::c_void,
    _cmd: *const std::ffi::c_void,
    _sender: *mut std::ffi::c_void,
) -> usize {
    PREEXISTING_TERMINATION_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    NS_TERMINATE_CANCEL
}

#[cfg(target_os = "macos")]
#[test]
fn existing_delegate_imp_is_preserved_and_chained() {
    let _guard = TEST_LOCK.lock().unwrap();
    use std::ffi::{c_char, c_void};

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn objc_allocateClassPair(
            superclass: *mut c_void,
            name: *const c_char,
            extra_bytes: usize,
        ) -> *mut c_void;
        fn objc_registerClassPair(cls: *mut c_void);
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn class_addMethod(
            cls: *mut c_void,
            name: *const c_void,
            imp: unsafe extern "C" fn(),
            types: *const c_char,
        ) -> bool;
        fn objc_msgSend(receiver: *mut c_void, sel: *const c_void, ...) -> *mut c_void;
    }

    reset_bridge_state();
    PREEXISTING_TERMINATION_CALLS.store(0, std::sync::atomic::Ordering::SeqCst);

    unsafe {
        let nsobject_cls = objc_getClass(b"NSObject\0".as_ptr().cast());
        assert!(!nsobject_cls.is_null());

        let class_name = b"IJTestExistingDelegate\0".as_ptr().cast();
        let mock_cls = objc_getClass(class_name);
        assert!(
            mock_cls.is_null(),
            "test class must not already be registered by another test"
        );
        let mock_cls = objc_allocateClassPair(nsobject_cls, class_name, 0);
        assert!(!mock_cls.is_null());

        // Simulate a delegate that already implements applicationShouldTerminate:.
        let sel = sel_registerName(b"applicationShouldTerminate:\0".as_ptr().cast());
        let types = b"Q@:@\0".as_ptr().cast();
        let imp: unsafe extern "C" fn() = std::mem::transmute(
            preexisting_should_terminate
                as unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) -> usize,
        );
        assert!(class_addMethod(mock_cls, sel, imp, types));
        objc_registerClassPair(mock_cls);

        assert!(macos_bridge::inject_termination_hook(mock_cls));

        type MsgSendNoArgs = unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void;
        let msg_send: MsgSendNoArgs = std::mem::transmute(objc_msgSend as *const ());
        let obj = msg_send(mock_cls, sel_registerName(b"alloc\0".as_ptr().cast()));
        let instance = msg_send(obj, sel_registerName(b"init\0".as_ptr().cast()));
        assert!(!instance.is_null());

        type MsgSendTerm = unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) -> usize;
        let msg_send_term: MsgSendTerm = std::mem::transmute(objc_msgSend as *const ());

        // Unauthorized: the bridge owns termination to show the prompt and must
        // not run the pre-existing delegate yet.
        set_quit_authorized(false);
        assert_eq!(
            msg_send_term(instance, sel, std::ptr::null_mut()),
            NS_TERMINATE_CANCEL
        );
        assert_eq!(
            PREEXISTING_TERMINATION_CALLS.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "unauthorized quit must not invoke the pre-existing IMP"
        );
        assert!(poll_native_quit_requested());

        // Authorized: the pre-existing delegate decision is chained and kept.
        set_quit_authorized(true);
        assert_eq!(
            msg_send_term(instance, sel, std::ptr::null_mut()),
            NS_TERMINATE_CANCEL,
            "pre-existing delegate refusal must not be overridden"
        );
        assert_eq!(
            PREEXISTING_TERMINATION_CALLS.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "pre-existing IMP must be invoked exactly once (not clobbered)"
        );

        // Cleanup the global chain so later tests are unaffected.
        reset_bridge_state();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_objective_c_delegate_method_injection_and_call() {
    let _guard = TEST_LOCK.lock().unwrap();
    use std::ffi::{c_char, c_void};

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn objc_allocateClassPair(
            superclass: *mut c_void,
            name: *const c_char,
            extra_bytes: usize,
        ) -> *mut c_void;
        fn objc_registerClassPair(cls: *mut c_void);
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn objc_msgSend(receiver: *mut c_void, sel: *const c_void, ...) -> *mut c_void;
    }

    reset_bridge_state();

    unsafe {
        // Create an isolated test class inheriting from NSObject
        let nsobject_cls = objc_getClass(b"NSObject\0".as_ptr().cast());
        assert!(
            !nsobject_cls.is_null(),
            "NSObject must exist in ObjC runtime"
        );

        let class_name = b"IJTestMockDelegate\0".as_ptr().cast();
        let mut mock_cls = objc_getClass(class_name);
        if mock_cls.is_null() {
            mock_cls = objc_allocateClassPair(nsobject_cls, class_name, 0);
            assert!(!mock_cls.is_null());
            objc_registerClassPair(mock_cls);
        }

        // Inject the termination hook
        let injected = macos_bridge::inject_termination_hook(mock_cls);
        assert!(injected, "Hook injection must succeed");

        // Instantiate mock delegate: [[IJTestMockDelegate alloc] init]
        type MsgSendNoArgs = unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void;
        let msg_send: MsgSendNoArgs = std::mem::transmute(objc_msgSend as *const ());

        let alloc_sel = sel_registerName(b"alloc\0".as_ptr().cast());
        let init_sel = sel_registerName(b"init\0".as_ptr().cast());
        let obj = msg_send(mock_cls, alloc_sel);
        let instance = msg_send(obj, init_sel);
        assert!(!instance.is_null(), "Mock instance must be created");

        // 1. When quit is NOT authorized, applicationShouldTerminate: must return NSTerminateCancel (0)
        set_quit_authorized(false);
        assert!(!poll_native_quit_requested());

        type MsgSendTerm = unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) -> usize;
        let msg_send_term: MsgSendTerm = std::mem::transmute(objc_msgSend as *const ());
        let term_sel = sel_registerName(b"applicationShouldTerminate:\0".as_ptr().cast());

        let reply = msg_send_term(instance, term_sel, std::ptr::null_mut());
        assert_eq!(
            reply, NS_TERMINATE_CANCEL,
            "Unauthorized quit must return NSTerminateCancel (0)"
        );
        assert!(
            poll_native_quit_requested(),
            "Bridge must signal that native quit was requested"
        );
        assert!(
            !poll_native_quit_requested(),
            "Second poll must be false (cleared)"
        );

        // 2. When quit IS authorized, applicationShouldTerminate: must return NSTerminateNow (1)
        set_quit_authorized(true);
        let reply_authorized = msg_send_term(instance, term_sel, std::ptr::null_mut());
        assert_eq!(
            reply_authorized, NS_TERMINATE_NOW,
            "Authorized quit must return NSTerminateNow (1)"
        );
    }
}
