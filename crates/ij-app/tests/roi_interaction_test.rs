use egui::{
    CentralPanel, Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2,
};
use ij_app::ImageJApp;
use ij_core::{ImageDocument, PixelPlane};

fn frame(ctx: &Context, app: &mut ImageJApp, id: ij_session::ImageId, events: Vec<Event>) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 320.0))),
        events,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, id));
    });
}

fn point(image: Rect, x: f32, y: f32, zoom: f32) -> Pos2 {
    Pos2::new(image.left() + x * zoom, image.top() + y * zoom)
}

fn drag(ctx: &Context, app: &mut ImageJApp, id: ij_session::ImageId, start: Pos2, end: Pos2) {
    frame(
        ctx,
        app,
        id,
        vec![
            Event::PointerMoved(start),
            Event::PointerButton {
                pos: start,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    frame(ctx, app, id, vec![Event::PointerMoved(end)]);
    frame(
        ctx,
        app,
        id,
        vec![Event::PointerButton {
            pos: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
}

#[test]
fn inside_drag_moves_existing_roi_and_records_one_macro() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("move", 100, 100).unwrap();
    app.dispatch_rect_roi(20, 20, 20, 20).unwrap();
    app.image_views.get_mut(&id).unwrap().set_zoom(2.0);
    frame(&ctx, &mut app, id, vec![]);
    let image = ImageJApp::image_canvas_rect(&ctx, id).unwrap();
    let records = app.session.macro_recorder.len();
    drag(
        &ctx,
        &mut app,
        id,
        point(image, 30.0, 30.0, 2.0),
        point(image, 40.0, 35.0, 2.0),
    );
    assert_eq!(
        app.session
            .get_image(id)
            .unwrap()
            .roi
            .as_ref()
            .unwrap()
            .bounds(),
        (30, 25, 20, 20)
    );
    assert_eq!(app.session.macro_recorder.len(), records + 1);
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "makeRectangle(30, 25, 20, 20);"
    );
}

#[test]
fn handle_drag_resizes_and_outside_drag_creates_new_roi() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("resize", 100, 100).unwrap();
    app.dispatch_rect_roi(20, 20, 20, 20).unwrap();
    frame(&ctx, &mut app, id, vec![]);
    let image = ImageJApp::image_canvas_rect(&ctx, id).unwrap();
    drag(
        &ctx,
        &mut app,
        id,
        point(image, 40.0, 40.0, 1.0),
        point(image, 50.0, 55.0, 1.0),
    );
    assert_eq!(
        app.session
            .get_image(id)
            .unwrap()
            .roi
            .as_ref()
            .unwrap()
            .bounds(),
        (20, 20, 30, 35)
    );
    drag(
        &ctx,
        &mut app,
        id,
        point(image, 70.0, 70.0, 1.0),
        point(image, 80.0, 85.0, 1.0),
    );
    assert_eq!(
        app.session
            .get_image(id)
            .unwrap()
            .roi
            .as_ref()
            .unwrap()
            .bounds(),
        (70, 70, 10, 15)
    );
}

fn key(
    ctx: &Context,
    app: &mut ImageJApp,
    id: ij_session::ImageId,
    key: Key,
    modifiers: Modifiers,
) {
    let mut input = RawInput::default();
    input.modifiers = modifiers;
    input.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    let _ = ctx.run(input, |ctx| app.handle_image_shortcuts(ctx, id));
    let mut released = RawInput::default();
    released.modifiers = modifiers;
    released.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    });
    let _ = ctx.run(released, |_| {});
}

#[test]
fn arrows_nudge_roi_and_shift_arrows_navigate_stack() {
    let planes = vec![
        PixelPlane::Gray8(vec![0; 100]),
        PixelPlane::Gray8(vec![1; 100]),
    ];
    let doc = ImageDocument::new_stack("arrows", 10, 10, planes).unwrap();
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();
    app.dispatch_rect_roi(2, 2, 3, 3).unwrap();
    let ctx = Context::default();
    key(&ctx, &mut app, id, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        app.session
            .get_image(id)
            .unwrap()
            .roi
            .as_ref()
            .unwrap()
            .bounds(),
        (3, 2, 3, 3)
    );
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 1);
    key(
        &ctx,
        &mut app,
        id,
        Key::ArrowRight,
        Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 2);
    key(
        &ctx,
        &mut app,
        id,
        Key::ArrowDown,
        Modifiers {
            alt: true,
            ..Default::default()
        },
    );
    assert_eq!(
        app.session
            .get_image(id)
            .unwrap()
            .roi
            .as_ref()
            .unwrap()
            .bounds(),
        (3, 2, 3, 4)
    );
    app.session.get_image_mut(id).unwrap().roi = None;
    key(&ctx, &mut app, id, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 1);
}
