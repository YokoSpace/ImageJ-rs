//! R4: Freehand tool construction, consecutive-duplicate removal and
//! measurement parity with ImageJ 1.54u's
//! `makeSelection("freehand", xs, ys)`.

use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::tool::freehand_push;
use ij_app::{ImageJApp, Tool};
use ij_bridge::JavaOracleBridge;
use ij_core::{format_ij_num, Roi};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn frame(
    ctx: &Context,
    app: &mut ImageJApp,
    id: ij_session::ImageId,
    events: Vec<Event>,
    time: f64,
) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
        time: Some(time),
        events,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, id));
    });
}

fn pixel_pos(image: Rect, x: f32, y: f32) -> Pos2 {
    // Aim at the pixel center so `ImageView::screen_to_pixel` (which floors)
    // returns the intended integer coordinate.
    Pos2::new(image.left() + x + 0.5, image.top() + y + 0.5)
}

/// Presses at `points[0]`, moves through the remaining screen positions one
/// frame each, then releases at the last one. ImageJ calls `grow` once per
/// mouse-drag event (Roi.handleMouseDrag 1287-1295), so one frame per move
/// mirrors the Java event stream.
fn drag_path(
    ctx: &Context,
    app: &mut ImageJApp,
    id: ij_session::ImageId,
    points: &[Pos2],
    time: f64,
) {
    let start = points[0];
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
        time,
    );
    let mut t = time + 0.01;
    for &pos in &points[1..] {
        frame(ctx, app, id, vec![Event::PointerMoved(pos)], t);
        t += 0.01;
    }
    let end = *points.last().unwrap();
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
        t,
    );
}

fn canvas(app: &mut ImageJApp, ctx: &Context, id: ij_session::ImageId) -> Rect {
    app.active_tool = Tool::Freehand;
    frame(ctx, app, id, vec![], 0.0);
    ImageJApp::image_canvas_rect(ctx, id).expect("image canvas rect")
}

fn polygon_points(app: &ImageJApp, id: ij_session::ImageId) -> Option<Vec<(i32, i32)>> {
    match app.session.get_image(id)?.roi.as_ref()? {
        Roi::Polygon(p) => Some(
            p.xpoints
                .iter()
                .copied()
                .zip(p.ypoints.iter().copied())
                .collect(),
        ),
        _ => None,
    }
}

fn oracle_freehand_area(points: &[(i32, i32)]) -> String {
    let xs = points
        .iter()
        .map(|(x, _)| x.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let ys = points
        .iter()
        .map(|(_, y)| y.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).expect("oracle bridge");
    let out = bridge
        .eval_macro(&format!(
            "newImage('t','8-bit ramp',100,100,1); \
             makeSelection('freehand', newArray({xs}), newArray({ys})); \
             run('Set Measurements...','area'); run('Measure');"
        ))
        .expect("oracle macro");
    let mut lines = out.stdout.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().expect("oracle header");
    let row = lines.next().expect("oracle data row");
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        vec!["Area"],
        "unexpected oracle header: {header}"
    );
    let values: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(values[0], "1", "unexpected oracle row: {row}");
    values[1].to_string()
}

#[test]
fn freehand_tool_is_enabled() {
    assert!(Tool::Freehand.is_supported());
}

#[test]
fn freehand_push_skips_consecutive_duplicates_and_clamps() {
    // FreehandRoi.grow appends only when the new integer point differs from
    // the previous one (FreehandRoi.java 30-33).
    let mut points = vec![(10, 10)];
    freehand_push(&mut points, (10, 10), 100, 100);
    assert_eq!(points, vec![(10, 10)]);
    freehand_push(&mut points, (11, 10), 100, 100);
    freehand_push(&mut points, (11, 10), 100, 100);
    freehand_push(&mut points, (12, 12), 100, 100);
    assert_eq!(points, vec![(10, 10), (11, 10), (12, 12)]);

    // grow clamps ox/oy to [0, xMax]/[0, yMax] where xMax/yMax are the image
    // width/height (Roi.setImage 322-323, FreehandRoi.java 26-29).
    let mut clamped = vec![(5, 5)];
    freehand_push(&mut clamped, (-3, -4), 100, 80);
    assert_eq!(clamped, vec![(5, 5), (0, 0)]);
    freehand_push(&mut clamped, (250, 250), 100, 80);
    assert_eq!(clamped, vec![(5, 5), (0, 0), (100, 80)]);
}

#[test]
fn freehand_drag_builds_integer_polygon_with_oracle_area() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("freehand", 100, 100).unwrap();
    let image = canvas(&mut app, &ctx, id);

    let path = [(10, 10), (30, 12), (45, 30), (35, 50), (12, 40)];
    let screen: Vec<Pos2> = path
        .iter()
        .map(|&(x, y)| pixel_pos(image, x as f32, y as f32))
        .collect();
    drag_path(&ctx, &mut app, id, &screen, 0.0);

    let built = polygon_points(&app, id).expect("freehand ROI after drag");
    assert_eq!(built, path.to_vec());

    let stats = app.session.get_image(id).unwrap().measure().unwrap();
    assert_eq!(format_ij_num(stats.area), oracle_freehand_area(&path));
}

#[test]
fn freehand_drag_skips_repeated_pixel_positions() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("freehand_dupes", 100, 100).unwrap();
    let image = canvas(&mut app, &ctx, id);

    // Two consecutive frames land inside pixel (30, 12); Java's grow compares
    // integer coordinates and drops the second one (FreehandRoi.java 30).
    let screen = vec![
        pixel_pos(image, 10.0, 10.0),
        pixel_pos(image, 30.0, 12.0),
        pixel_pos(image, 30.0, 12.0),
        pixel_pos(image, 45.0, 30.0),
        pixel_pos(image, 35.0, 50.0),
        pixel_pos(image, 12.0, 40.0),
    ];
    drag_path(&ctx, &mut app, id, &screen, 0.0);

    let path = [(10, 10), (30, 12), (45, 30), (35, 50), (12, 40)];
    assert_eq!(
        polygon_points(&app, id).expect("freehand ROI"),
        path.to_vec()
    );

    let stats = app.session.get_image(id).unwrap().measure().unwrap();
    assert_eq!(format_ij_num(stats.area), oracle_freehand_area(&path));
}

#[test]
fn short_or_flat_freehand_drag_leaves_no_selection() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("freehand_degenerate", 100, 100).unwrap();
    let image = canvas(&mut app, &ctx, id);

    // Two points: finishPolygon deletes the ROI when nPoints<3
    // (PolygonRoi.java 591-593).
    drag_path(
        &ctx,
        &mut app,
        id,
        &[pixel_pos(image, 10.0, 10.0), pixel_pos(image, 40.0, 40.0)],
        0.0,
    );
    assert!(app.session.get_image(id).unwrap().roi.is_none());

    // Zero height: finishPolygon deletes the ROI when width==0 or height==0
    // (PolygonRoi.java 592).
    drag_path(
        &ctx,
        &mut app,
        id,
        &[
            pixel_pos(image, 10.0, 20.0),
            pixel_pos(image, 30.0, 20.0),
            pixel_pos(image, 50.0, 20.0),
        ],
        1.0,
    );
    assert!(app.session.get_image(id).unwrap().roi.is_none());
}
