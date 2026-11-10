use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::ImageJApp;
use ij_bridge::JavaOracleBridge;
use ij_core::{auto_contrast_range, ImageDocument};
use ij_session::Command;

fn render_image(ctx: &Context, app: &mut ImageJApp, id: ij_session::ImageId) {
    let _ = ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, id));
    });
}

fn render_palette(ctx: &Context, app: &mut ImageJApp, events: Vec<Event>) {
    let mut input = RawInput::default();
    input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 420.0)));
    input.events = events;
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_bc_palette_content(ui));
    });
}

fn click_palette_button(ctx: &Context, app: &mut ImageJApp, label: &str) {
    render_palette(ctx, app, vec![]);
    let pos = ImageJApp::bc_button_rect(ctx, label).unwrap().center();
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
fn auto_and_apply_buttons_dispatch_java_macro_actions() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("buttons", 32, 32).unwrap();
    app.set_display_range_active(40.0, 180.0).unwrap();
    let records = app.session.macro_recorder.len();
    click_palette_button(&ctx, &mut app, "Auto");
    assert_eq!(app.session.macro_recorder.len(), records + 1);
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "run(\"Enhance Contrast\", \"saturated=0.35\");"
    );
    app.session
        .dispatch(Command::SetDisplayRange {
            min: 50.0,
            max: 200.0,
        })
        .unwrap();
    let before = app.session.get_image(id).unwrap().plane().clone();
    click_palette_button(&ctx, &mut app, "Apply");
    assert_ne!(app.session.get_image(id).unwrap().plane(), &before);
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "run(\"Apply LUT\");"
    );
}

#[test]
fn apply_button_is_disabled_for_16bit_images() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let doc = ImageDocument::new_ramp_16bit("16bit", 32, 32).unwrap();
    let id = app.session.insert_doc(doc);
    app.session.set_active_image(id).unwrap();
    let before = app.session.get_image(id).unwrap().plane().clone();
    let records = app.session.macro_recorder.len();
    click_palette_button(&ctx, &mut app, "Apply");
    assert_eq!(app.session.get_image(id).unwrap().plane(), &before);
    assert_eq!(app.session.macro_recorder.len(), records);
}

#[test]
fn maximum_slider_drag_previews_each_frame_and_records_once_on_release() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("drag", 32, 32).unwrap();
    app.set_display_range_active(0.0, 128.0).unwrap();
    app.session.set_dirty(id, false);
    render_image(&ctx, &mut app, id);
    let initial_uploads = app.texture_upload_count();
    let initial_records = app.session.macro_recorder.len();
    render_palette(&ctx, &mut app, vec![]);
    let rect = ImageJApp::bc_max_slider_rect(&ctx).unwrap();
    let start = Pos2::new(rect.left() + 55.0, rect.center().y);
    let move_one = Pos2::new(start.x - 20.0, start.y);
    let move_two = Pos2::new(start.x - 40.0, start.y);
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
    render_palette(&ctx, &mut app, vec![Event::PointerMoved(move_one)]);
    let first_max = app.session.get_image(id).unwrap().display_range.max;
    assert_ne!(first_max, 128.0, "first drag frame did not preview");
    assert_eq!(app.session.macro_recorder.len(), initial_records);
    assert!(!app.session.is_dirty(id));
    render_image(&ctx, &mut app, id);
    render_palette(&ctx, &mut app, vec![Event::PointerMoved(move_two)]);
    let second_max = app.session.get_image(id).unwrap().display_range.max;
    assert_ne!(second_max, first_max, "second drag frame did not preview");
    assert_eq!(app.session.macro_recorder.len(), initial_records);
    render_image(&ctx, &mut app, id);
    assert_eq!(app.texture_upload_count(), initial_uploads + 2);
    render_palette(
        &ctx,
        &mut app,
        vec![Event::PointerButton {
            pos: move_two,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(app.session.macro_recorder.len(), initial_records + 1);
    assert!(app
        .session
        .macro_recorder
        .last()
        .unwrap()
        .starts_with("setMinAndMax("));
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
        app.session
            .dispatch(Command::PreviewDisplayRange { min: 0.0, max })
            .unwrap();
        assert_eq!(app.session.get_image(id).unwrap().display_range.max, max);
        assert!(!app.session.is_dirty(id));
        assert_eq!(app.session.macro_recorder.len(), initial_records);
        assert!(app.session.image_revision(id) > initial_revision);
        render_image(&ctx, &mut app, id);
    }
    assert_eq!(app.texture_upload_count(), initial_uploads + 2);

    app.session
        .dispatch(Command::SetDisplayRange {
            min: 0.0,
            max: 180.0,
        })
        .unwrap();
    assert!(app.session.is_dirty(id));
    assert_eq!(app.session.macro_recorder.len(), initial_records + 1);
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "setMinAndMax(0, 180);"
    );
}

#[test]
fn first_auto_range_matches_java_enhance_contrast() {
    let doc = ImageDocument::new_ramp_8bit("auto", 256, 256).unwrap();
    let (rust_range, next_threshold) = auto_contrast_range(&doc, 0);
    assert_eq!(next_threshold, 5000);
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let oracle = JavaOracleBridge::locate_from_repo_root(repo)
        .unwrap()
        .run_macro_on_image(
            &doc,
            "run(\"Enhance Contrast\", \"saturated=0.35\"); getMinAndMax(min, max); print(\"RANGE:\"+min+\",\"+max);",
        )
        .unwrap();
    let range = oracle
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("RANGE:"))
        .unwrap();
    let (min, max) = range.split_once(',').unwrap();
    let rust_range = rust_range.unwrap();
    assert_eq!(rust_range.min, min.parse::<f64>().unwrap());
    assert_eq!(rust_range.max, max.parse::<f64>().unwrap());
}

#[test]
fn apply_lut_matches_java_pixels_inside_rect_roi() {
    let mut doc = ImageDocument::new_ramp_8bit("apply", 16, 16).unwrap();
    doc.display_range.min = 50.0;
    doc.display_range.max = 200.0;
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let oracle = JavaOracleBridge::locate_from_repo_root(repo)
        .unwrap()
        .run_macro_on_image(&doc, "run(\"Apply LUT\");")
        .unwrap();

    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(doc);
    app.session.set_active_image(id).unwrap();
    app.session.dispatch(Command::ApplyDisplayLut).unwrap();
    assert_eq!(
        app.session.get_image(id).unwrap().plane(),
        oracle.output_image.unwrap().plane()
    );
}
