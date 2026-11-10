use egui::{
    Context, Event, Key, Modifiers, RawInput, ViewportCommand, ViewportEvent, ViewportId,
    ViewportInfo,
};
use ij_app::app::{CloseTarget, ImageJApp};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

fn make_viewport_input(vid: ViewportId, focused: bool) -> RawInput {
    let mut input = RawInput {
        viewport_id: vid,
        ..Default::default()
    };
    input.viewports.insert(
        vid,
        ViewportInfo {
            focused: Some(focused),
            ..Default::default()
        },
    );
    input
}

fn setup_test_viewport_renderer(
    per_viewport_inputs: Rc<RefCell<HashMap<ViewportId, RawInput>>>,
    rendered_viewports: Option<Rc<RefCell<Vec<ViewportId>>>>,
) {
    Context::set_immediate_viewport_renderer(move |egui_ctx, mut immediate_viewport| {
        let vid = immediate_viewport.ids.this;
        if let Some(rv) = &rendered_viewports {
            rv.borrow_mut().push(vid);
        }
        let mut input = per_viewport_inputs
            .borrow_mut()
            .remove(&vid)
            .unwrap_or_else(|| make_viewport_input(vid, false));
        input.viewport_id = vid;
        input.viewports.entry(vid).or_default();
        let _ = egui_ctx.run(input, |ctx| {
            (immediate_viewport.viewport_ui_cb)(ctx);
        });
    });
}

#[test]
fn root_close_clean_app_outputs_close_without_cancel_close() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();

    // Clean app with no dirty documents
    assert_eq!(app.open_window_count(), 0);
    assert!(!app.any_dirty());

    // Root window receives CloseRequested from OS / winit
    let mut root_input = RawInput::default();
    root_input
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap()
        .events
        .push(ViewportEvent::Close);

    let full_output = ctx.run(root_input, |ctx| {
        app.update_ui(ctx);
    });

    let commands = &full_output.viewport_output[&ViewportId::ROOT].commands;

    // Quit was accepted and resolved immediately:
    // It MUST NOT send CancelClose (which causes never-ending quit loop in eframe)!
    assert!(
        !commands.contains(&ViewportCommand::CancelClose),
        "Clean root close must NOT emit CancelClose, got: {:?}",
        commands
    );
    // It MUST send Close so eframe terminates the process:
    assert!(
        commands.contains(&ViewportCommand::Close),
        "Clean root close must emit Close, got: {:?}",
        commands
    );
    assert!(app.quit_app, "app.quit_app must be set to true");

    // Second frame: eframe winit delivers CloseRequested in response to ViewportCommand::Close.
    // The app MUST NOT emit CancelClose on this second frame either!
    let mut next_input = RawInput::default();
    next_input
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap()
        .events
        .push(ViewportEvent::Close);

    let second_output = ctx.run(next_input, |ctx| {
        app.update_ui(ctx);
    });

    let second_commands = &second_output.viewport_output[&ViewportId::ROOT].commands;
    assert!(
        !second_commands.contains(&ViewportCommand::CancelClose),
        "Subsequent frame after quit must NOT emit CancelClose, got: {:?}",
        second_commands
    );
}

#[test]
fn root_close_dirty_app_outputs_cancel_close_only_and_shows_prompt() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();

    // Create a dirty document
    let id = app.open_ramp_8bit("dirty_img", 16, 16).unwrap();
    assert!(app.is_dirty(id));

    // Root window receives CloseRequested
    let mut root_input = RawInput::default();
    root_input
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap()
        .events
        .push(ViewportEvent::Close);

    let full_output = ctx.run(root_input, |ctx| {
        app.update_ui(ctx);
    });

    let commands = &full_output.viewport_output[&ViewportId::ROOT].commands;

    // Because close is DEFERRED, it MUST send CancelClose so eframe does not kill the process:
    assert!(
        commands.contains(&ViewportCommand::CancelClose),
        "Deferred root close MUST emit CancelClose to prevent premature exit"
    );
    // It MUST NOT send Close while deferred:
    assert!(
        !commands.contains(&ViewportCommand::Close),
        "Deferred root close MUST NOT emit Close"
    );
    assert!(!app.quit_app, "quit_app must be false while deferred");
    assert!(
        app.pending_close.is_some(),
        "pending_close confirmation guard must be active"
    );
    let pending = app.pending_close.as_ref().unwrap();
    assert_eq!(pending.target, CloseTarget::Quit);
    assert_eq!(
        app.open_window_count(),
        1,
        "Dirty document must be retained"
    );

    // Now resolve the prompt by discarding ("Don't Save")
    let resolved = app.confirm_pending_close_discard();
    assert!(resolved, "Discarding all dirty docs resolves quit");
    assert!(app.quit_app, "quit_app is now true");

    // Run frame after resolution
    let resolve_input = RawInput::default();
    let resolve_output = ctx.run(resolve_input, |ctx| {
        app.update_ui(ctx);
    });
    let resolve_commands = &resolve_output.viewport_output[&ViewportId::ROOT].commands;

    assert!(
        resolve_commands.contains(&ViewportCommand::Close),
        "Resolved quit MUST emit Close"
    );
    assert!(
        !resolve_commands.contains(&ViewportCommand::CancelClose),
        "Resolved quit MUST NOT emit CancelClose"
    );
}

