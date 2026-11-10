use ij_app::app::{CloseTarget, ImageJApp};
use ij_core::ImageDocument;
use ij_io::save_tiff;
use std::fs;

#[test]
fn dirty_tracking_lifecycle_marks_unsaved_changes() {
    let mut app = ImageJApp::new();

    // 1. New in-memory ramp document is marked dirty upon creation
    let id = app.open_ramp_8bit("test_ramp", 16, 16).unwrap();
    assert!(app.is_dirty(id), "Newly created image must be marked dirty");
    assert!(app.any_dirty());

    // 2. Saving to disk clears dirty status
    let temp_dir = std::env::temp_dir().join("ij_close_guard_test");
    let _ = fs::create_dir_all(&temp_dir);
    let path = temp_dir.join("saved_ramp.tif");

    app.save_active_tiff(path.clone()).unwrap();
    assert!(!app.is_dirty(id), "Saved image must be clean");
    assert!(!app.any_dirty());

    // 3. Pixel modifications mark it dirty again
    app.invert_active().unwrap();
    assert!(app.is_dirty(id), "Inverted image must be dirty");

    app.save_active_tiff(path.clone()).unwrap();
    assert!(!app.is_dirty(id));

    app.smooth_active().unwrap();
    assert!(app.is_dirty(id), "Filtered image must be dirty");

    app.save_active_tiff(path.clone()).unwrap();
    assert!(!app.is_dirty(id));

    app.undo_active().unwrap();
    assert!(app.is_dirty(id), "Undone image must be dirty");

    // 4. Opening an existing file from disk is clean initially
    let id2 = app.open_tiff_file(path.clone()).unwrap();
    assert!(
        !app.is_dirty(id2),
        "Opened file must not be dirty initially"
    );

    // Cleanup
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn single_window_close_guard_cancel_retains_document() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("cancel_test", 16, 16).unwrap();
    assert!(app.is_dirty(id));

    // Requesting close on a dirty window must not close it silently
    let closed_immediately = app.request_close_window(id);
    assert!(
        !closed_immediately,
        "Dirty document must not close immediately"
    );
    assert_eq!(app.open_window_count(), 1, "Document must be retained");

    let pending = app
        .pending_close
        .as_ref()
        .expect("Pending close must be active");
    assert_eq!(pending.target, CloseTarget::Single(id));
    assert_eq!(pending.current_prompt, Some(id));

    // User chooses Cancel
    app.confirm_pending_close_cancel();
    assert!(app.pending_close.is_none());
    assert_eq!(
        app.open_window_count(),
        1,
        "Document must remain open after cancel"
    );
    assert!(app.session.get_image(id).is_some());
    assert_eq!(app.status_message, "Close canceled");
}

#[test]
fn single_window_close_guard_cancelled_or_failed_save_retains_document() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("failed_save_test", 16, 16).unwrap();
    assert!(app.is_dirty(id));

    // 1. User clicks Save, but then cancels the file dialog (path is None)
    assert!(!app.request_close_window(id));
    let save_res = app.confirm_pending_close_save(None).unwrap();
    assert!(!save_res);
    assert_eq!(
        app.open_window_count(),
        1,
        "Document must be retained on cancelled save"
    );
    assert!(app.status_message.contains("Save cancelled"));
    assert!(app.session.get_image(id).is_some());

    // 2. User clicks Save and provides an invalid/unwritable path (save fails)
    assert!(!app.request_close_window(id));
    let bad_path = std::path::PathBuf::from("/nonexistent_directory_for_test/impossible/file.tif");
    let err = app.confirm_pending_close_save(Some(bad_path));
    assert!(err.is_err(), "Invalid path save must return error");
    assert_eq!(
        app.open_window_count(),
        1,
        "Document must be retained on failed save"
    );
    assert!(app.status_message.contains("Save failed"));
    assert!(
        app.dialogs.last_error.is_some(),
        "Error dialog must be surfaced"
    );
    assert!(app.session.get_image(id).is_some());
}

