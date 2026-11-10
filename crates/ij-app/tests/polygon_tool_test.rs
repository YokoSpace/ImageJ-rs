//! R3: Polygon tool construction, closing rules, transient rubber band and
//! measurement parity with ImageJ 1.54u's `makePolygon(...)`.

use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::tool::{polygon_closes_at_start, polygon_preview_segments};
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

/// One click at `pos`, returning the release time. The press and release sit
/// one frame apart so egui reports `Response::clicked()` on the release frame.
fn click(
    ctx: &Context,
    app: &mut ImageJApp,
    id: ij_session::ImageId,
    pos: Pos2,
    time: f64,
) -> f64 {
    frame(ctx, app, id, vec![Event::PointerMoved(pos)], time);
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
        time + 0.01,
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
        time + 0.02,
    );
    time + 0.02
}

/// A second click within egui's 300 ms double-click window.
fn double_click(
    ctx: &Context,
    app: &mut ImageJApp,
    id: ij_session::ImageId,
    pos: Pos2,
    time: f64,
) {
    let release = click(ctx, app, id, pos, time);
    click(ctx, app, id, pos, release + 0.05);
}

fn pixel_pos(image: Rect, x: f32, y: f32) -> Pos2 {
    // Aim at the pixel center so `ImageView::screen_to_pixel` (which floors)
    // returns the intended integer coordinate.
    Pos2::new(
        image.left() + x + 0.5,
        image.top() + y + 0.5,
    )
}

