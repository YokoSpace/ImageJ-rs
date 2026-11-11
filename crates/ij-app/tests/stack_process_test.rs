use egui::{Context, RawInput};
use ij_app::app::ImageJApp;
use ij_app::stack_process::{StackOperation, StackProcessChoice, StackProcessOutcome};
use ij_core::{ImageDocument, PixelPlane};
use ij_session::{ImageId, SessionError};

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

/// Every operation the "Process Stack?" dialog can ask about.
fn all_stack_operations() -> Vec<StackOperation> {
    vec![
        StackOperation::Invert,
        StackOperation::Smooth,
        StackOperation::Sharpen,
        StackOperation::FindEdges,
        StackOperation::GaussianBlur { sigma: 1.5 },
        StackOperation::Median { radius: 2.0 },
        StackOperation::Minimum { radius: 2.0 },
        StackOperation::Maximum { radius: 2.0 },
        StackOperation::Despeckle,
    ]
}

/// Starts the operation through the same entry point the Process menu uses.
fn invoke(app: &mut ImageJApp, operation: &StackOperation) -> Result<(), SessionError> {
    match operation {
        StackOperation::Invert => app.invert_active(),
        StackOperation::Smooth => app.smooth_active(),
        StackOperation::Sharpen => app.sharpen_active(),
        StackOperation::FindEdges => app.find_edges_active(),
        StackOperation::GaussianBlur { sigma } => app.gaussian_blur_active(*sigma),
        StackOperation::Median { radius } => app.median_active(*radius),
        StackOperation::Minimum { radius } => app.minimum_active(*radius),
        StackOperation::Maximum { radius } => app.maximum_active(*radius),
        StackOperation::Despeckle => app.despeckle_active(),
        StackOperation::Math(operation) => app.math_active(*operation),
        StackOperation::Binary(ij_core::binary_morphology::BinaryMorphology::Erode) => {
            app.binary_erode_active()
        }
        StackOperation::Binary(ij_core::binary_morphology::BinaryMorphology::Dilate) => {
            app.binary_dilate_active()
        }
        StackOperation::Binary(ij_core::binary_morphology::BinaryMorphology::Open) => {
            app.binary_open_active()
        }
        StackOperation::Binary(ij_core::binary_morphology::BinaryMorphology::Close) => {
            app.binary_close_active()
        }
    }
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
fn every_stack_operation_offers_whole_stack_processing() {
    for operation in all_stack_operations() {
        assert!(
            operation.supports_whole_stack(),
            "{} must support whole-stack processing",
            operation.display_name()
        );
        assert!(
            operation.whole_stack_command().is_some(),
            "{} must map to a whole-stack command",
            operation.display_name()
        );
        assert!(
            operation.whole_stack_limitation().is_none(),
            "{} must not report a whole-stack limitation",
            operation.display_name()
        );
    }
}

#[test]
fn every_stack_operation_applies_choice_all_to_every_slice() {
    for operation in all_stack_operations() {
        let (mut app, id, original) = stack_fixture();
        invoke(&mut app, &operation).unwrap();
        assert!(app.pending_stack_process.is_some());

        let outcome = app.resolve_stack_process(StackProcessChoice::All).unwrap();
        assert_eq!(
            outcome,
            StackProcessOutcome::Applied,
            "{} must accept the Yes answer",
            operation.display_name()
        );
        assert!(app.pending_stack_process.is_none());

        let doc = app.session.get_image(id).unwrap();
        assert_eq!(
            doc.current_slice,
            2,
            "{} must preserve the selected slice",
            operation.display_name()
        );
        for (index, (after, before)) in doc.slices.iter().zip(original.iter()).enumerate() {
            assert_ne!(
                after,
                before,
                "{} must filter slice {}",
                operation.display_name(),
                index + 1
            );
        }

        // One recorder run with the `stack` option, exactly like
        // IJ.setupDialog's "Yes" answer (IJ.java 1287).
        assert_eq!(
            app.session.macro_recorder.len(),
            1,
            "{} must record exactly one command",
            operation.display_name()
        );
        let recorded = &app.session.macro_recorder[0];
        assert!(
            recorded.contains("stack"),
            "{} must record the stack option, got {recorded}",
            operation.display_name()
        );
        assert!(app.is_dirty(id));

        // PlugInFilterRunner calls Undo.reset() for a complete stack
        // (PlugInFilterRunner.java 118-120).
        assert_eq!(
            app.undo_active().unwrap(),
            ij_session::CommandOutcome::NothingToUndo,
            "{} must not leave an undo snapshot",
            operation.display_name()
        );
    }
}

#[test]
fn every_stack_operation_current_slice_choice_only_filters_the_selected_slice() {
    for operation in all_stack_operations() {
        let (mut app, id, original) = stack_fixture();
        invoke(&mut app, &operation).unwrap();

        let outcome = app
            .resolve_stack_process(StackProcessChoice::Current)
            .unwrap();
        assert_eq!(outcome, StackProcessOutcome::Applied);

        let doc = app.session.get_image(id).unwrap();
        assert_eq!(doc.current_slice, 2);
        assert_eq!(doc.slices[0], original[0]);
        assert_ne!(doc.slices[1], original[1]);
        assert_eq!(doc.slices[2], original[2]);

        assert_eq!(app.session.macro_recorder.len(), 1);
        let recorded = &app.session.macro_recorder[0];
        assert!(
            recorded.contains("slice"),
            "{} must record the slice option, got {recorded}",
            operation.display_name()
        );
        assert!(!recorded.contains("stack"));
    }
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

/// Erode on a binary stack follows the same prompt path as the other
/// Process filters, keeps no undo, and erodes every slice.
#[test]
fn stack_binary_erode_prompts_yes_processes_whole_stack_without_undo() {
    let plane = || {
        let mut pixels = vec![255u8; 64];
        for y in 1..4 {
            for x in 1..4 {
                pixels[y * 8 + x] = 0;
            }
        }
        PixelPlane::Gray8(pixels)
    };
    let original = vec![plane(), plane()];
    let mut app = ImageJApp::new();
    let id = app
        .session
        .insert_doc(ImageDocument::new_stack("binary stack", 8, 8, original.clone()).unwrap());
    app.set_active_window(id).unwrap();

    app.binary_erode_active().unwrap();
    assert!(app.pending_stack_process.is_some());
    assert_eq!(app.status_message, "Process all 2 images?");
    assert_eq!(app.session.get_image(id).unwrap().slices, original);

    let outcome = app.resolve_stack_process(StackProcessChoice::All).unwrap();
    assert_eq!(outcome, StackProcessOutcome::Applied);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Erode\", \"stack\");")
    );

    // count=1 erode with the black-background default: '.'=0=background,
    // '#'=255=foreground. getEdgePixel0 pads out-of-image neighbours with the
    // background value (ByteProcessor.java 797), so the border ring turns
    // black and every white pixel touching it or the rectangle is eroded.
    let art = [
        "........",
        ".....##.",
        ".....##.",
        ".....##.",
        ".....##.",
        ".######.",
        ".######.",
        "........",
    ];
    let expected_pixels: Vec<u8> = art
        .iter()
        .flat_map(|row| row.bytes().map(|b| if b == b'.' { 0 } else { 255 }))
        .collect();
    let expected = PixelPlane::Gray8(expected_pixels);
    assert_eq!(
        app.session.get_image(id).unwrap().slices,
        vec![expected.clone(), expected]
    );
    // Whole-stack runs drop the undo snapshot (PlugInFilterRunner.java 126).
    assert_eq!(
        app.session.dispatch(ij_session::Command::Undo).unwrap(),
        ij_session::CommandOutcome::NothingToUndo
    );
}
