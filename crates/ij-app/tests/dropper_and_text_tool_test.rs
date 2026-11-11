use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::app::ImageJApp;
use ij_app::tool::Tool;
use ij_session::ImageId;

fn frame(ctx: &Context, app: &mut ImageJApp, image_id: ImageId, events: Vec<Event>) {
    let modifiers = events.iter().find_map(|e| match e { Event::PointerButton { modifiers, .. } => Some(*modifiers), _ => None }).unwrap_or_default();
    let input = RawInput {
        modifiers,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 320.0))),
        events,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, image_id));
    });
}

fn click(ctx: &Context, app: &mut ImageJApp, image_id: ImageId, x: f32, y: f32) {
    click_mod(ctx, app, image_id, x, y, Modifiers::NONE);
}

fn click_mod(ctx: &Context, app: &mut ImageJApp, image_id: ImageId, x: f32, y: f32, modifiers: Modifiers) {
    frame(ctx, app, image_id, Vec::new());
    let image = ImageJApp::image_canvas_rect(ctx, image_id).unwrap();
    let pos = Pos2::new(image.left() + x + 0.5, image.top() + y + 0.5);
    frame(ctx, app, image_id, vec![Event::PointerMoved(pos)]);
    frame(ctx, app, image_id, vec![Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: true,
        modifiers,
    }]);
    frame(ctx, app, image_id, vec![Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers,
    }]);
}

#[test]
fn every_enabled_tool_renders_headlessly() {
    for &tool in Tool::ALL {
        assert!(tool.is_supported(), "{}", tool.name());
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        let id = app.open_ramp_8bit("tools", 32, 32).unwrap();
        app.active_tool = tool;
        click(&ctx, &mut app, id, 2.0, 3.0);
    }
}

#[test]
fn dropper_samples_display_and_text_draw_is_undoable() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("text", 32, 32).unwrap();
    let before = app.session.active_doc().unwrap().plane().clone();
    app.active_tool = Tool::Dropper;
    click(&ctx, &mut app, id, 2.0, 3.0);
    let doc = app.session.active_doc().unwrap();
    let rgba = ij_app::pixel_render::plane_to_rgba(doc.plane(), doc.width, doc.height, doc.display_range, doc.threshold);
    assert_eq!(app.session.foreground_color, rgba[(3 * 32 + 2) * 4..(3 * 32 + 2) * 4 + 3]);
    app.active_tool = Tool::Text;
    click(&ctx, &mut app, id, 2.0, 3.0);
    app.text_selection.as_mut().unwrap().3 = "A".into();
    frame(&ctx, &mut app, id, vec![]);
    app.draw_active().unwrap();
    assert_ne!(app.session.active_doc().unwrap().plane(), &before);
    app.session.dispatch(ij_session::Command::Undo).unwrap();
    assert_eq!(app.session.active_doc().unwrap().plane(), &before);
}

#[test]
fn alt_dropper_sets_background() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("background", 32, 32).unwrap();
    app.active_tool = Tool::Dropper;
    click_mod(&ctx, &mut app, id, 2.0, 3.0, Modifiers::ALT);
    assert_eq!(app.session.foreground_color, [0,0,0]);
    assert_ne!(app.session.background_color, [255,255,255]);
}

#[test]
fn text_selection_stays_cleared_after_select_none() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("text selection", 32, 32).unwrap();
    app.active_tool = Tool::Text;
    click(&ctx, &mut app, id, 2.0, 3.0);
    app.text_selection.as_mut().unwrap().3 = "A".into();
    frame(&ctx, &mut app, id, vec![]);
    assert!(app.session.active_doc().unwrap().roi.is_some());

    app.session.dispatch(ij_session::Command::ClearRoi).unwrap();
    frame(&ctx, &mut app, id, vec![]);
    assert!(app.session.active_doc().unwrap().roi.is_none());
    assert!(app.text_selection.is_none());
}

#[test]
fn new_selection_at_text_origin_draws_its_own_outline() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("text selection", 32, 32).unwrap();
    app.active_tool = Tool::Text;
    click(&ctx, &mut app, id, 2.0, 3.0);
    app.text_selection.as_mut().unwrap().3 = "A".into();
    frame(&ctx, &mut app, id, vec![]);

    let replacement = ij_core::Roi::Rect(ij_core::RectRoi::new(2, 3, 1, 1).unwrap());
    app.session.active_doc_mut().unwrap().roi = Some(replacement);
    app.session.foreground_color = [255, 255, 255];
    app.draw_active().unwrap();
    let ij_core::PixelPlane::Gray8(pixels) = app.session.active_doc().unwrap().plane() else {
        panic!("expected Gray8 image");
    };
    assert_eq!(pixels[3 * 32 + 2], 255);
    assert_ne!(pixels[3 * 32 + 6], 255);
}
