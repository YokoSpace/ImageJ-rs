use ij_bridge::JavaOracleBridge;
use ij_app::action::{AppAction, MenuKind, MENU_TREE};
use ij_app::app::ImageJApp;
use ij_core::{ImageDocument, MeasurementFlags, PixelPlane, ThresholdLevels};
use ij_session::{Command, CommandOutcome, Session};
use egui::{Context, Event, Key, Modifiers, RawInput};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap()
        .to_path_buf()
}

#[test]
fn analyze_histogram_menu_and_headless_action_open_window() {
    let analyze = MENU_TREE.iter().find(|node| node.label == "Analyze").unwrap();
    let MenuKind::Submenu(items) = analyze.kind else { panic!("Analyze submenu expected") };
    let histogram = items.iter().find(|node| node.label == "Histogram").unwrap();
    assert!(matches!(histogram.kind, MenuKind::Item(AppAction::Histogram)));
    assert_eq!(histogram.shortcut, Some("H"));

    let mut app = ImageJApp::new();
    let ctx = egui::Context::default();
    assert!(app.action_enabled(AppAction::Histogram).is_err());
    app.open_ramp_8bit("ramp", 16, 16).unwrap();
    app.perform(AppAction::Histogram, &ctx);
    let window = app.histogram_window.as_ref().expect("Histogram window");
    assert_eq!(window.title, "Histogram of ramp");
    assert_eq!(window.summary.count, 256);
    assert_eq!(window.summary.counts.len(), 256);
    assert_eq!(window.stats_labels()[0], "N: 256");
    assert!(window.stats_labels().iter().any(|s| s.starts_with("Mean: ")));
    assert_eq!(window.bin_tsv().lines().count(), 256);
    let _ = ctx.run(RawInput::default(), |ctx| app.update_ui(ctx));
}

#[test]
fn h_shortcut_opens_histogram_for_focused_image() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("focused", 8, 8).unwrap();
    let ctx = Context::default();
    let input = RawInput {
        events: vec![Event::Key {
            key: Key::H,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| app.handle_image_shortcuts(ctx, id));
    assert_eq!(app.histogram_window.as_ref().unwrap().title, "Histogram of focused");
}

fn assert_near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}

#[test]
fn histogram_command_matches_java_oracle_for_gray8_gray16_gray32_and_roi() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for (name, plane, mode) in [
        ("Gray8", PixelPlane::Gray8(vec![1, 1, 2, 3, 4, 5]), 1.0),
        (
            "Gray16",
            PixelPlane::Gray16(vec![1000, 1000, 1200, 1300, 1400, 1500]),
            1000.0,
        ),
        (
            "Gray32",
            PixelPlane::Gray32(vec![1.0, 1.0, 2.0, 3.0, 4.0, 5.0]),
            1.0,
        ),
    ] {
        for roi in [false, true] {
            let mut doc = ImageDocument::new(name, 3, 2, plane.clone()).unwrap();
            if roi {
                doc.set_rect_roi(0, 0, 2, 2).unwrap();
            }
            let oracle = bridge
                .run_macro_on_image(
                    &doc,
                    "getStatistics(area,mean,min,max,std); \
                     getHistogram(values,counts,256); \
                     print(\"STATS:\"+d2s(area,9)+\",\"+d2s(mean,9)+\",\"+d2s(std,9)+\",\"+d2s(min,9)+\",\"+d2s(max,9)); \
                     print(\"HIST\"); Array.print(counts);",
                )
                .unwrap();
            let mut lines = oracle.stdout.lines();
            let stats_line = lines.find(|line| line.starts_with("STATS:")).unwrap();
            let expected = stats_line[6..]
                .split(',')
                .map(|s| s.parse::<f64>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(expected.len(), 5);
            assert_eq!(lines.next(), Some("HIST"));
            let expected_counts = lines
                .next()
                .unwrap()
                .split(',')
                .map(|s| s.trim().parse::<u64>().unwrap())
                .collect::<Vec<_>>();

            let mut session = Session::new();
            session.insert_doc(doc);
            let before = session.active_doc().unwrap().clone();
            let outcome = session.dispatch(Command::Histogram).unwrap();
            let CommandOutcome::HistogramComputed(histogram) = outcome else {
                panic!("HistogramComputed expected");
            };
            assert_eq!(histogram.counts.as_slice(), expected_counts.as_slice(), "{name} roi={roi}");
            assert_eq!(histogram.count, expected_counts.iter().sum::<u64>());
            assert_near(histogram.mean, expected[1]);
            assert_near(histogram.std_dev, expected[2]);
            assert_near(histogram.min, expected[3]);
            assert_near(histogram.max, expected[4]);
            // FloatStatistics.getMode() adds half a bin width to dmode
            // (FloatStatistics.java 130-135); byte/short dmode is a raw value.
            let expected_mode = if name == "Gray32" {
                expected[3] + (expected[4] - expected[3]) / 512.0
            } else {
                mode
            };
            assert_near(histogram.mode, expected_mode);
            assert_eq!(session.active_doc().unwrap(), &before);
            assert_eq!(session.macro_recorder.last().map(String::as_str), Some("run(\"Histogram\");"));
        }
    }
}

#[test]
fn limit_to_threshold_limits_histogram_counts_without_changing_pixels() {
    // HistogramWindow.java 120-140 passes LIMIT into getStatistics; the
    // statistics classes count only values inside the processor threshold.
    for (plane, lower, upper) in [
        (PixelPlane::Gray8(vec![1, 1, 2, 3, 4, 5]), 2.0, 4.0),
        (PixelPlane::Gray16(vec![1000, 1000, 1200, 1300, 1400, 1500]), 1200.0, 1400.0),
        (PixelPlane::Gray32(vec![1.0, 1.0, 2.0, 3.0, 4.0, 5.0]), 2.0, 4.0),
    ] {
        let mut doc = ImageDocument::new("limited", 3, 2, plane).unwrap();
        doc.threshold = Some(ThresholdLevels { lower, upper });
        let mut session = Session::new();
        session.insert_doc(doc.clone());
        session.results.config.flags |= MeasurementFlags::LIMIT;
        let CommandOutcome::HistogramComputed(histogram) = session.dispatch(Command::Histogram).unwrap() else {
            panic!("HistogramComputed expected")
        };
        assert_eq!(histogram.count, 3);
        assert_eq!(histogram.counts.iter().sum::<u64>(), 3);
        assert_eq!(session.active_doc().unwrap(), &doc);
    }
}
