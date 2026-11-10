//! Session-level correctness for UI-facing commands: threshold reset, Undo
//! with no snapshot, interactive slice navigation, and dirty tracking for
//! metadata that ImageJ persists in the TIFF ImageDescription.

use ij_core::{ImageDocument, PixelPlane, RectRoi, Roi};
use ij_io::{load_tiff, save_tiff};
use ij_session::{Command, CommandOutcome, Session};

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
fn roi_edits_and_clears_dirty_only_when_the_saved_selection_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roi.tif");
    let mut doc = ImageDocument::new_ramp_8bit("roi", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    save_tiff(&path, &doc).unwrap();

    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path: path.clone() }).unwrap();
    let id = session.active_image_id().unwrap();
    assert!(!session.is_dirty(id));

    session.dispatch(Command::MakeRectRoi { x: 0, y: 0, width: 4, height: 4 }).unwrap();
    assert!(session.is_dirty(id));
    session.dispatch(Command::SaveTiff { path: path.clone() }).unwrap();
    assert!(!session.is_dirty(id));
    assert_eq!(load_tiff(&path).unwrap().roi, Some(Roi::Rect(RectRoi { x: 0, y: 0, width: 4, height: 4 })));

    session.dispatch(Command::MakeRectRoi { x: 0, y: 0, width: 4, height: 4 }).unwrap();
    assert!(!session.is_dirty(id));

    session.dispatch(Command::ClearRoi).unwrap();
    assert!(session.is_dirty(id));
    session.dispatch(Command::SaveTiff { path: path.clone() }).unwrap();
    assert!(!session.is_dirty(id));
    assert!(load_tiff(&path).unwrap().roi.is_none());
    session.dispatch(Command::ClearRoi).unwrap();
    assert!(!session.is_dirty(id));
}

#[test]
fn reset_threshold_is_a_session_command_that_records_the_macro() {
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "threshold".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    session
        .dispatch(Command::AutoThreshold {
            method: "IsoData".into(),
            dark_background: false,
            histogram_16: false,
        })
        .unwrap();
    assert!(session.active_doc().unwrap().threshold.is_some());

    let before = session.macro_recorder.len();
    let outcome = session.dispatch(Command::ResetThreshold).unwrap();

    assert_eq!(outcome, CommandOutcome::ThresholdReset);
    assert!(
        session.active_doc().unwrap().threshold.is_none(),
        "ResetThreshold must clear ImageProcessor.NO_THRESHOLD state"
    );
    assert_eq!(
        &session.macro_recorder[before..],
        &["resetThreshold();".to_string()],
        "ThresholdAdjuster.reset records resetThreshold (ThresholdAdjuster.java 740-745)"
    );
}

#[test]
fn reset_threshold_without_an_active_image_is_an_error() {
    let mut session = Session::new();
    assert!(session.dispatch(Command::ResetThreshold).is_err());
}

#[test]
fn undo_without_snapshot_is_reported_and_does_not_dirty_the_image() {
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "undo".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    let id = session.active_image_id().unwrap();
    session.set_dirty(id, false);
    let macros_before = session.macro_recorder.len();

    let outcome = session.dispatch(Command::Undo).unwrap();

    assert_eq!(
        outcome,
        CommandOutcome::NothingToUndo,
        "Undo.undo does nothing when no snapshot exists (Undo.java 121-132)"
    );
    assert!(
        !session.is_dirty(id),
        "a no-op Undo must not report unsaved changes"
    );
    assert_eq!(
        session.macro_recorder.len(),
        macros_before,
        "a no-op Undo must not append an invented macro record"
    );
}

#[test]
fn undo_with_snapshot_reports_success_and_marks_dirty() {
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "undo_success".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    session.dispatch(Command::Invert).unwrap();
    let id = session.active_image_id().unwrap();
    session.set_dirty(id, false);

    let outcome = session.dispatch(Command::Undo).unwrap();

    assert_eq!(outcome, CommandOutcome::Undone);
    assert!(session.is_dirty(id));
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Undo\");")
    );
}

#[test]
fn interactive_slice_navigation_does_not_record_set_slice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stack.tif");
    save_tiff(&path, &stack_doc()).unwrap();

    let mut session = Session::new();
    session
        .dispatch(Command::OpenTiff { path: path.clone() })
        .unwrap();
    let macros_before = session.macro_recorder.len();

    session
        .dispatch(Command::NavigateSlice { index: 2 })
        .unwrap();
    assert_eq!(session.active_doc().unwrap().current_slice, 2);
    assert_eq!(
        session.macro_recorder.len(),
        macros_before,
        "slider navigation must not record a setSlice macro"
    );

    session.dispatch(Command::SetSlice { index: 3 }).unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("setSlice(3);"),
        "the explicit macro function still records setSlice"
    );
}

#[test]
fn persisted_display_range_and_dimensions_mark_only_changed_documents_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metadata.tif");
    save_tiff(&path, &stack_doc()).unwrap();

    let mut session = Session::new();
    session
        .dispatch(Command::OpenTiff { path: path.clone() })
        .unwrap();
    let id = session.active_image_id().unwrap();
    assert!(!session.is_dirty(id));

    // Display range is written to the TIFF ImageDescription, so a real change
    // makes the in-memory document differ from what is on disk.
    session
        .dispatch(Command::SetDisplayRange {
            min: 1.0,
            max: 200.0,
        })
        .unwrap();
    assert!(session.is_dirty(id), "changed display range must be dirty");

    session.save_image(id, &path).unwrap();
    assert!(!session.is_dirty(id));
    session
        .dispatch(Command::SetDisplayRange {
            min: 1.0,
            max: 200.0,
        })
        .unwrap();
    assert!(
        !session.is_dirty(id),
        "re-applying the same display range must not dirty the image"
    );

    // C/Z/T dimensions are written as channels/slices/frames in the same
    // ImageDescription.
    session
        .dispatch(Command::SetDimensions {
            channels: 1,
            slices: 1,
            frames: 3,
        })
        .unwrap();
    assert!(session.is_dirty(id), "changed dimensions must be dirty");

    session.save_image(id, &path).unwrap();
    assert!(!session.is_dirty(id));
    session
        .dispatch(Command::SetDimensions {
            channels: 1,
            slices: 1,
            frames: 3,
        })
        .unwrap();
    assert!(
        !session.is_dirty(id),
        "re-applying the same dimensions must not dirty the image"
    );
}

#[test]
fn slice_navigation_does_not_dirty_a_clean_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("navigation.tif");
    save_tiff(&path, &stack_doc()).unwrap();

    let mut session = Session::new();
    session
        .dispatch(Command::OpenTiff { path: path.clone() })
        .unwrap();
    let id = session.active_image_id().unwrap();

    session
        .dispatch(Command::NavigateSlice { index: 3 })
        .unwrap();

    assert_eq!(session.active_doc().unwrap().current_slice, 3);
    assert!(
        !session.is_dirty(id),
        "the displayed stack index is not persisted in the TIFF description"
    );
}
