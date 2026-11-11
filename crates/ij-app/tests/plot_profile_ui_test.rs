//! Headless coverage for `Analyze > Plot Profile` (roadmap M6b).
//!
//! Expected strings and samples come from the Java reference, never from the
//! Rust output:
//! - `ProfilePlot.getPlot` (original_imagej/ij/gui/ProfilePlot.java 116-141)
//!   builds `"Plot of "+getShortTitle(imp)` and `xValues[i] = (float)(i*xInc)`.
//! - `ProfilePlot.getShortTitle` (ProfilePlot.java 143-149) strips a trailing
//!   extension only when the dot is not the first character and the dot plus
//!   extension is at most five characters.
//! - `ProfilePlot.getColumnAverageProfile` (ProfilePlot.java 210-225) averages
//!   each column and leaves `0/0 = NaN` for a column with no in-bounds sample.
//!
//! The hand-derived samples were cross-checked against the 1.54u oracle:
//! `makeRectangle(0,0,4,2); getProfile()` prints `20, 30, 40, 50` and
//! `makeRectangle(2,0,4,3); getProfile()` prints `60, 70, NaN, NaN`.

use egui::{Context, RawInput};
use ij_app::action::{AppAction, MenuKind, MENU_TREE};
use ij_app::app::{plot_profile_runs, plot_profile_short_title, ImageJApp};
use ij_core::{ImageDocument, PixelPlane};

/// 4x3 8-bit image whose rows are `0,10,20,30`, `40,50,60,70`, `80,90,100,110`.
fn ramp_doc(title: &str) -> ImageDocument {
    let mut pixels = Vec::new();
    for row in 0..3u8 {
        for col in 0..4u8 {
            pixels.push(row * 40 + col * 10);
        }
    }
    ImageDocument::new(title, 4, 3, PixelPlane::Gray8(pixels)).unwrap()
}

fn app_with(doc: ImageDocument) -> ImageJApp {
    let mut app = ImageJApp::new();
    app.session.insert_doc(doc);
    app
}

fn assert_close(actual: (f32, f32), expected: (f32, f32)) {
    assert!(
        (actual.0 - expected.0).abs() < 1e-6 && (actual.1 - expected.1).abs() < 1e-6,
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn analyze_menu_wires_plot_profile_with_k_shortcut() {
    // Menus.java assigns "K" to Analyze>Plot Profile.
    let analyze = MENU_TREE
        .iter()
        .find(|node| node.label == "Analyze")
        .expect("Analyze menu");
    let MenuKind::Submenu(items) = analyze.kind else {
        panic!("Analyze submenu expected")
    };
    let node = items
        .iter()
        .find(|node| node.label == "Plot Profile")
        .expect("Plot Profile menu item");
    assert!(matches!(node.kind, MenuKind::Item(AppAction::PlotProfile)));
    assert_eq!(node.shortcut, Some("K"));
}

#[test]
fn plot_profile_action_requires_rect_or_line_selection() {
    let app = ImageJApp::new();
    assert_eq!(
        app.action_enabled(AppAction::PlotProfile).unwrap_err(),
        "Requires an active image"
    );

    // ProfilePlot.java 53-56: no selection at all.
    assert_eq!(
        app_with(ramp_doc("no-roi"))
            .action_enabled(AppAction::PlotProfile)
            .unwrap_err(),
        "Selection required."
    );

    // ProfilePlot.java 57-62: any non-line, non-rectangle selection.
    let mut oval = ramp_doc("oval");
    oval.set_oval_roi(0, 0, 2, 2).unwrap();
    assert_eq!(
        app_with(oval)
            .action_enabled(AppAction::PlotProfile)
            .unwrap_err(),
        "Line or rectangular selection required."
    );

    let mut rect = ramp_doc("rect");
    rect.set_rect_roi(0, 0, 4, 2).unwrap();
    assert!(app_with(rect)
        .action_enabled(AppAction::PlotProfile)
        .is_ok());

    let mut line = ramp_doc("line");
    line.set_line_roi(0, 0, 3, 0);
    assert!(app_with(line)
        .action_enabled(AppAction::PlotProfile)
        .is_ok());
}

#[test]
fn plot_profile_window_uses_java_short_title_and_calibrated_axes() {
    let mut doc = ramp_doc("ramp.tif");
    doc.set_rect_roi(0, 0, 4, 2).unwrap();
    doc.calibration.pixel_width = 0.5;
    doc.calibration.unit = "micron".to_string();
    let mut app = app_with(doc);

    app.perform(AppAction::PlotProfile, &Context::default());

    let window = app
        .plot_profile_window
        .as_ref()
        .expect("Plot Profile window");
    // ProfilePlot.java 143-149: ".tif" is four characters, so it is stripped.
    assert_eq!(window.title, "Plot of ramp");
    // ProfilePlot.java 117-119: "Distance ("+units+")" with Calibration.getUnits().
    assert_eq!(window.x_label, "Distance (microns)");
    // ProfilePlot.java 68: yLabel = cal.getValueUnit(); uncalibrated images use
    // Calibration.DEFAULT_VALUE_UNIT = "Gray Value" (Calibration.java 12).
    assert_eq!(window.y_label, "Gray Value");
    // ProfilePlot.java 210-225: column averages over rows 0-1.
    assert_eq!(window.y_values, vec![20.0, 30.0, 40.0, 50.0]);
    // ProfilePlot.java 120-122: xValues[i] = (float)(i*xInc), xInc=pixelWidth.
    assert_eq!(window.x_values, vec![0.0, 0.5, 1.0, 1.5]);
    assert_eq!(window.x_range(), (0.0, 1.5));
    assert_eq!(window.y_range(), (20.0, 50.0));
}

#[test]
fn plot_profile_window_lists_every_sample_pair() {
    let mut doc = ramp_doc("ramp.tif");
    doc.set_rect_roi(0, 0, 4, 2).unwrap();
    doc.calibration.pixel_width = 0.5;
    doc.calibration.unit = "micron".to_string();
    let mut app = app_with(doc);

    app.perform(AppAction::PlotProfile, &Context::default());

    let window = app.plot_profile_window.as_ref().unwrap();
    // PlotWindow.java 663-668 shows Plot.getResultsTable (Plot.java 3852-3900),
    // which lists one X/Y row per sample; ResultsTable formats with d2s.
    assert_eq!(
        window.values_tsv(),
        "X\tY\n0\t20\n0.500\t30\n1\t40\n1.500\t50\n"
    );
    assert_eq!(window.values_tsv().lines().count(), 5);
}

#[test]
fn plot_profile_perform_records_one_command_and_preserves_document() {
    let mut doc = ramp_doc("ramp.tif");
    doc.set_rect_roi(0, 0, 4, 2).unwrap();
    let mut app = app_with(doc);
    let before = app.session.active_doc().unwrap().clone();
    let recorded = app.session.macro_recorder.len();
    let rows = app.session.results.rows.len();

    app.perform(AppAction::PlotProfile, &Context::default());

    assert_eq!(app.session.macro_recorder.len(), recorded + 1);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Plot Profile\");")
    );
    assert_eq!(app.session.active_doc().unwrap(), &before);
    assert_eq!(app.session.results.rows.len(), rows);
    let id = app.session.active_image_id().unwrap();
    assert!(!app.session.is_dirty(id));
}