#[test]
fn single_window_close_guard_discard_and_save_success() {
    let mut app = ImageJApp::new();

    // Test discard: closes without saving
    let id1 = app.open_ramp_8bit("discard_test", 16, 16).unwrap();
    assert!(!app.request_close_window(id1));
    let done = app.confirm_pending_close_discard();
    assert!(done);
    assert_eq!(app.open_window_count(), 0);
    assert!(app.session.get_image(id1).is_none());

    // Test save success: saves file, clears dirty, closes window
    let id2 = app.open_ramp_8bit("save_success_test", 16, 16).unwrap();
    assert!(!app.request_close_window(id2));

    let temp_dir = std::env::temp_dir().join("ij_save_success_test");
    let _ = fs::create_dir_all(&temp_dir);
    let path = temp_dir.join("saved_ok.tif");

    let saved = app.confirm_pending_close_save(Some(path.clone())).unwrap();
    assert!(saved);
    assert_eq!(
        app.open_window_count(),
        0,
        "Window should be closed after successful save"
    );
    assert!(path.exists(), "File should be written to disk");

    // Cleanup
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn clean_window_closes_immediately_without_prompt() {
    let mut app = ImageJApp::new();

    // Create a known TIFF on disk
    let temp_dir = std::env::temp_dir().join("ij_clean_close_test");
    let _ = fs::create_dir_all(&temp_dir);
    let path = temp_dir.join("clean.tif");
    let doc = ImageDocument::new_ramp_8bit("clean", 16, 16).unwrap();
    save_tiff(&path, &doc).unwrap();

    let id = app.open_tiff_file(path.clone()).unwrap();
    assert!(!app.is_dirty(id), "Opened file is not dirty");

    // Requesting close on a clean window closes immediately without prompt
    let closed = app.request_close_window(id);
    assert!(closed, "Clean window must close immediately");
    assert!(app.pending_close.is_none());
    assert_eq!(app.open_window_count(), 0);

    let _ = fs::remove_file(path);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn close_all_guards_dirty_documents_in_sequence() {
    let mut app = ImageJApp::new();

    let id1 = app.open_ramp_8bit("ramp1", 16, 16).unwrap();
    let id2 = app.open_ramp_8bit("ramp2", 16, 16).unwrap();

    assert!(app.is_dirty(id1));
    assert!(app.is_dirty(id2));

    // Request close all
    let closed = app.request_close_all();
    assert!(!closed, "Dirty windows must not close all immediately");
    assert_eq!(app.open_window_count(), 2);

    let pending = app.pending_close.as_ref().unwrap();
    assert_eq!(pending.target, CloseTarget::All);
    assert_eq!(pending.current_prompt, Some(id1));

    // 1. Cancel aborts close all; both windows stay open
    app.confirm_pending_close_cancel();
    assert_eq!(app.open_window_count(), 2);
    assert!(app.session.get_image(id1).is_some());
    assert!(app.session.get_image(id2).is_some());

    // 2. Start close all again; discard id1 -> advances to prompt id2
    assert!(!app.request_close_all());
    let finished = app.confirm_pending_close_discard();
    assert!(!finished, "Still have id2 pending in close all sequence");
    assert_eq!(app.open_window_count(), 1);
    assert!(app.session.get_image(id1).is_none());
    assert!(app.session.get_image(id2).is_some());

    let pending = app.pending_close.as_ref().unwrap();
    assert_eq!(pending.current_prompt, Some(id2));

    // Discard id2 -> finishes close all
    let finished2 = app.confirm_pending_close_discard();
    assert!(finished2);
    assert_eq!(app.open_window_count(), 0);
    assert!(app.pending_close.is_none());
}

#[test]
fn quit_guard_requires_resolution_before_quitting() {
    let mut app = ImageJApp::new();
    let _id = app.open_ramp_8bit("quit_guard", 16, 16).unwrap();

    // 1. Calling request_quit on dirty app does not quit immediately
    let quit_now = app.request_quit();
    assert!(!quit_now);
    assert!(!app.quit_app);
    assert_eq!(app.open_window_count(), 1);

    // Cancel: app stays alive
    app.confirm_pending_close_cancel();
    assert!(!app.quit_app);
    assert_eq!(app.open_window_count(), 1);

    // 2. Request quit again and discard: app enters quit_app = true
    assert!(!app.request_quit());
    let quit_finished = app.confirm_pending_close_discard();
    assert!(quit_finished);
    assert!(
        app.quit_app,
        "quit_app flag must be set after resolving all dirty documents"
    );
    assert_eq!(app.open_window_count(), 0);
}
