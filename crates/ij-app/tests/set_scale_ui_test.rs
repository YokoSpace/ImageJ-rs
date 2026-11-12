//! P3.6: `Analyze > Set Scale...` dialog, initial values and Remove Scale.
//!
//! Expected labels and defaults come from ScaleDialog.run
//! (original_imagej/ij/plugin/filter/ScaleDialog.java 24-120) and
//! ImagePlus.removeScale (ImagePlus.java 2206-2216).

use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::action::{AppAction, MenuKind, MENU_TREE};
use ij_app::app::{ImageJApp, set_scale_defaults, set_scale_input_is_valid, set_scale_label};
use ij_core::{Calibration, ImageDocument};
use ij_session::{Command, CommandOutcome};

fn analyze_menu() -> &'static [ij_app::action::MenuNode] {
    let analyze = MENU_TREE
        .iter()
        .find(|node| node.label == "Analyze")
        .expect("Analyze menu");
    let MenuKind::Submenu(nodes) = analyze.kind else {
        panic!("Analyze must be a submenu");
    };
    nodes
}

fn app_with_ramp() -> ImageJApp {
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("scale", 64, 64).unwrap();
    app
}

fn render_dialog(ctx: &Context, app: &mut ImageJApp, events: Vec<Event>) {
    let mut input = RawInput::default();
    input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(460.0, 340.0)));
    input.events = events;
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_set_scale_content(ui));
    });
}

