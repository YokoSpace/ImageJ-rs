use egui::{CentralPanel, Context, RawInput};
use ij_app::ImageJApp;
use ij_session::Command;

#[test]
fn preview_threshold_updates_red_overlay_without_macro_or_dirty_state() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("threshold-preview", 32, 32).unwrap();
    app.session.set_dirty(id, false);
    let _ = ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, id));
    });
    let uploads_before = app.texture_upload_count();
    let records_before = app.session.macro_recorder.len();
    let revision_before = app.session.image_revision(id);

    for (lower, upper) in [(40.0, 180.0), (80.0, 140.0)] {
        app.session
            .dispatch(Command::PreviewThreshold { lower, upper })
            .unwrap();
        let levels = app.session.get_image(id).unwrap().threshold.unwrap();
        assert_eq!((levels.lower, levels.upper), (lower, upper));
        assert!(app.session.image_revision(id) > revision_before);
        assert_eq!(app.session.macro_recorder.len(), records_before);
        assert!(!app.session.is_dirty(id));
        let _ = ctx.run(RawInput::default(), |ctx| {
            CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, id));
        });
    }
    assert_eq!(app.texture_upload_count(), uploads_before + 2);
}
