//! UI-level regression tests for review findings: threshold reset, Undo with
//! no snapshot, interactive slice navigation, and the close guard for metadata
//! that is persisted in TIFF.

use ij_app::app::{CloseTarget, ImageJApp};
use ij_core::{ImageDocument, PixelPlane};
use ij_io::save_tiff;
use ij_session::{Command, CommandOutcome};

fn stack_doc() -> ImageDocument {
    ImageDocument::new_stack(
        "stack",
        4,
        4,
        vec![
            PixelPlane::Gray8(vec![10; 16]),
            PixelPlane::Gray8(vec![20; 16]),
            PixelPlane::Gray8(vec![30; 16]),
        ],
    )
    .unwrap()
}

#[test]
fn roi_edit_on_clean_tiff_requires_save_confirmation_before_close() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roi.tif");
    let mut doc = ImageDocument::new_ramp_8bit("roi", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    save_tiff(&path, &doc).unwrap();

    let mut app = ImageJApp::new();
    let id = app.open_tiff_file(path).unwrap();
    assert!(!app.is_dirty(id));
    app.dispatch_rect_roi(0, 0, 4, 4).unwrap();
    assert!(app.is_dirty(id));
    assert!(!app.request_close_window(id));
    assert_eq!(app.pending_close.as_ref().unwrap().target, CloseTarget::Single(id));
    app.confirm_pending_close_cancel();
    assert!(app.session.get_image(id).is_some());
}

#[test]
fn reset_threshold_goes_through_session_and_records_reset_threshold() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("threshold", 16, 16).unwrap();
    app.auto_threshold_active("IsoData", false).unwrap();
    assert!(app.session.get_image(id).unwrap().threshold.is_some());

    let before = app.session.macro_recorder.len();
    let outcome = app.reset_threshold_active().unwrap();

    assert_eq!(outcome, CommandOutcome::ThresholdReset);
    assert!(app.session.get_image(id).unwrap().threshold.is_none());
    assert_eq!(app.status_message, "Threshold reset");
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("resetThreshold();")
    );
    assert_eq!(app.session.macro_recorder.len(), before + 1);
}

#[test]
fn undo_without_snapshot_reports_nothing_to_undo_without_dirtying() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("undo_noop", 16, 16).unwrap();
    app.session.set_dirty(id, false);
    let macros_before = app.session.macro_recorder.len();

    let outcome = app.undo_active().unwrap();

    assert_eq!(outcome, CommandOutcome::NothingToUndo);
    assert_eq!(app.status_message, "Nothing to undo");
    assert!(!app.is_dirty(id), "no-op Undo must not dirty the document");
    assert!(
        app.dialogs.last_error.is_none(),
        "no-op Undo is not an error and must not open an error dialog"
    );
    assert_eq!(app.session.macro_recorder.len(), macros_before);
}

#[test]
fn slice_slider_navigation_does_not_record_a_macro() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(stack_doc());
    app.set_active_window(id).unwrap();
    let macros_before = app.session.macro_recorder.len();

    app.set_slice_active(2).unwrap();

    assert_eq!(app.session.get_image(id).unwrap().current_slice, 2);
    assert_eq!(app.status_message, "Slice 2");
    assert_eq!(
        app.session.macro_recorder.len(),
        macros_before,
        "interactive slider movement is not a recorded ImageJ command"
    );
}

#[test]
fn close_guard_prompts_after_display_range_change_on_clean_tiff() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("display_range.tif");
    let doc = ImageDocument::new_ramp_8bit("display_range", 16, 16).unwrap();
    save_tiff(&path, &doc).unwrap();

    let mut app = ImageJApp::new();
    let id = app.open_tiff_file(path).unwrap();
    assert!(!app.is_dirty(id));

    app.set_display_range_active(5.0, 200.0).unwrap();
    assert!(
        app.is_dirty(id),
        "display range is persisted in the TIFF description"
    );

    assert!(
        !app.request_close_window(id),
        "close must be guarded after a persisted metadata change"
    );
    assert_eq!(
        app.pending_close.as_ref().unwrap().target,
        CloseTarget::Single(id)
    );

    app.confirm_pending_close_cancel();
    assert_eq!(app.open_window_count(), 1);
    assert!(app.session.get_image(id).is_some());
}

#[test]
fn close_guard_prompts_after_dimension_change_on_clean_tiff() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dimensions.tif");
    save_tiff(&path, &stack_doc()).unwrap();

    let mut app = ImageJApp::new();
    let id = app.open_tiff_file(path).unwrap();
    assert!(!app.is_dirty(id));

    app.session
        .dispatch(Command::SetDimensions {
            channels: 1,
            slices: 1,
            frames: 3,
        })
        .unwrap();
    assert!(
        app.is_dirty(id),
        "C/Z/T dimensions are persisted in the TIFF description"
    );

    assert!(
        !app.request_close_window(id),
        "close must be guarded after a persisted metadata change"
    );
    assert_eq!(
        app.pending_close.as_ref().unwrap().target,
        CloseTarget::Single(id)
    );
}

#[test]
fn reapplying_the_same_display_range_does_not_dirty_a_clean_tiff() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("same_range.tif");
    let mut doc = ImageDocument::new_ramp_8bit("same_range", 16, 16).unwrap();
    doc.display_range = ij_core::DisplayRange {
        min: 5.0,
        max: 200.0,
    };
    save_tiff(&path, &doc).unwrap();

    let mut app = ImageJApp::new();
    let id = app.open_tiff_file(path).unwrap();
    assert!(!app.is_dirty(id));

    app.set_display_range_active(5.0, 200.0).unwrap();
    assert!(
        !app.is_dirty(id),
        "setting the same display range must not create phantom changes"
    );

    assert!(
        app.request_close_window(id),
        "a clean document must still close without a prompt"
    );
}
