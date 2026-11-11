use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::app::ImageJApp;
use ij_app::tool::Tool;
use ij_core::Roi;
use ij_session::{Command, ImageId};

fn frame(ctx: &Context, app: &mut ImageJApp, image_id: ImageId, events: Vec<Event>) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 320.0))),
        events,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, image_id));
    });
}

fn click(ctx: &Context, app: &mut ImageJApp, image_id: ImageId, x: f32, y: f32) {
    frame(ctx, app, image_id, Vec::new());
    let image = ImageJApp::image_canvas_rect(ctx, image_id).unwrap();
    let pos = Pos2::new(image.left() + x + 0.5, image.top() + y + 0.5);
    frame(ctx, app, image_id, vec![Event::PointerMoved(pos)]);
    frame(ctx, app, image_id, vec![Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    }]);
    frame(ctx, app, image_id, vec![Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
}

#[test]
fn point_tool_adds_multiple_points_and_measure_emits_one_row_per_point() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let image_id = app.open_ramp_8bit("points", 16, 16).unwrap();
    assert!(Tool::Point.is_supported());
    app.active_tool = Tool::Point;
    click(&ctx, &mut app, image_id, 2.0, 3.0);
    click(&ctx, &mut app, image_id, 7.0, 9.0);
    assert_eq!(
        app.session.active_doc().unwrap().roi,
        Some(Roi::Point(ij_core::PointRoi { points: vec![(2, 3), (7, 9)] }))
    );
    app.session.dispatch(Command::Measure).unwrap();
    assert_eq!(app.session.results.rows.len(), 2);
    assert_eq!(app.session.results.rows[0].mean, 32.0);
    assert_eq!(app.session.results.rows[1].mean, 112.0);
}

#[test]
fn angle_tool_commits_on_third_click_and_measures_java_angle() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let image_id = app.open_ramp_8bit("angle", 16, 16).unwrap();
    assert!(Tool::Angle.is_supported());
    app.active_tool = Tool::Angle;
    click(&ctx, &mut app, image_id, 2.0, 3.0);
    click(&ctx, &mut app, image_id, 5.0, 8.0);
    assert!(app.session.active_doc().unwrap().roi.is_none());
    click(&ctx, &mut app, image_id, 9.0, 4.0);
    assert_eq!(
        app.session.active_doc().unwrap().roi,
        Some(Roi::Angle(ij_core::AngleRoi { points: [(2, 3), (5, 8), (9, 4)] }))
    );
    app.session.dispatch(Command::Measure).unwrap();
    let angle = app.session.results.rows.last().unwrap().angle.unwrap();
    assert!((angle - 75.96375653207352).abs() < 1e-9);
}