#[test]
fn plot_profile_short_title_matches_java_get_short_title() {
    // ProfilePlot.java 143-149: strip only when the dot is not the first
    // character and the dot plus extension is at most five characters.
    for (title, expected) in [
        ("ramp", "ramp"),
        ("ramp.tif", "ramp"),
        ("ramp.tiff", "ramp"),
        ("photo.jpeg", "photo"),
        ("a.b", "a"),
        ("a.longext", "a.longext"),
        (".hidden", ".hidden"),
        // Last dot at index 12 of 17 characters: 17-12 = 5, so Java strips it.
        ("no.extension.here", "no.extension"),
        // Last dot at index 4 of 10 characters: 10-4 = 6, so Java keeps it.
        ("file.tiffx", "file.tiffx"),
    ] {
        assert_eq!(plot_profile_short_title(title), expected, "{title}");
    }
}

#[test]
fn plot_profile_runs_break_at_nan_samples() {
    // A NaN sample (ProfilePlot.java 210-225 leaves 0/0 for a column with no
    // in-bounds samples) must break the polyline instead of connecting through
    // an invalid coordinate.
    let runs = plot_profile_runs(
        &[0.0, 1.0, 2.0, 3.0],
        &[10.0, f64::NAN, 30.0, 40.0],
        (0.0, 3.0),
        (10.0, 40.0),
    );
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].len(), 1);
    assert_eq!(runs[1].len(), 2);
    assert_close(runs[0][0], (0.0, 0.0));
    assert_close(runs[1][0], (2.0 / 3.0, 2.0 / 3.0));
    assert_close(runs[1][1], (1.0, 1.0));
}

#[test]
fn plot_profile_window_renders_headlessly_without_mutating_session() {
    let mut doc = ramp_doc("clipped.tif");
    // Columns 4-5 lie outside the 4-pixel width, so their averages are NaN
    // (ProfilePlot.java 210-225) and the drawn line must break there.
    doc.set_rect_roi(2, 0, 4, 3).unwrap();
    let mut app = app_with(doc);
    app.perform(AppAction::PlotProfile, &Context::default());

    {
        let window = app.plot_profile_window.as_ref().unwrap();
        // ProfilePlot.java 143-149 strips ".tif" from "clipped.tif".
        assert_eq!(window.title, "Plot of clipped");
        assert_eq!(window.y_values[0], 60.0);
        assert_eq!(window.y_values[1], 70.0);
        assert!(window.y_values[2].is_nan());
        assert!(window.y_values[3].is_nan());
        assert_eq!(window.runs().len(), 1);
        assert_eq!(window.runs()[0].len(), 2);
    }

    let before = app.session.active_doc().unwrap().clone();
    let recorded = app.session.macro_recorder.len();
    let ctx = Context::default();
    let _ = ctx.run(RawInput::default(), |ctx| app.update_ui(ctx));

    assert!(app.plot_profile_window.is_some());
    assert_eq!(app.session.active_doc().unwrap(), &before);
    assert_eq!(app.session.macro_recorder.len(), recorded);
}