#[test]
fn per_viewport_shortcuts_reach_focused_image() {
    let ctx = Context::default();
    ctx.set_embed_viewports(false);
    let mut app = ImageJApp::new();

    let id1 = app.open_ramp_8bit("Image_1", 16, 16).unwrap();
    let id2 = app.open_ramp_8bit("Image_2", 32, 32).unwrap();

    // Make Image 1 active in session
    app.set_active_window(id1).unwrap();
    assert_eq!(app.active_image_id(), Some(id1));

    let vp_id2 = ViewportId::from_hash_of(&("ij_image_window", id2.0));

    // Simulate native eframe multi-viewport architecture:
    let per_viewport_inputs = Rc::new(RefCell::new(HashMap::new()));

    // Key event 'M' (Measure) is delivered ONLY to Image 2 viewport (user is focused on Image 2)
    let mut img2_input = make_viewport_input(vp_id2, true);
    img2_input.events.push(Event::Key {
        key: Key::M,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    per_viewport_inputs.borrow_mut().insert(vp_id2, img2_input);

    setup_test_viewport_renderer(per_viewport_inputs, None);

    // Root viewport receives empty RawInput (no keys)
    let root_input = RawInput::default();
    let _ = ctx.run(root_input, |ctx| {
        app.update_ui(ctx);
    });

    // Measurement must have been executed on Image 2:
    assert_eq!(
        app.session.results.rows.len(),
        1,
        "Image-focused shortcut 'M' must measure"
    );
    let row = &app.session.results.rows[0];
    // Image 2 is 32x32 = 1024 pixels. Image 1 is 16x16 = 256 pixels.
    assert_eq!(
        row.area, 1024.0,
        "Shortcut delivered to Image 2 viewport must measure Image 2 (Area 1024), NOT Image 1"
    );
    assert_eq!(
        app.active_image_id(),
        Some(id2),
        "Focused image must become active window"
    );
}

#[test]
fn no_duplicate_shortcuts_between_root_and_image_viewport() {
    let ctx = Context::default();
    ctx.set_embed_viewports(false);
    let mut app = ImageJApp::new();

    let id = app.open_ramp_8bit("Single_Img", 16, 16).unwrap();
    let vp_id = ViewportId::from_hash_of(&("ij_image_window", id.0));

    let per_viewport_inputs = Rc::new(RefCell::new(HashMap::new()));

    // Key event 'M' is delivered to the image window
    let mut img_input = make_viewport_input(vp_id, true);
    img_input.events.push(Event::Key {
        key: Key::M,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    per_viewport_inputs.borrow_mut().insert(vp_id, img_input);

    setup_test_viewport_renderer(per_viewport_inputs, None);

    let _ = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });

    // Exactly one measurement must be registered (key consumed, no duplicates)
    assert_eq!(
        app.session.results.rows.len(),
        1,
        "Key consumption must prevent duplicate shortcut execution"
    );
}

#[test]
fn shortcuts_blocked_during_error_dialog_and_modal_prompt() {
    let ctx = Context::default();
    ctx.set_embed_viewports(false);
    let mut app = ImageJApp::new();

    let id = app.open_ramp_8bit("Test_Img", 16, 16).unwrap();
    let vp_id = ViewportId::from_hash_of(&("ij_image_window", id.0));

    // Active error dialog
    app.dialogs.last_error = Some("Test error active".into());

    let per_viewport_inputs = Rc::new(RefCell::new(HashMap::new()));

    // Try to trigger Measure 'M' on image viewport while error dialog is open
    let mut img_input = make_viewport_input(vp_id, true);
    img_input.events.push(Event::Key {
        key: Key::M,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    per_viewport_inputs.borrow_mut().insert(vp_id, img_input);

    setup_test_viewport_renderer(per_viewport_inputs, None);

    let _ = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });

    assert_eq!(
        app.session.results.rows.len(),
        0,
        "Shortcuts must NOT execute while error dialog is active"
    );
}

#[test]
fn error_dialog_shows_long_path_and_dismisses() {
    let ctx = Context::default();
    ctx.set_embed_viewports(false);
    let mut app = ImageJApp::new();

    let long_path = "/Users/nauti/very/deeply/nested/directory/structure/that/contains/an/extremely/long/path/with/lots/of/characters/test_image.tif";
    app.dialogs.last_error = Some(format!("Could not open \"{long_path}\": file not found"));

    let err_vp_id = ViewportId::from_hash_of("ij_error_dialog");
    let rendered_viewports = Rc::new(RefCell::new(Vec::new()));

    // Frame 1: render error dialog
    setup_test_viewport_renderer(
        Rc::new(RefCell::new(HashMap::new())),
        Some(rendered_viewports.clone()),
    );

    let _ = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });

    assert!(
        rendered_viewports.borrow().contains(&err_vp_id),
        "Error dialog viewport must be rendered"
    );
    assert!(app.dialogs.last_error.is_some());

    // Frame 2: send Escape to error dialog viewport to dismiss
    let per_viewport_inputs = Rc::new(RefCell::new(HashMap::new()));
    let mut err_input = make_viewport_input(err_vp_id, true);
    err_input.events.push(Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    per_viewport_inputs
        .borrow_mut()
        .insert(err_vp_id, err_input);

    setup_test_viewport_renderer(per_viewport_inputs, None);

    let _ = ctx.run(RawInput::default(), |ctx| {
        app.update_ui(ctx);
    });

    assert!(
        app.dialogs.last_error.is_none(),
        "Error dialog must dismiss upon Escape / OK"
    );
}

#[test]
fn cmd_q_and_ctrl_q_trigger_quit_and_dirty_guard() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();

    // 1. With a dirty document, Cmd+Q enters pending_close guard
    let id = app.open_ramp_8bit("cmd_q_test", 16, 16).unwrap();
    assert!(app.is_dirty(id));

    let mut cmd_q_input = RawInput::default();
    cmd_q_input.events.push(Event::Key {
        key: Key::Q,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });

    let _ = ctx.run(cmd_q_input, |ctx| {
        app.update_ui(ctx);
    });

    assert!(
        !app.quit_app,
        "Cmd+Q on dirty document must NOT immediately quit"
    );
    assert!(
        app.pending_close.is_some(),
        "Cmd+Q must activate pending_close dirty confirmation prompt"
    );
    assert_eq!(app.open_window_count(), 1, "Document retained");

    // Cancel the quit
    app.confirm_pending_close_cancel();
    assert!(app.pending_close.is_none());

    // 2. Clean up document: now app is clean
    app.close_all_windows();
    assert_eq!(app.open_window_count(), 0);

    // 3. Ctrl+Q on clean app immediately sets quit_app and outputs ViewportCommand::Close
    let mut ctrl_q_input = RawInput::default();
    ctrl_q_input.events.push(Event::Key {
        key: Key::Q,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::CTRL,
    });

    let output = ctx.run(ctrl_q_input, |ctx| {
        app.update_ui(ctx);
    });

    assert!(app.quit_app, "Ctrl+Q on clean app must set quit_app = true");
    let commands = &output.viewport_output[&ViewportId::ROOT].commands;
    assert!(
        commands.contains(&ViewportCommand::Close),
        "Ctrl+Q on clean app must output ViewportCommand::Close"
    );
    assert!(
        !commands.contains(&ViewportCommand::CancelClose),
        "Ctrl+Q on clean app must NOT output CancelClose"
    );
}
