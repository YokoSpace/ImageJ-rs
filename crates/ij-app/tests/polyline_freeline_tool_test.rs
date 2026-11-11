//! P3.7b: Segmented Line (Polyline) and Freehand Line (Freeline) tool
//! construction, finishing rules and measurement parity with ImageJ 1.54u's
//! multi-point `makeLine(...)` and `makeSelection("freeline", ...)`.

use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::tool::polyline_is_valid;
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

/// Presses at `points[0]`, moves through the remaining screen positions one
/// frame each, then releases at the last one (FreehandRoi.grow per drag event).
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

fn pixel_pos(image: Rect, x: f32, y: f32) -> Pos2 {
    // Aim at the pixel center so `ImageView::screen_to_pixel` (which floors)
    // returns the intended integer coordinate.
    Pos2::new(image.left() + x + 0.5, image.top() + y + 0.5)
}

fn canvas(app: &mut ImageJApp, ctx: &Context, id: ij_session::ImageId, tool: Tool) -> Rect {
    app.active_tool = tool;
    frame(ctx, app, id, vec![], 0.0);
    ImageJApp::image_canvas_rect(ctx, id).expect("image canvas rect")
}

fn polyline_points(app: &ImageJApp, id: ij_session::ImageId) -> Option<(Vec<(i32, i32)>, bool)> {
    match app.session.get_image(id)?.roi.as_ref()? {
        Roi::Polyline(p) => Some((p.points.clone(), p.freehand)),
        _ => None,
    }
}

/// The Length column the oracle reports for the same selection.
fn oracle_polyline_length(points: &[(i32, i32)], freehand: bool) -> String {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).expect("oracle bridge");
    let roi_macro = if freehand {
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
        format!("makeSelection('freeline', newArray({xs}), newArray({ys}));")
    } else {
        let coords = points
            .iter()
            .map(|(x, y)| format!("{x}, {y}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("makeLine({coords});")
    };
    let out = bridge
        .eval_macro(&format!(
            "newImage('t','8-bit ramp',100,100,1); {roi_macro} \
             run('Set Measurements...','area'); run('Measure');"
        ))
        .expect("oracle macro");
    let mut lines = out.stdout.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().expect("oracle header");
    let row = lines.next().expect("oracle data row");
    let names: Vec<&str> = header.split_whitespace().collect();
    let values: Vec<&str> = row.split_whitespace().collect();
    let names: Vec<&str> = names.iter().map(|s| s.trim_start_matches('\\')).collect();
    let length_index = names
        .iter()
        .position(|&name| name == "Length")
        .expect("Length column");
    values[length_index + 1].to_string()
}

#[test]
fn polyline_and_freeline_tools_are_enabled() {
    assert!(Tool::Polyline.is_supported());
    assert!(Tool::Freeline.is_supported());
}

#[test]
fn polyline_validity_only_requires_two_points() {
    // Open lines survive zero width/height: finishPolygon inflates both
    // bounds before the delete test, so only nPoints<2 drops the selection
    // (PolygonRoi.java 571-593).
    assert!(!polyline_is_valid(&[]));
    assert!(!polyline_is_valid(&[(1, 1)]));
    assert!(polyline_is_valid(&[(1, 1), (1, 1)]));
    assert!(polyline_is_valid(&[(1, 1), (5, 1), (9, 1)]));
}

#[test]
fn clicks_then_double_click_commit_segmented_line_with_oracle_length() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("polyline", 100, 100).unwrap();
    let image = canvas(&mut app, &ctx, id, Tool::Polyline);

    let points = [(10, 10), (30, 12), (45, 30)];
    let mut t = 0.0;
    for &(x, y) in &points[..2] {
        t = click(
            &ctx,
            &mut app,
            id,
            pixel_pos(image, x as f32, y as f32),
            t,
        ) + 1.0;
    }

    // While constructing, the selection stays transient (CONSTRUCTING state).
    assert!(
        app.session.get_image(id).unwrap().roi.is_none(),
        "polyline must not be committed before it is finished"
    );

    // handleMouseUp completes on the double click once two-plus vertices
    // exist (PolygonRoi.java 1101-1118).
    double_click(
        &ctx,
        &mut app,
        id,
        pixel_pos(image, points[2].0 as f32, points[2].1 as f32),
        t,
    );

    let (built, freehand) = polyline_points(&app, id).expect("polyline ROI after finishing");
    assert_eq!(built, points.to_vec());
    assert!(!freehand, "the Segmented Line tool is POLYLINE, not FREELINE");
    assert_eq!(app.status_message, "Segmented line ROI created");

    // Java records a multi-point makeLine for a user-created polyline
    // (Recorder.recordRoi 397-424).
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("makeLine(10, 10, 30, 12, 45, 30);")
    );

    let stats = app.session.get_image(id).unwrap().measure().unwrap();
    assert_eq!(
        format_ij_num(stats.line.expect("line measurement").length),
        oracle_polyline_length(&points, false)
    );
}

