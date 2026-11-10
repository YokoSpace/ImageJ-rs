//! Save-safety regression tests for the close/quit prompt.
//!
//! ImageJ's `FileSaver.save()` re-saves a document opened from a TIFF at its
//! existing path instead of showing a Save dialog (original_imagej/ij/io/
//! FileSaver.java 42-69). The close prompt must follow that route and must keep
//! the prompt, document and dirty state alive when the save is cancelled or
//! fails.

use ij_app::app::ImageJApp;
use ij_core::ImageDocument;
use ij_io::{load_tiff, save_tiff};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn write_ramp(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    let doc = ImageDocument::new_ramp_8bit(name, 16, 16).unwrap();
    save_tiff(&path, &doc).unwrap();
    path
}

#[test]
fn cancelled_close_save_retains_prompt_document_and_dirty_state() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("cancelled", 16, 16).unwrap();
    assert!(app.is_dirty(id));

    assert!(!app.request_close_window(id));
    let pending_before = app.pending_close.clone().expect("prompt must be open");
    let macros_before = app.session.macro_recorder.clone();

    // The user pressed Save in the close prompt and then cancelled the picker.
    let finished = app.confirm_pending_close_save(None).unwrap();

    assert!(
        !finished,
        "cancelled save must not finish the close sequence"
    );
    assert_eq!(
        app.pending_close.as_ref(),
        Some(&pending_before),
        "the close prompt must survive a cancelled save"
    );
    assert_eq!(app.open_window_count(), 1);
    assert!(app.session.get_image(id).is_some());
    assert!(app.is_dirty(id), "cancelled save must keep dirty state");
    assert_eq!(
        app.session.macro_recorder, macros_before,
        "a cancelled save must not record a macro"
    );
    assert!(app.status_message.contains("Save cancelled"));
}

#[test]
fn failed_close_save_retains_prompt_document_dirty_state_and_recorder() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("failed", 16, 16).unwrap();
    assert!(app.is_dirty(id));

    assert!(!app.request_close_window(id));
    let pending_before = app.pending_close.clone().expect("prompt must be open");
    let macros_before = app.session.macro_recorder.clone();

    let bad_path = dir.path().join("missing_parent").join("impossible.tif");
    let err = app
        .confirm_pending_close_save(Some(bad_path))
        .expect_err("writing into a missing directory must fail");

    assert!(matches!(err, ij_session::SessionError::Io(_)));
    assert_eq!(
        app.pending_close.as_ref(),
        Some(&pending_before),
        "the close prompt must survive a failed save"
    );
    assert_eq!(app.open_window_count(), 1);
    assert!(app.session.get_image(id).is_some());
    assert!(app.is_dirty(id), "a failed save must keep dirty state");
    assert_eq!(
        app.session.macro_recorder, macros_before,
        "a failed save must not record a save macro"
    );
    assert!(app.status_message.contains("Save failed"));
    assert!(app.dialogs.last_error.is_some());
}

#[test]
fn opened_tiff_close_save_writes_existing_destination_directly() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ramp(&dir, "existing.tif");
    let original = load_tiff(&path).unwrap();

    let mut app = ImageJApp::new();
    let id = app.open_tiff_file(path.clone()).unwrap();
    assert!(!app.is_dirty(id));

    app.invert_active().unwrap();
    assert!(app.is_dirty(id));
    let inverted = app.session.get_image(id).unwrap().clone();
    assert_ne!(
        inverted.plane().sample_f64(0),
        original.plane().sample_f64(0)
    );

    assert!(!app.request_close_window(id));

    // No picker: the remembered source path resolves the prompt directly.
    let outcome = app
        .save_pending_close_to_remembered_path()
        .expect("an opened TIFF must have a remembered destination");
    assert!(outcome.expect("direct save must succeed"));

    assert_eq!(app.open_window_count(), 0);
    assert!(app.session.get_image(id).is_none());
    assert!(
        !app.pending_close
            .as_ref()
            .is_some_and(|p| p.current_prompt == Some(id))
    );

    // The existing file was rewritten, not duplicated with a ".tif.tif" name.
    let reopened = load_tiff(&path).unwrap();
    assert_eq!(
        reopened.plane().sample_f64(0),
        inverted.plane().sample_f64(0),
        "close-save must write the existing TIFF destination in place"
    );
    assert!(
        fs::read_dir(dir.path())
            .unwrap()
            .all(|entry| entry.unwrap().path() == path),
        "close-save must not create an additional .tif.tif file"
    );
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some(format!("saveAs(\"Tiff\", \"{}\");", path.display()).as_str())
    );
}

#[test]
fn unsaved_image_close_save_still_needs_a_save_dialog() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("untitled_ramp", 16, 16).unwrap();
    assert!(!app.request_close_window(id));

    assert!(
        app.save_pending_close_to_remembered_path().is_none(),
        "a new unsaved image must still go through the Save As picker"
    );
    assert_eq!(app.open_window_count(), 1);
    assert!(app.pending_close.is_some());
    assert!(app.is_dirty(id));
}

