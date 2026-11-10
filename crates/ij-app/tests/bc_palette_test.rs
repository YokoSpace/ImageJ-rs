use egui::{CentralPanel, Context, RawInput};
use ij_app::ImageJApp;
use ij_session::Command;

fn render_image(ctx: &Context, app: &mut ImageJApp, id: ij_session::ImageId) {
    let _ = ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, id));
    });
}

#[test]
fn preview_range_refreshes_texture_without_recording_or_dirtying() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("preview", 32, 32).unwrap();
    app.session.set_dirty(id, false);
    render_image(&ctx, &mut app, id);
    let initial_uploads = app.texture_upload_count();
    let initial_records = app.session.macro_recorder.len();
    let initial_revision = app.session.image_revision(id);

    for max in [220.0, 180.0] {
        app.session.dispatch(Command::PreviewDisplayRange { min: 0.0, max }).unwrap();
        assert_eq!(app.session.get_image(id).unwrap().display_range.max, max);
        assert!(!app.session.is_dirty(id));
        assert_eq!(app.session.macro_recorder.len(), initial_records);
        assert!(app.session.image_revision(id) > initial_revision);
        render_image(&ctx, &mut app, id);
    }
    assert_eq!(app.texture_upload_count(), initial_uploads + 2);

    app.session.dispatch(Command::SetDisplayRange { min: 0.0, max: 180.0 }).unwrap();
    assert!(app.session.is_dirty(id));
    assert_eq!(app.session.macro_recorder.len(), initial_records + 1);
    assert_eq!(app.session.macro_recorder.last().unwrap(), "setMinAndMax(0, 180);");
}