fn click_at(ctx: &Context, app: &mut ImageJApp, pos: Pos2) {
    render_dialog(ctx, app, vec![Event::PointerMoved(pos)]);
    render_dialog(
        ctx,
        app,
        vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
    );
    render_dialog(
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

fn click_button(ctx: &Context, app: &mut ImageJApp, label: &str) {
    render_dialog(ctx, app, vec![]);
    let pos = ImageJApp::set_scale_button_rect(ctx, label)
        .unwrap_or_else(|| panic!("missing button rect for {label}"))
        .center();
    click_at(ctx, app, pos);
}

#[test]
fn analyze_menu_wires_set_scale() {
    let item = analyze_menu()
        .iter()
        .find(|node| node.label == "Set Scale...")
        .expect("Set Scale item");
    assert!(matches!(item.kind, MenuKind::Item(AppAction::SetScale)));
    assert_eq!(item.shortcut, None);
    // Calibrate... remains the unsupported density-calibration dialog.
    let calibrate = analyze_menu()
        .iter()
        .find(|node| node.label == "Calibrate...")
        .expect("Calibrate item");
    assert!(matches!(calibrate.kind, MenuKind::Unsupported));
}

#[test]
fn defaults_for_uncalibrated_image_without_selection() {
    let doc = ImageDocument::new_ramp_8bit("t", 64, 64).unwrap();
    let defaults = set_scale_defaults(&doc);
    assert_eq!(defaults.distance, 0.0);
    assert_eq!(defaults.known, 0.0);
    assert_eq!(defaults.aspect, 1.0);
    assert_eq!(defaults.unit, "pixel");
    assert_eq!(defaults.raw_length, 0.0);
    assert_eq!(set_scale_label(0.0, 0.0, "pixel"), "<no scale>");
}

#[test]
fn line_and_rect_selections_seed_distance_in_pixels() {
    let mut line_doc = ImageDocument::new_ramp_8bit("t", 64, 64).unwrap();
    line_doc.set_line_roi(0, 0, 30, 40);
    let line = set_scale_defaults(&line_doc);
    // Line.getRawLength (Line.java 440-442) ignores calibration.
    assert_eq!(line.distance, 50.0);
    assert_eq!(line.raw_length, 50.0);
    assert_eq!(line.known, 0.0);

    let mut rect_doc = ImageDocument::new_ramp_8bit("t", 64, 64).unwrap();
    rect_doc.set_rect_roi(2, 3, 10, 7).unwrap();
    let rect = set_scale_defaults(&rect_doc);
    // ScaleDialog.java 43-47 uses the larger rectangle side.
    assert_eq!(rect.distance, 10.0);
    assert_eq!(rect.raw_length, 10.0);
    assert_eq!(rect.known, 0.0);
}

#[test]
fn calibrated_images_prefill_known_distance_aspect_and_unit() {
    let mut doc = ImageDocument::new_ramp_8bit("t", 64, 64).unwrap();
    doc.calibration = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.25,
        pixel_depth: 1.0,
        unit: "mm".to_string(),
        invert_y: false,
        x_origin: 0.0,
        y_origin: 0.0,
    };
    doc.set_line_roi(0, 0, 30, 40);

    let defaults = set_scale_defaults(&doc);
    assert_eq!(defaults.distance, 50.0);
    assert_eq!(defaults.known, 25.0);
    assert_eq!(defaults.aspect, 0.5);
    assert_eq!(defaults.unit, "mm");
    assert_eq!(set_scale_label(50.0, 25.0, "mm"), "2 pixels/mm");

    // With no line/rect, ScaleDialog.java 50-62 inverts pixelWidth instead.
    doc.roi = None;
    let no_selection = set_scale_defaults(&doc);
    assert_eq!(no_selection.distance, 2.0);
    assert_eq!(no_selection.known, 1.0);
    assert_eq!(no_selection.aspect, 0.5);
}

#[test]
fn scale_label_matches_scale_dialog_text_value_changed() {
    assert_eq!(set_scale_label(20.0, 10.0, "mm"), "2 pixels/mm");
    assert_eq!(set_scale_label(10.0, 10.0, "mm"), "1 pixel/mm");
    assert_eq!(set_scale_label(1.0, 3.0, "mm"), "0.3333 pixels/mm");
    for no_scale in [
        set_scale_label(0.0, 10.0, "mm"),
        set_scale_label(20.0, 0.0, "mm"),
        set_scale_label(20.0, 10.0, "pixel"),
        set_scale_label(20.0, 10.0, "Pixel"),
        set_scale_label(20.0, 10.0, ""),
    ] {
        assert_eq!(no_scale, "<no scale>");
    }
}

#[test]
fn perform_set_scale_opens_dialog_with_selection_defaults() {
    let ctx = Context::default();
    let mut app = app_with_ramp();
    app.session
        .dispatch(Command::MakeRectRoi {
            x: 4,
            y: 5,
            width: 12,
            height: 7,
        })
        .unwrap();

    app.perform(AppAction::SetScale, &ctx);

    assert!(app.set_scale.open);
    assert_eq!(app.set_scale.distance, 12.0);
    assert_eq!(app.set_scale.known, 0.0);
    assert_eq!(app.set_scale.aspect, 1.0);
    assert_eq!(app.set_scale.unit, "pixel");
}

#[test]
fn ok_applies_calibration_and_records_macro() {
    let ctx = Context::default();
    let mut app = app_with_ramp();
    app.perform(AppAction::SetScale, &ctx);
    app.set_scale.distance = 20.0;
    app.set_scale.known = 10.0;
    app.set_scale.aspect = 1.0;
    app.set_scale.unit = "mm".to_string();

    click_button(&ctx, &mut app, "OK");

    assert!(!app.set_scale.open, "OK must close the dialog");
    let calibration = &app.session.active_doc().unwrap().calibration;
    assert_eq!(calibration.pixel_width, 0.5);
    assert_eq!(calibration.pixel_height, 0.5);
    assert_eq!(calibration.pixel_depth, 0.5);
    assert_eq!(calibration.unit, "mm");
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Set Scale...\", \"distance=20 known=10 pixel=1 unit=mm\");")
    );
}

