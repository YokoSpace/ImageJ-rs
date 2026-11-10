//! Close/quit safety regression tests.
//!
//! These cover the destructive routes that could previously exit or replace a
//! pending prompt while a dirty document still existed:
//! * a document created or modified after the quit snapshot,
//! * repeated quit/close requests while the modal prompt is open,
//! * Cancel on a mixed clean/dirty document set,
//! * document-changing actions while the modal prompt is open.

use ij_app::app::{CloseTarget, ImageJApp};
use ij_core::ImageDocument;
use ij_io::save_tiff;
use ij_session::Command;
use std::fs;
use std::path::PathBuf;

struct TempTiff {
    dir: PathBuf,
    path: PathBuf,
}

impl TempTiff {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ij_close_safety_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clean.tif");
        let doc = ImageDocument::new_ramp_8bit("clean", 16, 16).unwrap();
        save_tiff(&path, &doc).unwrap();
        Self { dir, path }
    }
}

impl Drop for TempTiff {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.dir);
    }
}

fn pixel0(app: &ImageJApp, id: ij_session::ImageId) -> f64 {
    app.session
        .get_image(id)
        .expect("document must exist")
        .plane()
        .sample_f64(0)
}

#[test]
fn quit_prompt_does_not_discard_dirty_document_created_after_prompt() {
    let mut app = ImageJApp::new();
    let id_a = app.open_ramp_8bit("first", 16, 16).unwrap();

    assert!(!app.request_quit());
    let pending = app
        .pending_close
        .as_ref()
        .expect("quit prompt must be open");
    assert_eq!(pending.target, CloseTarget::Quit);
    assert_eq!(pending.current_prompt, Some(id_a));

    // Simulate a document slipping in through a non-UI path (plugin, race, or
    // a future code path) while the quit prompt is open.
    let outcome = app
        .session
        .dispatch(Command::NewRamp8Bit {
            title: "late".to_string(),
            width: 16,
            height: 16,
        })
        .unwrap();
    let id_b = match outcome {
        ij_session::CommandOutcome::ImageOpened(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(app.is_dirty(id_b), "late document must be dirty");

    // Resolving the first prompt must NOT quit and discard the late document.
    let finished = app.confirm_pending_close_discard();
    assert!(!finished, "late dirty document must keep the sequence open");
    assert!(
        !app.quit_app,
        "quit must not be authorized while a dirty document exists"
    );
    assert!(
        app.session.get_image(id_b).is_some(),
        "late dirty document must survive"
    );

    let pending = app
        .pending_close
        .as_ref()
        .expect("prompt must move to late document");
    assert_eq!(pending.target, CloseTarget::Quit);
    assert_eq!(pending.current_prompt, Some(id_b));

    // Only after the late document is resolved may the app quit.
    assert!(app.confirm_pending_close_discard());
    assert!(app.quit_app);
    assert!(app.session.get_image(id_b).is_none());
}

#[test]
fn quit_prompt_does_not_discard_document_dirtied_after_prompt() {
    let temp = TempTiff::new("dirtied_after_prompt");
    let mut app = ImageJApp::new();
    let id_dirty = app.open_ramp_8bit("dirty", 16, 16).unwrap();
    let id_clean = app.open_tiff_file(temp.path.clone()).unwrap();
    assert!(!app.is_dirty(id_clean));

    assert!(!app.request_quit());
    assert_eq!(
        app.pending_close.as_ref().unwrap().current_prompt,
        Some(id_dirty)
    );
    assert!(app.session.get_image(id_clean).is_some());

    // A background/plugin edit dirtying a previously clean document while the
    // prompt is open must be caught before exit, not silently discarded.
    app.session.set_dirty(id_clean, true);

    let finished = app.confirm_pending_close_discard();
    assert!(!finished, "newly dirty document requires its own prompt");
    assert!(!app.quit_app);
    assert!(app.session.get_image(id_clean).is_some());
    assert_eq!(
        app.pending_close.as_ref().unwrap().current_prompt,
        Some(id_clean)
    );

    assert!(app.confirm_pending_close_discard());
    assert!(app.quit_app);
    assert!(app.session.get_image(id_clean).is_none());
}

#[test]
fn repeated_quit_and_close_requests_do_not_replace_active_prompt() {
    let mut app = ImageJApp::new();
    let id_a = app.open_ramp_8bit("a", 16, 16).unwrap();
    let id_b = app.open_ramp_8bit("b", 16, 16).unwrap();

    assert!(!app.request_quit());
    let original = app.pending_close.clone().expect("quit prompt must be open");
    assert_eq!(original.target, CloseTarget::Quit);
    assert_eq!(original.current_prompt, Some(id_a));
    assert_eq!(original.queue, vec![id_a, id_b]);

    // Every repeated route while the modal is open must be a deterministic
    // no-op: it may neither reset, downgrade nor advance the prompt.
    assert!(!app.request_quit());
    assert_eq!(app.pending_close.as_ref(), Some(&original));

    assert!(!app.request_close_all());
    assert_eq!(app.pending_close.as_ref(), Some(&original));

    assert!(!app.request_close_window(id_b));
    assert_eq!(app.pending_close.as_ref(), Some(&original));

    assert!(!app.request_close_window(id_a));
    assert_eq!(app.pending_close.as_ref(), Some(&original));
    assert_eq!(app.open_window_count(), 2);

    // Advancing the queue still works once, and repeated requests keep the
    // second prompt stable as well.
    assert!(!app.confirm_pending_close_discard());
    let second = app
        .pending_close
        .clone()
        .expect("second prompt must be open");
    assert_eq!(second.current_prompt, Some(id_b));
    assert!(!app.request_quit());
    assert_eq!(app.pending_close.as_ref(), Some(&second));

    app.confirm_pending_close_cancel();
    assert_eq!(
        app.open_window_count(),
        1,
        "only the discarded document is gone"
    );
    assert!(app.session.get_image(id_b).is_some());
}

#[test]
fn cancel_quit_preserves_mixed_clean_and_dirty_documents() {
    let temp = TempTiff::new("cancel_quit_mixed");
    let mut app = ImageJApp::new();
    let id_clean = app.open_tiff_file(temp.path.clone()).unwrap();
    let id_dirty = app.open_ramp_8bit("dirty", 16, 16).unwrap();
    assert!(!app.is_dirty(id_clean));
    assert!(app.is_dirty(id_dirty));

    assert!(!app.request_quit());
    // Clean documents must remain open while the user decides; otherwise a
    // Cancel would still destroy them.
    assert!(app.session.get_image(id_clean).is_some());
    assert!(app.session.get_image(id_dirty).is_some());
    assert_eq!(app.open_window_count(), 2);

    app.confirm_pending_close_cancel();
    assert!(!app.quit_app);
    assert!(app.pending_close.is_none());
    assert_eq!(app.open_window_count(), 2);
    assert!(app.session.get_image(id_clean).is_some());
    assert!(app.session.get_image(id_dirty).is_some());
    assert!(app.is_dirty(id_dirty), "Cancel must not clear dirty state");
}

#[test]
fn cancel_close_all_preserves_mixed_clean_and_dirty_documents() {
    let temp = TempTiff::new("cancel_all_mixed");
    let mut app = ImageJApp::new();
    let id_clean = app.open_tiff_file(temp.path.clone()).unwrap();
    let id_dirty = app.open_ramp_8bit("dirty", 16, 16).unwrap();

    assert!(!app.request_close_all());
    assert_eq!(app.open_window_count(), 2);
    assert!(app.session.get_image(id_clean).is_some());

    app.confirm_pending_close_cancel();
    assert_eq!(app.open_window_count(), 2);
    assert!(app.session.get_image(id_clean).is_some());
    assert!(app.session.get_image(id_dirty).is_some());

    // A fresh Close All still resolves every dirty document.
    assert!(!app.request_close_all());
    assert!(app.confirm_pending_close_discard());
    assert_eq!(app.open_window_count(), 0);
    assert!(!app.quit_app, "Close All must not quit the application");
}

#[test]
fn document_mutation_and_open_are_blocked_while_close_prompt_is_open() {
    let temp = TempTiff::new("mutation_blocked");
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("ramp", 16, 16).unwrap();
    let before = pixel0(&app, id);

    assert!(!app.request_close_window(id));
    assert!(app.pending_close.is_some());

    // New document creation is refused.
    assert!(
        app.open_ramp_8bit("blocked_new", 8, 8).is_err(),
        "New must be refused while the close prompt is open"
    );
    assert_eq!(app.open_window_count(), 1);

    // Opening a file is refused.
    assert!(
        app.open_tiff_file(temp.path.clone()).is_err(),
        "Open must be refused while the close prompt is open"
    );
    assert_eq!(app.open_window_count(), 1);

    // Pixel-changing commands are refused and leave pixels untouched.
    assert!(app.invert_active().is_err());
    assert_eq!(pixel0(&app, id), before);
    assert!(app.undo_active().is_err());
    assert_eq!(pixel0(&app, id), before);
    assert!(app.smooth_active().is_err());
    assert_eq!(pixel0(&app, id), before);
    assert!(app.invert_stack_active().is_err());
    assert_eq!(pixel0(&app, id), before);

    // Prompt must still be the same, unresolved one.
    assert_eq!(app.pending_close.as_ref().unwrap().current_prompt, Some(id));

    // After Cancel the same operations become available again.
    app.confirm_pending_close_cancel();
    let id_new = app.open_ramp_8bit("allowed_after_cancel", 8, 8).unwrap();
    assert!(app.session.get_image(id_new).is_some());
    assert_eq!(app.open_window_count(), 2);
}

#[test]
fn close_all_prompt_catches_dirty_document_added_after_prompt() {
    let mut app = ImageJApp::new();
    let id_a = app.open_ramp_8bit("a", 16, 16).unwrap();
    assert!(!app.request_close_all());
    assert_eq!(app.pending_close.as_ref().unwrap().target, CloseTarget::All);
    assert_eq!(
        app.pending_close.as_ref().unwrap().current_prompt,
        Some(id_a)
    );

    let outcome = app
        .session
        .dispatch(Command::NewRamp8Bit {
            title: "late".to_string(),
            width: 16,
            height: 16,
        })
        .unwrap();
    let id_b = match outcome {
        ij_session::CommandOutcome::ImageOpened(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(!app.confirm_pending_close_discard());
    assert_eq!(
        app.pending_close.as_ref().unwrap().current_prompt,
        Some(id_b),
        "late dirty document must be prompted by Close All"
    );
    assert!(app.session.get_image(id_b).is_some());

    assert!(app.confirm_pending_close_discard());
    assert!(app.pending_close.is_none());
    assert!(app.session.get_image(id_b).is_none());
    assert!(!app.quit_app, "Close All must not quit");
}

#[test]
fn quit_on_clean_app_still_quits_immediately() {
    let mut app = ImageJApp::new();
    assert!(app.request_quit());
    assert!(app.quit_app);
    assert!(app.pending_close.is_none());
    // Once termination is in flight, creating new (dirty) work would only be
    // discarded by the exit, so it must be refused too.
    assert!(app.open_ramp_8bit("too_late", 8, 8).is_err());
}
