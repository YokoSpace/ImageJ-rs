use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::ImageJApp;
use ij_bridge::JavaOracleBridge;
use ij_session::Command;

fn render_palette(ctx: &Context, app: &mut ImageJApp, events: Vec<Event>) {
    let mut input = RawInput::default();
    input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 420.0)));
    input.events = events;
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_threshold_palette_content(ui));
    });
}

fn click_button(ctx: &Context, app: &mut ImageJApp, label: &str) {
    render_palette(ctx, app, vec![]);
    let pos = ImageJApp::threshold_button_rect(ctx, label)
        .unwrap()
        .center();
    render_palette(
        ctx,
        app,
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    render_palette(
        ctx,
        app,
        vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
}

#[test]
fn lower_slider_previews_threshold_on_each_drag_frame() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("slider", 32, 32).unwrap();
    app.session.set_dirty(id, false);
    render_palette(&ctx, &mut app, vec![]);
    let rect = ImageJApp::threshold_lower_slider_rect(&ctx).unwrap();
    let start = Pos2::new(rect.left() + 45.0, rect.center().y);
    let first = Pos2::new(start.x + 30.0, start.y);
    let second = Pos2::new(start.x + 60.0, start.y);
    let records = app.session.macro_recorder.len();
    render_palette(
        &ctx,
        &mut app,
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
    render_palette(&ctx, &mut app, vec![Event::PointerMoved(first)]);
    let lower_one = app.session.get_image(id).unwrap().threshold.unwrap().lower;
    render_palette(&ctx, &mut app, vec![Event::PointerMoved(second)]);
    let lower_two = app.session.get_image(id).unwrap().threshold.unwrap().lower;
    assert_ne!(lower_one, lower_two);
    assert_eq!(app.session.macro_recorder.len(), records);
    assert!(!app.session.is_dirty(id));
}

#[test]
fn auto_otsu_button_matches_java_oracle() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("otsu", 32, 32).unwrap();
    let doc = app.session.get_image(id).unwrap().clone();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let oracle = JavaOracleBridge::locate_from_repo_root(repo).unwrap()
        .run_macro_on_image(&doc, "setAutoThreshold(\"Otsu\"); getThreshold(l,u); print(\"THRESH \"+d2s(l,6)+\" \"+d2s(u,6));")
        .unwrap();
    let line = oracle
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("THRESH "))
        .unwrap();
    let mut parts = line
        .split_whitespace()
        .skip(1)
        .map(|n| n.parse::<f64>().unwrap());
    let expected = (parts.next().unwrap(), parts.next().unwrap());
    app.dialogs.threshold_method = "Otsu".into();
    click_button(&ctx, &mut app, "Auto");
    let levels = app.session.get_image(id).unwrap().threshold.unwrap();
    assert_eq!((levels.lower, levels.upper), expected);
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "setAutoThreshold(\"Otsu\");"
    );
}

#[test]
fn apply_button_converts_thresholded_image_to_mask() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("opening", 32, 32).unwrap();
    render_palette(&ctx, &mut app, vec![]);
    assert!(app.session.get_image(id).unwrap().threshold.is_some());
    let doc = app.session.get_image(id).unwrap().clone();
    // The palette's auto-set levels are not saved in TIFFs, so the oracle
    // needs the matching setThreshold prelude.
    let levels = doc.threshold.unwrap();
    let oracle = JavaOracleBridge::locate_from_repo_root(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap(),
    )
    .unwrap()
    .run_macro_on_image(
        &doc,
        &format!(
            "setThreshold({}, {}); run(\"Convert to Mask\");",
            ij_core::format_ij_num(levels.lower),
            ij_core::format_ij_num(levels.upper)
        ),
    )
    .unwrap()
    .output_image
    .unwrap();
    click_button(&ctx, &mut app, "Apply");
    let applied = app.session.get_image(id).unwrap();
    assert_eq!(applied.slices, oracle.slices);
    assert_ne!(applied.plane(), doc.plane());
    assert!(app.session.is_dirty(id));
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "run(\"Convert to Mask\");"
    );
}

#[test]
fn apply_button_without_threshold_reports_threshold_is_not_set() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("no-threshold", 32, 32).unwrap();
    render_palette(&ctx, &mut app, vec![]);
    click_button(&ctx, &mut app, "Reset");
    assert!(app.session.get_image(id).unwrap().threshold.is_none());
    let pixels = app.session.get_image(id).unwrap().plane().clone();
    click_button(&ctx, &mut app, "Apply");
    // ThresholdAdjuster.apply rejects an image without threshold
    // (ThresholdAdjuster.java 776-781).
    assert_eq!(app.dialogs.last_error.as_deref(), Some("Threshold is not set"));
    assert_eq!(app.session.get_image(id).unwrap().plane(), &pixels);
}

#[test]
fn set_button_records_raw_threshold_once() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("set", 32, 32).unwrap();
    render_palette(&ctx, &mut app, vec![]);
    app.session
        .dispatch(Command::PreviewThreshold {
            lower: 40.0,
            upper: 180.0,
        })
        .unwrap();
    render_palette(&ctx, &mut app, vec![]);
    let records = app.session.macro_recorder.len();
    click_button(&ctx, &mut app, "Set");
    assert_eq!(
        app.session.get_image(id).unwrap().threshold.unwrap().lower,
        40.0
    );
    assert_eq!(app.session.macro_recorder.len(), records + 1);
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "setThreshold(40, 180, \"raw\");"
    );
}

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