#[test]
fn cancel_discards_dialog_edits() {
    let ctx = Context::default();
    let mut app = app_with_ramp();
    app.perform(AppAction::SetScale, &ctx);
    app.set_scale.distance = 20.0;
    app.set_scale.known = 10.0;
    app.set_scale.unit = "mm".to_string();

    click_button(&ctx, &mut app, "Cancel");

    assert!(!app.set_scale.open);
    assert_eq!(
        app.session.active_doc().unwrap().calibration,
        Calibration::default()
    );
    assert!(app
        .session
        .macro_recorder
        .iter()
        .all(|record| !record.starts_with("run(\"Set Scale...\"")));
}

#[test]
fn remove_scale_button_restores_pixel_calibration() {
    let ctx = Context::default();
    let mut app = app_with_ramp();
    app.session.active_doc_mut().unwrap().calibration = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.25,
        pixel_depth: 1.0,
        unit: "mm".to_string(),
        invert_y: false,
        x_origin: 0.0,
        y_origin: 0.0,
    };
    app.perform(AppAction::SetScale, &ctx);
    assert_eq!(app.set_scale.distance, 2.0);

    click_button(&ctx, &mut app, "Click to Remove Scale");

    assert!(app.set_scale.open, "ImageJ keeps the dialog open");
    assert_eq!(
        app.session.active_doc().unwrap().calibration,
        Calibration {
            pixel_width: 0.5,
            pixel_height: 0.25,
            pixel_depth: 1.0,
            unit: "mm".to_string(),
            invert_y: false,
            x_origin: 0.0,
            y_origin: 0.0,
        },
        "ScaleDialog applies field changes only after OK"
    );
    assert_eq!(app.set_scale.distance, 0.0);
    assert_eq!(app.set_scale.known, 0.0);
    assert_eq!(app.set_scale.aspect, 1.0);
    assert_eq!(app.set_scale.unit, "pixel");
    click_button(&ctx, &mut app, "OK");
    assert!(!app.set_scale.open);
    assert_eq!(app.session.active_doc().unwrap().calibration, Calibration::default());
    assert_eq!(app.session.macro_recorder.last().map(String::as_str), Some("Image.removeScale;"));
}

#[test]
fn cancel_after_remove_scale_button_preserves_calibration() {
    let ctx = Context::default();
    let mut app = app_with_ramp();
    let scaled = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.25,
        pixel_depth: 1.0,
        unit: "mm".to_string(),
        invert_y: false,
        x_origin: 0.0,
        y_origin: 0.0,
    };
    app.session.active_doc_mut().unwrap().calibration = scaled.clone();
    app.perform(AppAction::SetScale, &ctx);
    click_button(&ctx, &mut app, "Click to Remove Scale");
    click_button(&ctx, &mut app, "Cancel");
    assert_eq!(app.session.active_doc().unwrap().calibration, scaled);
}

#[test]
fn non_finite_numbers_are_rejected_without_touching_calibration() {
    assert!(!set_scale_input_is_valid(f64::NAN, 10.0, 1.0));
    assert!(!set_scale_input_is_valid(20.0, f64::INFINITY, 1.0));
    assert!(!set_scale_input_is_valid(20.0, 10.0, f64::NAN));

    let ctx = Context::default();
    let mut app = app_with_ramp();
    app.perform(AppAction::SetScale, &ctx);
    app.set_scale.distance = f64::NAN;

    assert!(app.apply_set_scale_dialog().is_err());
    assert!(app.set_scale.open, "invalid input leaves the dialog open");
    assert_eq!(
        app.session.active_doc().unwrap().calibration,
        Calibration::default()
    );
    assert!(app.dialogs.last_error.is_some());
}

#[test]
fn set_scale_session_command_reports_updated() {
    let mut app = app_with_ramp();
    assert!(matches!(
        app.session.dispatch(Command::SetScale {
            distance: 20.0,
            known: 10.0,
            aspect: 1.0,
            unit: "mm".into(),
        }),
        Ok(CommandOutcome::Updated)
    ));
    assert!(matches!(
        app.session.dispatch(Command::RemoveScale),
        Ok(CommandOutcome::Updated)
    ));
}
