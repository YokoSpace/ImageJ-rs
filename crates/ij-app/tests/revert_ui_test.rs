use ij_app::app::ImageJApp;
use ij_app::action::AppAction;
use ij_core::ImageDocument;
use ij_io::save_tiff;
use ij_session::Command;

#[test]
fn dirty_revert_requires_confirmation_and_cancel_preserves_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("revert.tif");
    let original = ImageDocument::new_ramp_8bit("revert.tif", 8, 8).unwrap();
    save_tiff(&path, &original).unwrap();

    let mut app = ImageJApp::new();
    let id = app.open_tiff_file(path).unwrap();
    app.session.dispatch(Command::Invert).unwrap();
    let edited = app.session.get_image(id).unwrap().slices.clone();
    assert_ne!(edited, original.slices);

    app.perform(AppAction::Revert, &egui::Context::default());
    assert_eq!(app.pending_revert, Some(id));
    assert_eq!(app.session.get_image(id).unwrap().slices, edited);

    app.resolve_revert_confirmation(false);
    assert_eq!(app.pending_revert, None);
    assert_eq!(app.session.get_image(id).unwrap().slices, edited);
    assert!(app.is_dirty(id));

    app.request_revert_active();
    app.resolve_revert_confirmation(true);
    assert_eq!(app.session.get_image(id).unwrap().slices, original.slices);
    assert!(!app.is_dirty(id));
}
