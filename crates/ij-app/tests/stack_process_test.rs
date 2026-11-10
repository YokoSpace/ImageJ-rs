use egui::{Context, RawInput};
use ij_app::app::ImageJApp;
use ij_app::stack_process::{StackOperation, StackProcessChoice, StackProcessOutcome};
use ij_core::{ImageDocument, PixelPlane};
use ij_session::ImageId;

/// Three 4x4 slices with distinct ramps, current slice set to 2, clean state.
fn stack_fixture() -> (ImageJApp, ImageId, Vec<PixelPlane>) {
    let planes: Vec<PixelPlane> = (0..3u8)
        .map(|slice| {
            PixelPlane::Gray8(
                (0..16u8)
                    .map(|index| index.wrapping_mul(13).wrapping_add(slice.wrapping_mul(40)))
                    .collect(),
            )
        })
        .collect();
    let mut doc = ImageDocument::new_stack("three-slice stack", 4, 4, planes.clone()).unwrap();
    doc.set_slice(2).unwrap();

    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();
    assert!(!app.is_dirty(id));
    (app, id, planes)
}

fn gray8(plane: &PixelPlane) -> &[u8] {
    match plane {
        PixelPlane::Gray8(pixels) => pixels,
        _ => panic!("expected Gray8 plane"),
    }
}

fn inverted(plane: &PixelPlane) -> PixelPlane {
    PixelPlane::Gray8(gray8(plane).iter().map(|&p| 255 - p).collect())
}

#[test]
fn stack_invert_prompts_and_yes_processes_the_whole_stack() {
    let (mut app, id, original) = stack_fixture();
    assert!(app.session.macro_recorder.is_empty());

    app.invert_active().unwrap();

    // Nothing is processed before the user answers the ImageJ prompt.
    assert!(app.pending_stack_process.is_some());
    assert_eq!(app.status_message, "Process all 3 images?");
    assert_eq!(app.session.get_image(id).unwrap().slices, original);
    assert!(!app.is_dirty(id));
    assert!(app.session.macro_recorder.is_empty());

    let outcome = app.resolve_stack_process(StackProcessChoice::All).unwrap();
    assert_eq!(outcome, StackProcessOutcome::Applied);
    assert!(app.pending_stack_process.is_none());

    let expected: Vec<PixelPlane> = original.iter().map(inverted).collect();
    assert_eq!(app.session.get_image(id).unwrap().slices, expected);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Invert\", \"stack\");")
    );
    assert!(app.is_dirty(id));
}

#[test]
fn stack_invert_no_processes_only_the_current_slice_and_records_slice() {
    let (mut app, id, original) = stack_fixture();

    app.invert_active().unwrap();
    let outcome = app
        .resolve_stack_process(StackProcessChoice::Current)
        .unwrap();
    assert_eq!(outcome, StackProcessOutcome::Applied);

    let expected = vec![
        original[0].clone(),
        inverted(&original[1]),
        original[2].clone(),
    ];
    assert_eq!(app.session.get_image(id).unwrap().slices, expected);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Invert\", \"slice\");")
    );
    assert!(app.is_dirty(id));
}

#[test]
fn stack_cancel_leaves_pixels_and_dirty_state_untouched() {
    let (mut app, id, original) = stack_fixture();

    app.smooth_active().unwrap();
    assert!(app.pending_stack_process.is_some());
    assert!(app.session.macro_recorder.is_empty());
    assert!(!app.is_dirty(id));

    let outcome = app
        .resolve_stack_process(StackProcessChoice::Cancel)
        .unwrap();
    assert_eq!(outcome, StackProcessOutcome::Cancelled);
    assert!(app.pending_stack_process.is_none());
    assert_eq!(app.session.get_image(id).unwrap().slices, original);
    assert!(!app.is_dirty(id));
    assert!(app.session.macro_recorder.is_empty());
    assert_eq!(app.status_message, "Process stack canceled");
}

#[test]
fn unimplemented_whole_stack_filter_refuses_yes_and_requires_a_choice() {
    let (mut app, id, original) = stack_fixture();

    app.smooth_active().unwrap();
    let outcome = app.resolve_stack_process(StackProcessChoice::All).unwrap();
    assert_eq!(outcome, StackProcessOutcome::UnsupportedWholeStack);

    // No silent fallback: pixels, macro and dirty flag stay untouched and the
    // question remains open for an explicit Current Slice or Cancel.
    assert!(app.pending_stack_process.is_some());
    assert_eq!(app.session.get_image(id).unwrap().slices, original);
    assert!(!app.is_dirty(id));
    assert!(app.session.macro_recorder.is_empty());
    assert!(app.status_message.contains("not implemented"));

    let outcome = app
        .resolve_stack_process(StackProcessChoice::Current)
        .unwrap();
    assert_eq!(outcome, StackProcessOutcome::Applied);
    assert!(app.pending_stack_process.is_none());

    let slices = &app.session.get_image(id).unwrap().slices;
    assert_eq!(slices[0], original[0]);
    assert_ne!(slices[1], original[1]);
    assert_eq!(slices[2], original[2]);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Smooth\", \"slice\");")
    );
}

#[test]
fn gaussian_stack_choice_keeps_parameters_and_records_the_slice_option() {
    let (mut app, id, original) = stack_fixture();

    app.gaussian_blur_active(1.5).unwrap();
    let pending = app.pending_stack_process.as_ref().unwrap();
    assert_eq!(
        pending.operation,
        StackOperation::GaussianBlur { sigma: 1.5 }
    );

    app.resolve_stack_process(StackProcessChoice::Current)
        .unwrap();
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Gaussian Blur...\", \"sigma=1.5 slice\");")
    );
    let slices = &app.session.get_image(id).unwrap().slices;
    assert_eq!(slices[0], original[0]);
    assert_eq!(slices[2], original[2]);
}

#[test]
fn single_slice_filters_do_not_prompt_and_record_plain_macros() {
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("single", 8, 8).unwrap();

    app.smooth_active().unwrap();
    assert!(app.pending_stack_process.is_none());
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Smooth\");")
    );
}

#[test]
fn switching_active_image_discards_the_pending_answer() {
    let (mut app, stack_id, original) = stack_fixture();
    app.invert_active().unwrap();

    let other_id = app.open_ramp_8bit("other", 4, 4).unwrap();
    let other_before = app.session.get_image(other_id).unwrap().slices.clone();

    let outcome = app.resolve_stack_process(StackProcessChoice::All).unwrap();
    assert_eq!(outcome, StackProcessOutcome::Stale);
    assert!(app.pending_stack_process.is_none());
    assert_eq!(app.session.get_image(stack_id).unwrap().slices, original);
    assert_eq!(
        app.session.get_image(other_id).unwrap().slices,
        other_before
    );
    assert!(
        !app.session
            .macro_recorder
            .iter()
            .any(|m| m.contains("Invert"))
    );
}

#[test]
fn process_stack_dialog_renders_without_processing_until_answered() {
    let (mut app, id, original) = stack_fixture();
    app.invert_active().unwrap();

    let ctx = Context::default();
    let output = ctx.run(RawInput::default(), |ctx| {
        app.render_dialogs(ctx);
    });

    assert!(
        !output.shapes.is_empty(),
        "Process Stack? dialog must render its modal window"
    );
    assert!(app.pending_stack_process.is_some());
    assert_eq!(app.session.get_image(id).unwrap().slices, original);
}