#[test]
fn save_as_updates_the_remembered_destination_for_a_later_close_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("save_as", 16, 16).unwrap();
    app.invert_active().unwrap();

    let destination = dir.path().join("saved_as.tif");
    app.save_active_tiff(destination.clone()).unwrap();
    assert!(!app.is_dirty(id));

    app.invert_active().unwrap();
    assert!(app.is_dirty(id));
    assert!(!app.request_close_window(id));

    let outcome = app
        .save_pending_close_to_remembered_path()
        .expect("Save As must remember the new destination");
    assert!(outcome.expect("close-save must succeed"));

    assert_eq!(app.open_window_count(), 0);
    let reopened = load_tiff(&destination).unwrap();
    let expected = ImageDocument::new_ramp_8bit("save_as", 16, 16)
        .unwrap()
        .plane()
        .sample_f64(0);
    // Inverted twice: back to the original ramp value.
    assert_eq!(reopened.plane().sample_f64(0), expected);
}

#[test]
fn close_all_saves_each_dirty_document_to_its_own_remembered_path() {
    let dir = tempfile::tempdir().unwrap();
    let path_a = write_ramp(&dir, "a.tif");
    let path_b = write_ramp(&dir, "b.tif");

    let mut app = ImageJApp::new();
    let id_a = app.open_tiff_file(path_a.clone()).unwrap();
    let id_b = app.open_tiff_file(path_b.clone()).unwrap();
    app.set_active_window(id_a).unwrap();
    app.invert_active().unwrap();
    app.set_active_window(id_b).unwrap();
    app.invert_active().unwrap();

    assert!(!app.request_close_all());
    assert_eq!(
        app.pending_close.as_ref().unwrap().current_prompt,
        Some(id_a)
    );

    // First prompt: save A in place, which advances to B.
    let first = app
        .save_pending_close_to_remembered_path()
        .expect("A has a remembered path")
        .unwrap();
    assert!(!first);
    assert_eq!(
        app.pending_close.as_ref().unwrap().current_prompt,
        Some(id_b)
    );

    let second = app
        .save_pending_close_to_remembered_path()
        .expect("B has a remembered path")
        .unwrap();
    assert!(second);

    assert_eq!(app.open_window_count(), 0);
    assert!(app.pending_close.is_none());
    assert!(app.session.get_image(id_a).is_none());
    assert!(app.session.get_image(id_b).is_none());

    // Both existing paths were rewritten and no extra .tif files appeared.
    assert_ne!(
        load_tiff(&path_a).unwrap().plane().sample_f64(0),
        ImageDocument::new_ramp_8bit("a", 16, 16)
            .unwrap()
            .plane()
            .sample_f64(0)
    );
    assert_ne!(
        load_tiff(&path_b).unwrap().plane().sample_f64(0),
        ImageDocument::new_ramp_8bit("b", 16, 16)
            .unwrap()
            .plane()
            .sample_f64(0)
    );
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2, "no duplicate .tif.tif files: {names:?}");
}

#[test]
fn quit_saves_each_dirty_document_to_its_own_remembered_path() {
    let dir = tempfile::tempdir().unwrap();
    let path_a = write_ramp(&dir, "quit_a.tif");
    let path_b = write_ramp(&dir, "quit_b.tif");

    let mut app = ImageJApp::new();
    let id_a = app.open_tiff_file(path_a.clone()).unwrap();
    let id_b = app.open_tiff_file(path_b.clone()).unwrap();
    app.set_active_window(id_a).unwrap();
    app.invert_active().unwrap();
    app.set_active_window(id_b).unwrap();
    app.invert_active().unwrap();

    assert!(!app.request_quit());
    assert!(!app.quit_app);
    assert_eq!(
        app.pending_close.as_ref().unwrap().current_prompt,
        Some(id_a)
    );

    let first = app
        .save_pending_close_to_remembered_path()
        .expect("A has a remembered path")
        .unwrap();
    assert!(!first, "quit must still wait for B");
    assert!(!app.quit_app);
    assert_eq!(
        app.pending_close.as_ref().unwrap().current_prompt,
        Some(id_b)
    );

    let second = app
        .save_pending_close_to_remembered_path()
        .expect("B has a remembered path")
        .unwrap();
    assert!(second);
    assert!(app.quit_app);
    assert!(app.pending_close.is_none());
    assert_eq!(app.open_window_count(), 0);
    assert!(!app.any_dirty());

    assert_ne!(
        load_tiff(&path_a).unwrap().plane().sample_f64(0),
        ImageDocument::new_ramp_8bit("quit_a", 16, 16)
            .unwrap()
            .plane()
            .sample_f64(0)
    );
    assert_ne!(
        load_tiff(&path_b).unwrap().plane().sample_f64(0),
        ImageDocument::new_ramp_8bit("quit_b", 16, 16)
            .unwrap()
            .plane()
            .sample_f64(0)
    );
}

#[test]
fn proposed_tiff_name_never_doubles_a_tiff_extension() {
    use ij_app::app::proposed_tiff_file_name;

    assert_eq!(proposed_tiff_file_name("Untitled"), "Untitled.tif");
    assert_eq!(proposed_tiff_file_name("ramp"), "ramp.tif");
    assert_eq!(proposed_tiff_file_name("photo.tif"), "photo.tif");
    assert_eq!(proposed_tiff_file_name("photo.TIF"), "photo.TIF");
    assert_eq!(proposed_tiff_file_name("stack.tiff"), "stack.tiff");
    // A non-TIFF extension is kept and the TIFF extension is appended, like
    // ImageJ's SaveDialog (FileSaver.saveAsTiff uses the ".tif" extension).
    assert_eq!(proposed_tiff_file_name("scan.png"), "scan.png.tif");
}