fn canvas(app: &mut ImageJApp, ctx: &Context, id: ij_session::ImageId) -> Rect {
    app.active_tool = Tool::Polygon;
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

fn oracle_area(points: &[(i32, i32)]) -> String {
    let coords = points
        .iter()
        .map(|(x, y)| format!("{x},{y}"))
        .collect::<Vec<_>>()
        .join(",");
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).expect("oracle bridge");
    let out = bridge
        .eval_macro(&format!(
            "newImage('t','8-bit ramp',100,100,1); makePolygon({coords}); \
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
fn polygon_tool_is_enabled() {
    assert!(Tool::Polygon.is_supported());
}

#[test]
fn polygon_preview_segments_join_vertices_and_cursor() {
    // No vertices: nothing to draw.
    assert!(polygon_preview_segments(&[], None).is_empty());
    assert!(polygon_preview_segments(&[], Some((7, 7))).is_empty());

    // A single vertex plus cursor draws the rubber band even before the
    // selection is closed.
    assert_eq!(
        polygon_preview_segments(&[(2, 2)], Some((9, 4))),
        vec![((2, 2), (9, 4))]
    );

    // Committed edges plus the live edge from the last vertex to the cursor,
    // PolygonRoi.paint draws the open polyline (PolygonRoi.java 228).
    assert_eq!(
        polygon_preview_segments(&[(2, 2), (12, 2), (12, 9)], Some((4, 7))),
        vec![
            ((2, 2), (12, 2)),
            ((12, 2), (12, 9)),
            ((12, 9), (4, 7)),
        ]
    );

    // Cursor resting on the last vertex adds no degenerate segment.
    assert_eq!(
        polygon_preview_segments(&[(2, 2), (12, 2)], Some((12, 2))),
        vec![((2, 2), (12, 2))]
    );
}

#[test]
fn polygon_start_box_scales_with_zoom() {
    // PolygonRoi.handleMouseUp builds a 10x10 screen-pixel box centred on the
    // start point (PolygonRoi.java 1100); Rectangle.contains is [x, x+10).
    let start = (10, 10);

    // Zoom 1: start pixel is at screen (10, 10).
    assert!(polygon_closes_at_start(start, (14.9, 14.9), 1.0));
    assert!(polygon_closes_at_start(start, (5.0, 5.0), 1.0));
    assert!(!polygon_closes_at_start(start, (15.0, 12.0), 1.0));
    assert!(!polygon_closes_at_start(start, (4.9, 12.0), 1.0));

    // Zoom 2: start pixel is at screen (20, 20); the box is still 10 screen px,
    // i.e. 2.5 image pixels on each side.
    assert!(polygon_closes_at_start(start, (24.0, 24.0), 2.0));
    assert!(!polygon_closes_at_start(start, (25.5, 24.0), 2.0));

    // Zoom 4: start pixel is at screen (40, 40); the box covers +-1.25 px.
    assert!(polygon_closes_at_start(start, (44.0, 44.0), 4.0));
    assert!(!polygon_closes_at_start(start, (45.5, 44.0), 4.0));
}

#[test]
fn clicks_then_double_click_close_four_vertex_polygon_with_oracle_area() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("polygon", 100, 100).unwrap();
    let image = canvas(&mut app, &ctx, id);

    let points = [(2, 2), (40, 3), (35, 30), (5, 25)];

    let mut t = 0.0;
    for &(x, y) in &points[..3] {
        t = click(&ctx, &mut app, id, pixel_pos(image, x as f32, y as f32), t) + 1.0;
    }

    // While constructing, the selection is transient: the document has no ROI
    // and the rubber band previews the open polyline.
    assert!(
        app.session.get_image(id).unwrap().roi.is_none(),
        "polygon must not be committed before it is closed"
    );
    let preview = ImageJApp::polygon_construction_preview(&ctx, id).expect("construction state");
    assert_eq!(preview.len(), 2, "three vertices give two open edges");
    assert_eq!(preview[0], ((2, 2), (40, 3)));

    let (x, y) = points[3];
    double_click(&ctx, &mut app, id, pixel_pos(image, x as f32, y as f32), t);

    let built = polygon_points(&app, id).expect("polygon ROI after closing");
    assert_eq!(built, points.to_vec());
    assert!(
        ImageJApp::polygon_construction_preview(&ctx, id).is_none(),
        "construction state must be cleared after closing"
    );

    // Java records one makePolygon for a user-created polygon
    // (PolygonRoi.finishPolygon 601-604).
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("makePolygon(2, 2, 40, 3, 35, 30, 5, 25);")
    );

    let stats = app.session.get_image(id).unwrap().measure().unwrap();
    assert_eq!(format_ij_num(stats.area), oracle_area(&points));
    assert_eq!(format_ij_num(853.0), oracle_area(&points));
}

#[test]
fn click_near_start_closes_four_vertex_polygon() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("polygon_start", 100, 100).unwrap();
    let image = canvas(&mut app, &ctx, id);

    let points = [(10, 10), (60, 12), (55, 55), (12, 50)];

    let mut t = 0.0;
    for &(x, y) in &points {
        t = click(&ctx, &mut app, id, pixel_pos(image, x as f32, y as f32), t) + 1.0;
    }

    // A click within the start box terminates the selection without adding a
    // fifth vertex (PolygonRoi.java 1100-1118).
    click(&ctx, &mut app, id, pixel_pos(image, 14.0, 14.0), t);

    let built = polygon_points(&app, id).expect("polygon ROI after closing");
    assert_eq!(built, points.to_vec());

    let stats = app.session.get_image(id).unwrap().measure().unwrap();
    assert_eq!(format_ij_num(stats.area), oracle_area(&points));
}

#[test]
fn click_moved_well_away_from_start_adds_another_vertex() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("polygon_open", 100, 100).unwrap();
    let image = canvas(&mut app, &ctx, id);

    let points = [(10, 10), (60, 12), (55, 55), (12, 50)];

    let mut t = 0.0;
    for &(x, y) in &points {
        t = click(&ctx, &mut app, id, pixel_pos(image, x as f32, y as f32), t) + 1.0;
    }

    // Far from the start box: this is a normal additional vertex.
    click(&ctx, &mut app, id, pixel_pos(image, 40.0, 40.0), t);

    assert!(
        app.session.get_image(id).unwrap().roi.is_none(),
        "selection stays open until the user closes it"
    );
    let preview = ImageJApp::polygon_construction_preview(&ctx, id).expect("construction state");
    assert_eq!(preview.len(), 4, "five vertices give four open edges");
    assert_eq!(preview[3], ((12, 50), (40, 40)));
}
