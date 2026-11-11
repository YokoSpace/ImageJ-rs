//! M4c-1: `Analyze > Set Measurements...` dialog and session wiring.
//!
//! Expected labels/order come from Analyzer's ordered `list`
//! (original_imagej/ij/plugin/filter/Analyzer.java 29-32) and the
//! `addCheckboxGroup` calls at Analyzer.java 154-200. Expected column headers
//! come from `ResultsTable.defaultHeadings` (ResultsTable.java 42-45).

use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::action::{AppAction, MenuKind, MENU_TREE};
use ij_app::app::{
    clamp_set_measurements_precision, set_measurements_slot, ImageJApp, SetMeasurementsDialog,
    SET_MEASUREMENTS_OPTIONS,
};
use ij_core::MeasurementFlags;

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

#[test]
fn analyze_menu_wires_set_measurements() {
    let item = analyze_menu()
        .iter()
        .find(|node| node.label == "Set Measurements...")
        .expect("Set Measurements item");
    assert!(matches!(
        item.kind,
        MenuKind::Item(AppAction::SetMeasurements)
    ));
    assert_eq!(item.shortcut, None);
}

#[test]
fn dialog_checkboxes_match_analyzer_labels_and_order() {
    // Analyzer.java 29-32 (`list`) and 154-200 (labels/states).
    let expected = [
        ("Area", MeasurementFlags::AREA),
        ("Mean gray value", MeasurementFlags::MEAN),
        ("Standard deviation", MeasurementFlags::STD_DEV),
        ("Modal gray value", MeasurementFlags::MODE),
        ("Min & max gray value", MeasurementFlags::MIN_MAX),
        ("Centroid", MeasurementFlags::CENTROID),
        ("Center of mass", MeasurementFlags::CENTER_OF_MASS),
        ("Perimeter", MeasurementFlags::PERIMETER),
        ("Bounding rectangle", MeasurementFlags::RECT),
        ("Fit ellipse", MeasurementFlags::ELLIPSE),
        ("Shape descriptors", MeasurementFlags::SHAPE_DESCRIPTORS),
        ("Feret's diameter", MeasurementFlags::FERET),
        ("Integrated density", MeasurementFlags::INTEGRATED_DENSITY),
        ("Median", MeasurementFlags::MEDIAN),
        ("Skewness", MeasurementFlags::SKEWNESS),
        ("Kurtosis", MeasurementFlags::KURTOSIS),
        ("Area_fraction", MeasurementFlags::AREA_FRACTION),
        ("Stack position", MeasurementFlags::STACK_POSITION),
        ("Limit to threshold", MeasurementFlags::LIMIT),
        ("Display label", MeasurementFlags::LABELS),
        ("Invert Y coordinates", MeasurementFlags::INVERT_Y),
        ("Scientific notation", MeasurementFlags::SCIENTIFIC_NOTATION),
        ("Add to overlay", MeasurementFlags::ADD_TO_OVERLAY),
        ("NaN empty cells", MeasurementFlags::NAN_EMPTY_CELLS),
    ];
    assert_eq!(SET_MEASUREMENTS_OPTIONS.len(), expected.len());
    for (option, (label, flag)) in SET_MEASUREMENTS_OPTIONS.iter().zip(expected) {
        assert_eq!(option.label, label);
        assert_eq!(option.flag, flag);
    }
}

#[test]
fn dialog_defaults_match_imagej_startup_state() {
    // Analyzer.java 40-42: AREA+MEAN+MIN_MAX, precision 3.
    let dialog = SetMeasurementsDialog::default();
    assert!(!dialog.open);
    assert_eq!(
        dialog.flags,
        MeasurementFlags::AREA | MeasurementFlags::MEAN | MeasurementFlags::MIN_MAX
    );
    assert_eq!(dialog.precision, 3);
    assert!(dialog.contains(MeasurementFlags::AREA));
    assert!(!dialog.contains(MeasurementFlags::STD_DEV));

    let unsupported: Vec<_> = SET_MEASUREMENTS_OPTIONS
        .iter()
        .filter(|option| !option.supported)
        .map(|option| (option.label, option.flag))
        .collect();
    assert_eq!(
        unsupported,
        [
            ("Feret's diameter", MeasurementFlags::FERET),
            ("Display label", MeasurementFlags::LABELS),
            ("Invert Y coordinates", MeasurementFlags::INVERT_Y),
            ("Add to overlay", MeasurementFlags::ADD_TO_OVERLAY),
            ("NaN empty cells", MeasurementFlags::NAN_EMPTY_CELLS),
        ]
    );
}

