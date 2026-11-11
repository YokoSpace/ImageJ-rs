use egui::{Context, RawInput};
use ij_app::app::ImageJApp;
use ij_app::action::AppAction;
use ij_core::{ImageDocument, PixelPlane};
use ij_session::ImageId;

/// Three 8x8 bimodal Gray8 slices, current slice set to 2.
fn stack_fixture() -> (ImageJApp, ImageId, Vec<PixelPlane>) {
    let mut slices = Vec::new();
    for (bg, fg) in [(20u8, 200u8), (40, 120), (60, 230)] {
        let pixels = (0..64)
            .map(|index| {
                let x = index % 8;
                let y = index / 8;
                if (2..6).contains(&x) && (2..6).contains(&y) { fg } else { bg }
            })
            .collect();
        slices.push(PixelPlane::Gray8(pixels));
    }
    let mut doc = ImageDocument::new_stack("stack", 8, 8, slices.clone()).unwrap();
    doc.set_slice(2).unwrap();

    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();
    (app, id, slices)
}

/// Drives one frame of dialog rendering, like the desktop shell's update.
fn tick(app: &mut ImageJApp) {
    let ctx = Context::default();
    let _output = ctx.run(RawInput::default(), |ctx| {
        app.render_dialogs(ctx);
    });
}

#[test]
fn stack_binary_menu_action_opens_dialog_before_touching_pixels() {
    let (mut app, id, original) = stack_fixture();

    app.perform(AppAction::MakeBinary, &Context::default());

    let pending = app.dialogs.pending_stack_binary.as_ref().unwrap();
    assert_eq!(pending.macro_command, "Make Binary");
    // GenericDialog defaults (Thresholder.java 82-90 with staticUseLocal).
    assert_eq!(pending.options.method, "Default");
    assert_eq!(pending.options.background, "Default");
    assert!(pending.options.calculate, "staticUseLocal defaults true");
    assert!(!pending.options.only_current);
    assert!(!pending.black_background, "Prefs.blackBackground default false");

    let settled = app.session.get_image(id).unwrap();
    assert_eq!(settled.slices, original, "nothing runs before OK");
    assert!(!app.is_dirty(id));
    assert!(app.session.macro_recorder.is_empty());
}

#[test]
fn stack_binary_dialog_ok_converts_and_records() {
    let (mut app, id, _) = stack_fixture();
    app.perform(AppAction::MakeBinary, &Context::default());
    assert!(app.dialogs.pending_stack_binary.is_some());

    app.resolve_stack_binary_dialog(true);

    let settled = app.session.get_image(id).unwrap();
    let all_binary = settled.slices.iter().all(|plane| match plane {
        PixelPlane::Gray8(pixels) => pixels.iter().all(|&p| p == 0 || p == 255),
        _ => false,
    });
    assert!(all_binary, "all slices became 0/255 masks");
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        // Only "calculate" differs from the GenericDialog defaults.
        Some("run(\"Make Binary\", \"calculate\");")
    );
    assert!(app.is_dirty(id));
    assert_eq!(settled.current_slice, 2);
    assert!(app.dialogs.pending_stack_binary.is_none());
}

#[test]
fn stack_binary_dialog_cancel_touches_nothing() {
    let (mut app, id, original) = stack_fixture();
    app.perform(AppAction::MakeBinary, &Context::default());

    app.resolve_stack_binary_dialog(false);

    assert_eq!(app.session.get_image(id).unwrap().slices, original);
    assert!(!app.is_dirty(id));
    assert!(app.session.macro_recorder.is_empty());
    assert!(app.dialogs.pending_stack_binary.is_none());
    assert_eq!(app.status_message, "Convert Stack to Binary canceled");
}

#[test]
fn stack_binary_dialog_renders_through_the_dialog_pipeline() {
    let (mut app, _id, _original) = stack_fixture();
    app.perform(AppAction::MakeBinary, &Context::default());

    // Render one frame with the dialog pending; the dialog must still be
    // open and the application unchanged (no panic, no early dispatch).
    tick(&mut app);
    assert!(app.dialogs.pending_stack_binary.is_some());
    assert!(app.session.macro_recorder.is_empty());
}

#[test]
fn palette_apply_on_a_stack_prefills_adjuster_method_and_background() {
    let (mut app, _id, _original) = stack_fixture();
    // ThresholdAdjuster.runThresholdCommand does
    // Thresholder.setMethod(method) and setBackground(dark? "Dark":"Light")
    // (ThresholdAdjuster.java 859-868) before the Thresholder run, so the
    // Convert Stack to Binary dialog pre-fills from the adjuster.
    app.dialogs.threshold_method = "Otsu".into();
    app.dialogs.threshold_dark_bg = true;
    app.convert_to_mask_active(app.dialogs.threshold_dark_bg).unwrap();

    let pending = app.dialogs.pending_stack_binary.as_ref().unwrap();
    assert_eq!(pending.macro_command, "Convert to Mask");
    assert_eq!(pending.options.method, "Otsu");
    assert_eq!(pending.options.background, "Dark");
    assert!(pending.options.calculate);
}

#[test]
fn single_image_make_binary_runs_without_the_stack_dialog() {
    let mut app = ImageJApp::new();
    let doc = ImageDocument::new(
        "single",
        8,
        8,
        PixelPlane::Gray8(vec![20; 64]),
    )
    .unwrap();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();

    app.perform(AppAction::MakeBinary, &Context::default());

    assert!(app.dialogs.pending_stack_binary.is_none());
    assert_eq!(app.session.macro_recorder.last().map(String::as_str), Some("run(\"Make Binary\");"));
}