#[test]
fn double_click_on_a_single_vertex_appends_nothing() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("polyline_short", 100, 100).unwrap();
    let image = canvas(&mut app, &ctx, id, Tool::Polyline);

    // A second click at the same vertex within the double-click window
    // neither finishes (nPoints<=2) nor appends (samePoint branch,
    // PolygonRoi.java 1101-1121).
    let pos = pixel_pos(image, 10.0, 10.0);
    let release = click(&ctx, &mut app, id, pos, 0.0);
    click(&ctx, &mut app, id, pos, release + 0.05);

    assert!(
        app.session.get_image(id).unwrap().roi.is_none(),
        "a one-vertex polyline never commits"
    );
    let preview = ImageJApp::polygon_construction_preview(&ctx, id).expect("construction state");
    assert_eq!(preview.len(), 0, "the repeated click appended no vertex");

    // Two distinct vertices plus a double click finish the open line.
    let second = click(
        &ctx,
        &mut app,
        id,
        pixel_pos(image, 40.0, 20.0),
        release + 2.0,
    );
    click(&ctx, &mut app, id, pixel_pos(image, 40.0, 20.0), second + 0.05);
    let (built, _) = polyline_points(&app, id).expect("two-vertex polyline");
    assert_eq!(built, vec![(10, 10), (40, 20)]);
}

#[test]
fn freeline_drag_commits_freehand_line_with_oracle_length() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("freeline", 100, 100).unwrap();
    let records = app.session.macro_recorder.len();
    let image = canvas(&mut app, &ctx, id, Tool::Freeline);

    let path = [(10, 10), (30, 12), (45, 30), (35, 50), (12, 40)];
    let screen: Vec<Pos2> = path
        .iter()
        .map(|&(x, y)| pixel_pos(image, x as f32, y as f32))
        .collect();
    drag_path(&ctx, &mut app, id, &screen, 0.0);

    let (built, freehand) = polyline_points(&app, id).expect("freeline ROI after drag");
    assert_eq!(built, path.to_vec());
    assert!(freehand, "the Freehand Line tool creates a FREELINE");
    assert_eq!(app.status_message, "Freehand line ROI created");

    // Freehand construction records no macro (PolygonRoi.java 594-596).
    assert_eq!(app.session.macro_recorder.len(), records);

    let stats = app.session.get_image(id).unwrap().measure().unwrap();
    assert_eq!(
        format_ij_num(stats.line.expect("line measurement").length),
        oracle_polyline_length(&path, true)
    );
}

#[test]
fn short_freeline_drags_follow_finishpolygon_rules() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("freeline_degenerate", 100, 100).unwrap();
    let image = canvas(&mut app, &ctx, id, Tool::Freeline);

    // A one-point drag leaves no selection: finishPolygon deletes open lines
    // with nPoints<2 (PolygonRoi.java 591-593, FreehandRoi.java 90-96).
    drag_path(
        &ctx,
        &mut app,
        id,
        &[pixel_pos(image, 10.0, 10.0), pixel_pos(image, 10.0, 10.0)],
        0.0,
    );
    assert!(app.session.get_image(id).unwrap().roi.is_none());

    // Two distinct points commit, unlike the freehand area tool whose
    // degenerate span finishPolygon keeps deleting (PolygonRoi.java 591).
    drag_path(
        &ctx,
        &mut app,
        id,
        &[pixel_pos(image, 10.0, 20.0), pixel_pos(image, 50.0, 20.0)],
        1.0,
    );
    let (built, freehand) = polyline_points(&app, id).expect("two-point freeline");
    assert_eq!(built, vec![(10, 20), (50, 20)]);
    assert!(freehand);
}