#[test]
fn checkbox_grid_fills_column_major_like_generic_dialog() {
    // GenericDialog.java 653-656: index = col*rows+row, so the left column of
    // the 10x2 group holds options 0..10 and the right column 10..18.
    assert_eq!(set_measurements_slot(0, 18, 10, 0, 0), Some(0));
    assert_eq!(set_measurements_slot(0, 18, 10, 0, 1), Some(10));
    assert_eq!(
        SET_MEASUREMENTS_OPTIONS[10].label, "Shape descriptors",
        "Analyzer.java 164 keeps Shape descriptors in the right column"
    );
    assert_eq!(set_measurements_slot(0, 18, 10, 8, 1), None);
    assert_eq!(set_measurements_slot(0, 18, 10, 9, 1), None);

    // Second addCheckboxGroup(3, 2, ...) group: Limit to threshold is index 18.
    assert_eq!(set_measurements_slot(18, 6, 3, 0, 0), Some(18));
    assert_eq!(set_measurements_slot(18, 6, 3, 0, 1), Some(21));
    assert_eq!(set_measurements_slot(18, 6, 3, 2, 1), Some(23));
    assert_eq!(
        SET_MEASUREMENTS_OPTIONS[21].label, "Scientific notation",
        "Analyzer.java 197 keeps Scientific notation in the right column"
    );
}

#[test]
fn decimal_places_clamps_to_imagej_range() {
    // Analyzer.java 219-221.
    for (raw, expected) in [(-3i64, 0u8), (0, 0), (3, 3), (9, 9), (15, 9)] {
        assert_eq!(clamp_set_measurements_precision(raw), expected);
    }
}

#[test]
fn perform_set_measurements_opens_dialog_with_session_options() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("measure", 8, 8).unwrap();

    app.perform(AppAction::SetMeasurements, &ctx);

    assert!(app.set_measurements.open);
    assert_eq!(
        app.set_measurements.flags,
        MeasurementFlags::AREA | MeasurementFlags::MEAN | MeasurementFlags::MIN_MAX
    );
    assert_eq!(app.set_measurements.precision, 3);
}

fn render_dialog(ctx: &Context, app: &mut ImageJApp, events: Vec<Event>) {
    let mut input = RawInput::default();
    input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(520.0, 560.0)));
    input.events = events;
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_set_measurements_content(ui));
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

fn click_option(ctx: &Context, app: &mut ImageJApp, label: &str) {
    render_dialog(ctx, app, vec![]);
    let pos = ImageJApp::set_measurements_option_rect(ctx, label)
        .unwrap_or_else(|| panic!("missing checkbox rect for {label}"))
        .center();
    click_at(ctx, app, pos);
}

fn click_button(ctx: &Context, app: &mut ImageJApp, label: &str) {
    render_dialog(ctx, app, vec![]);
    let pos = ImageJApp::set_measurements_button_rect(ctx, label)
        .unwrap_or_else(|| panic!("missing button rect for {label}"))
        .center();
    click_at(ctx, app, pos);
}

#[test]
fn enabling_stddev_and_median_adds_their_results_columns() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("measure", 16, 16).unwrap();
    app.perform(AppAction::SetMeasurements, &ctx);
    assert!(app.set_measurements.open);

    click_option(&ctx, &mut app, "Standard deviation");
    assert!(
        app.set_measurements.contains(MeasurementFlags::STD_DEV),
        "checkbox click did not toggle Standard deviation"
    );
    click_option(&ctx, &mut app, "Median");
    assert!(app.set_measurements.contains(MeasurementFlags::MEDIAN));

    click_button(&ctx, &mut app, "OK");
    assert!(!app.set_measurements.open, "OK must close the dialog");
    assert_eq!(
        app.session.results.config.flags,
        MeasurementFlags::AREA
            | MeasurementFlags::MEAN
            | MeasurementFlags::MIN_MAX
            | MeasurementFlags::STD_DEV
            | MeasurementFlags::MEDIAN
    );
    assert_eq!(app.session.results.config.precision, 3);
    // Analyzer.java 29-32 order, rendered by ij-session.
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Set Measurements...\", \"area mean standard min median redirect=None decimal=3\");")
    );

    app.perform(AppAction::Measure, &ctx);

    // ResultsTable.defaultHeadings order (ResultsTable.java 42-45): StdDev
    // precedes Min/Max, Median follows IntDen.
    let tsv = app.session.results.to_tsv();
    let header = tsv.lines().next().expect("results header");
    assert_eq!(header, " \tArea\tMean\tStdDev\tMin\tMax\tMedian");
}

#[test]
fn cancel_discards_dialog_edits() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("measure", 8, 8).unwrap();
    let before = app.session.results.config;
    app.perform(AppAction::SetMeasurements, &ctx);

    click_option(&ctx, &mut app, "Standard deviation");
    assert!(app.set_measurements.contains(MeasurementFlags::STD_DEV));

    click_button(&ctx, &mut app, "Cancel");
    assert!(!app.set_measurements.open);
    assert_eq!(
        app.session.results.config, before,
        "Cancel must not change the session options (Analyzer.java 216-217)"
    );
    assert!(app
        .session
        .macro_recorder
        .iter()
        .all(|record| !record.starts_with("run(\"Set Measurements...\"")));
}

#[test]
fn dialog_edits_survive_reopen_after_ok() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("measure", 8, 8).unwrap();
    app.perform(AppAction::SetMeasurements, &ctx);
    click_option(&ctx, &mut app, "Median");
    click_button(&ctx, &mut app, "OK");

    app.perform(AppAction::SetMeasurements, &ctx);
    assert!(app.set_measurements.open);
    assert!(
        app.set_measurements.contains(MeasurementFlags::MEDIAN),
        "reopened dialog must reflect the stored session options"
    );
}
