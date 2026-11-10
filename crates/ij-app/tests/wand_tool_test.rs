use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::{ImageJApp, Tool};
use ij_core::{PixelPlane, Roi, Wand, WandMode};

fn frame(ctx: &Context, app: &mut ImageJApp, id: ij_session::ImageId, events: Vec<Event>) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
        events,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, id));
    });
}

fn click(ctx: &Context, app: &mut ImageJApp, id: ij_session::ImageId, pos: Pos2) {
    frame(ctx, app, id, vec![Event::PointerMoved(pos)]);
    frame(
        ctx,
        app,
        id,
        vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
    );
    frame(
        ctx,
        app,
        id,
        vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
}

#[test]
fn wand_click_selects_java_outline() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("wand", 7, 7).unwrap();
    let mut pixels = vec![0u8; 49];
    for y in 2..=4 {
        for x in 2..=4 {
            pixels[y * 7 + x] = 7;
        }
    }
    app.session.get_image_mut(id).unwrap().slices[0] = PixelPlane::Gray8(pixels);
    let expected = Wand::auto_outline(
        app.session.get_image(id).unwrap(),
        3,
        3,
        0.0,
        WandMode::Legacy,
    );

    app.active_tool = Tool::Wand;
    assert!(Tool::Wand.is_supported());
    frame(&ctx, &mut app, id, vec![]);
    let rect = ImageJApp::image_canvas_rect(&ctx, id).unwrap();
    click(
        &ctx,
        &mut app,
        id,
        Pos2::new(rect.left() + 3.5, rect.top() + 3.5),
    );

    let Roi::Polygon(roi) = app
        .session
        .get_image(id)
        .unwrap()
        .roi
        .as_ref()
        .expect("wand ROI")
    else {
        panic!("wand should produce a polygon ROI");
    };
    let actual = roi
        .xpoints
        .iter()
        .copied()
        .zip(roi.ypoints.iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("doWand(3, 3);")
    );
}
