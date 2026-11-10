//! Session-level integration tests for the narrow Java `PlugInFilter` path.
//!
//! These tests exercise `Session::dispatch(Command::RunPluginFilter)` end to
//! end against the real fixture JAR and the unmodified ImageJ 1.54u
//! `PlugInFilterRunner` reference. They deliberately cover only the supported
//! subset (single-plane 8-bit grayscale, optional rectangular ROI, default
//! metadata) plus the transactional failure paths.

use ij_bridge::plugin::{PluginBridge, PluginBridgeError};
use ij_core::{Calibration, DisplayRange, ImageDocument, PixelPlane, Roi, ThresholdLevels};
use ij_io::{load_tiff, save_tiff};
use ij_session::{Command, CommandOutcome, ImageId, Session, SessionError};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Duration;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("locate repo root")
        .to_path_buf()
}

fn fixture_jar_path() -> PathBuf {
    repo_root().join("tools/plugin-host/fixtures/fixtures.jar")
}

fn plugin_bridge() -> PluginBridge {
    PluginBridge::locate_from_repo_root(repo_root()).expect("locate plugin bridge")
}

fn gray8(doc: &ImageDocument) -> &[u8] {
    match doc.plane() {
        PixelPlane::Gray8(pixels) => pixels,
        other => panic!("expected 8-bit grayscale output, got {other:?}"),
    }
}

/// Runs the same fixture through ImageJ's own `PlugInFilterRunner` (the
/// reference path in `tools/plugin-host/run_reference_filter.sh`) without the
/// bridge's validation or TIFF-transfer logic.
fn run_reference_filter(image: &ImageDocument, class_name: &str, arg: &str) -> ImageDocument {
    let tmp = tempfile::tempdir().expect("reference tempdir");
    let input = tmp.path().join("reference-input.tif");
    let output = tmp.path().join("reference-output.tif");

    let mut transfer = image.clone();
    transfer.roi = None;
    save_tiff(&input, &transfer).expect("save reference input");

    let plugins_dir = fixture_jar_path()
        .parent()
        .expect("fixture jar parent")
        .to_path_buf();
    let mut cmd =
        ProcessCommand::new(repo_root().join("tools/plugin-host/run_reference_filter.sh"));
    cmd.arg("--plugins-dir")
        .arg(plugins_dir)
        .arg("--class")
        .arg(class_name)
        .arg("--arg")
        .arg(arg)
        .arg("--input-image")
        .arg(&input)
        .arg("--output-image")
        .arg(&output);
    if let Some(Roi::Rect(rect)) = &image.roi {
        cmd.arg("--roi").arg(format!(
            "{},{},{},{}",
            rect.x, rect.y, rect.width, rect.height
        ));
    }

    let child = cmd.output().expect("run reference filter");
    assert!(
        child.status.success(),
        "reference oracle failed: {}",
        String::from_utf8_lossy(&child.stderr)
    );

    let mut doc = load_tiff(&output).expect("load reference output");
    doc.title = image.title.clone();
    doc.roi = image.roi.clone();
    doc
}

fn session_with_bridge() -> Session {
    Session::new().with_plugin_bridge(plugin_bridge())
}

/// Creates a session whose active image is an 8-bit ramp built through the
/// public command surface and then marked clean.
fn session_with_ramp(title: &str, width: u32, height: u32) -> (Session, ImageId) {
    session_with_ramp_and_bridge(plugin_bridge(), title, width, height)
}

fn session_with_ramp_and_bridge(
    bridge: PluginBridge,
    title: &str,
    width: u32,
    height: u32,
) -> (Session, ImageId) {
    let mut session = Session::new().with_plugin_bridge(bridge);
    let id = match session
        .dispatch(Command::NewRamp8Bit {
            title: title.into(),
            width,
            height,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("expected ImageOpened, got {other:?}"),
    };
    session.set_dirty(id, false);
    (session, id)
}

fn run_filter_command(class_name: &str) -> Command {
    Command::RunPluginFilter {
        class_name: class_name.into(),
        classpath: vec![fixture_jar_path()],
        arg: String::new(),
    }
}

/// Everything that must be transactionally unchanged when a plugin run fails.
#[derive(Debug, PartialEq)]
struct TransactionState {
    image: ImageDocument,
    dirty: bool,
    results: String,
    macro_recorder: Vec<String>,
}

fn transaction_state(session: &Session, id: ImageId) -> TransactionState {
    TransactionState {
        image: session.get_image(id).expect("image").clone(),
        dirty: session.is_dirty(id),
        results: session.results.to_tsv_basic(),
        macro_recorder: session.macro_recorder.clone(),
    }
}

fn assert_unchanged(session: &Session, id: ImageId, before: &TransactionState, label: &str) {
    let after = transaction_state(session, id);
    assert_eq!(&after, before, "{label}: session state changed on failure");
}

#[test]
fn session_supported_rect_roi_filter_matches_reference_and_preserves_state() {
    let (mut session, id) = session_with_ramp("fixture-input", 32, 24);
    session
        .dispatch(Command::MakeRectRoi {
            x: 4,
            y: 3,
            width: 10,
            height: 8,
        })
        .unwrap();

    let before = session.get_image(id).unwrap().clone();
    let results_before = session.results.to_tsv_basic();
    let macro_len = session.macro_recorder.len();

    let outcome = session
        .dispatch(Command::RunPluginFilter {
            class_name: "fixtures.RectInvertFilter".into(),
            classpath: vec![fixture_jar_path()],
            arg: String::new(),
        })
        .expect("supported rect ROI filter should run");

    let reference = run_reference_filter(&before, "fixtures.RectInvertFilter", "");
    let after = session.get_image(id).unwrap();

    // Pixel-exact against the unmodified ImageJ PlugInFilterRunner.
    assert_eq!(
        gray8(after),
        gray8(&reference),
        "session pixels differ from the ImageJ reference"
    );

    // Document state the narrow runner does not own survives the run.
    assert_eq!(after.title, before.title);
    assert_eq!(after.width, before.width);
    assert_eq!(after.height, before.height);
    assert_eq!(after.slices.len(), 1);
    assert_eq!(after.n_channels, before.n_channels);
    assert_eq!(after.n_slices, before.n_slices);
    assert_eq!(after.n_frames, before.n_frames);
    assert_eq!(after.current_slice, before.current_slice);
    assert_eq!(after.position, before.position);
    assert_eq!(after.calibration, before.calibration);
    assert_eq!(after.display_range, before.display_range);
    assert_eq!(after.roi, before.roi);
    assert_eq!(after.threshold, before.threshold);

    // PlugInFilterRunner snapshots a single image before run(ip), so Undo
    // restores the pre-filter plane (PlugInFilterRunner.java 107-120).
    assert_eq!(after.snapshot.as_deref(), Some(before.slices.as_slice()));
    assert!(session.is_dirty(id), "a changed image must be dirty");
    assert_eq!(
        session.results.to_tsv_basic(),
        results_before,
        "plugin filters must not mutate the results table"
    );
    assert_eq!(
        &session.macro_recorder[macro_len..],
        &[
            "// Java plugin filter fixtures.RectInvertFilter: macro replay is not supported"
                .to_string()
        ]
    );

    match outcome {
        CommandOutcome::PluginFiltered(result) => {
            assert_eq!(result.class_name, "fixtures.RectInvertFilter");
            assert_eq!(result.arg, "");
            assert_eq!(result.filter_flags, 1 | 64);
            assert_eq!(result.run_invocations, 1);
            assert!(!result.final_setup_called);
        }
        other => panic!("expected PluginFiltered outcome, got {other:?}"),
    }

    // Undo really restores the pre-filter pixels.
    session.dispatch(Command::Undo).unwrap();
    assert_eq!(
        session.get_image(id).unwrap().plane(),
        before.plane(),
        "Undo did not restore the pre-filter pixels"
    );
}

#[test]
fn session_supported_filter_without_roi_matches_reference() {
    let (mut session, id) = session_with_ramp("no-roi", 17, 13);
    let before = session.get_image(id).unwrap().clone();

    session
        .dispatch(run_filter_command("fixtures.RectInvertFilter"))
        .expect("full-image filter should run");

    let reference = run_reference_filter(&before, "fixtures.RectInvertFilter", "");
    assert_eq!(gray8(session.get_image(id).unwrap()), gray8(&reference));
    assert_eq!(session.get_image(id).unwrap().roi, None);
    assert!(session.get_image(id).unwrap().snapshot.is_some());
}

#[test]
fn session_filter_error_leaves_image_undo_dirty_results_and_macro_unchanged() {
    let (mut session, id) = session_with_ramp("failing", 32, 24);
    // Establish a real undo snapshot and a populated results table first.
    session.dispatch(Command::Invert).unwrap();
    session.dispatch(Command::Measure).unwrap();
    assert!(session.get_image(id).unwrap().snapshot.is_some());
    assert!(!session.results.to_tsv_basic().is_empty());

    let before = transaction_state(&session, id);
    let result = session.dispatch(run_filter_command("fixtures.FailingFilter"));

    match result {
        Err(SessionError::Plugin(PluginBridgeError::FilterException {
            exception_type,
            message,
        })) => {
            assert!(exception_type.contains("IllegalStateException"));
            assert!(message.contains("fixture-failure-after-mutation"));
        }
        other => panic!("expected FilterException, got {other:?}"),
    }
    assert_unchanged(&session, id, &before, "filter exception");
}

#[test]
fn session_filter_unsupported_inputs_leave_state_unchanged() {
    let mut cases: Vec<(&str, ImageDocument)> = Vec::new();

    cases.push((
        "16-bit",
        ImageDocument::new("sixteen", 32, 24, PixelPlane::Gray16(vec![0; 32 * 24]))
            .expect("16-bit"),
    ));
    cases.push((
        "stack",
        ImageDocument::new_stack(
            "stack",
            32,
            24,
            vec![
                PixelPlane::Gray8(vec![0; 32 * 24]),
                PixelPlane::Gray8(vec![0; 32 * 24]),
            ],
        )
        .expect("stack"),
    ));

    let mut oval = ImageDocument::new_ramp_8bit("oval", 32, 24).expect("oval");
    oval.set_oval_roi(4, 3, 10, 8).unwrap();
    cases.push(("oval ROI", oval));

    let mut line = ImageDocument::new_ramp_8bit("line", 32, 24).expect("line");
    line.set_line_roi(1, 2, 10, 8);
    cases.push(("line ROI", line));

    let mut clipped = ImageDocument::new_ramp_8bit("clipped", 32, 24).expect("clipped");
    clipped.set_rect_roi(30, 20, 10, 10).unwrap();
    cases.push(("out-of-bounds ROI", clipped));

    let mut calibration = ImageDocument::new_ramp_8bit("cal", 32, 24).expect("cal");
    calibration.calibration = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.5,
        pixel_depth: 1.0,
        unit: "um".to_string(),
        invert_y: false,
    };
    cases.push(("non-default calibration", calibration));

    let mut display_range = ImageDocument::new_ramp_8bit("range", 32, 24).expect("range");
    display_range.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    cases.push(("non-default display range", display_range));

    let mut threshold = ImageDocument::new_ramp_8bit("threshold", 32, 24).expect("threshold");
    threshold.threshold = Some(ThresholdLevels {
        lower: 10.0,
        upper: 200.0,
    });
    cases.push(("active threshold", threshold));

    for (label, doc) in cases {
        let mut session = session_with_bridge();
        let id = session.insert_doc(doc);
        session.set_dirty(id, false);
        // Seed a non-empty results table and macro recorder so a partial
        // mutation is visible.
        let seed = ImageDocument::new_ramp_8bit("seed", 4, 4)
            .unwrap()
            .measure()
            .unwrap();
        session.results.push(seed);
        session.macro_recorder.push("// baseline".into());

        let before = transaction_state(&session, id);
        let result = session.dispatch(run_filter_command("fixtures.RectInvertFilter"));

        match (label, &result) {
            ("active threshold", Err(SessionError::UnsupportedPluginFilterState(_))) => {}
            (_, Err(SessionError::Plugin(PluginBridgeError::UnsupportedFilterInput(_)))) => {}
            _ => panic!("{label}: expected a pre-execution rejection, got {result:?}"),
        }
        assert_unchanged(&session, id, &before, label);
    }
}

#[test]
fn session_filter_rejects_output_metadata_changes_without_mutating_state() {
    for (label, class_name) in [
        ("LUT", "fixtures.OutputLutFilter"),
        ("display range", "fixtures.OutputDisplayRangeFilter"),
        ("calibration", "fixtures.OutputCalibrationFilter"),
        ("selection", "fixtures.OutputRoiFilter"),
    ] {
        let (mut session, id) = session_with_ramp("metadata", 32, 24);
        session
            .dispatch(Command::MakeRectRoi {
                x: 2,
                y: 2,
                width: 6,
                height: 5,
            })
            .unwrap();
        let before = transaction_state(&session, id);

        match session.dispatch(run_filter_command(class_name)) {
            Err(SessionError::Plugin(PluginBridgeError::FilterRejected { .. })) => {}
            other => panic!("{label}: expected FilterRejected, got {other:?}"),
        }
        assert_unchanged(&session, id, &before, label);
    }
}

#[test]
fn session_filter_class_not_found_leaves_state_unchanged() {
    let (mut session, id) = session_with_ramp("missing", 16, 16);
    let before = transaction_state(&session, id);

    match session.dispatch(run_filter_command("fixtures.DoesNotExist")) {
        Err(SessionError::Plugin(PluginBridgeError::FilterClassNotFound { .. })) => {}
        other => panic!("expected FilterClassNotFound, got {other:?}"),
    }
    assert_unchanged(&session, id, &before, "class not found");
}

#[test]
fn session_filter_timeout_leaves_state_unchanged() {
    let (mut session, id) = session_with_ramp_and_bridge(
        plugin_bridge().with_timeout(Duration::from_nanos(1)),
        "timeout",
        16,
        16,
    );
    let before = transaction_state(&session, id);

    match session.dispatch(run_filter_command("fixtures.RectInvertFilter")) {
        Err(SessionError::Plugin(PluginBridgeError::Timeout { .. })) => {}
        other => panic!("expected Timeout, got {other:?}"),
    }
    assert_unchanged(&session, id, &before, "timeout");
}

#[test]
fn session_filter_requires_an_explicit_plugin_bridge() {
    let mut session = Session::new();
    let id = match session
        .dispatch(Command::NewRamp8Bit {
            title: "no-bridge".into(),
            width: 16,
            height: 16,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("expected ImageOpened, got {other:?}"),
    };
    session.set_dirty(id, false);
    let before = transaction_state(&session, id);

    match session.dispatch(run_filter_command("fixtures.RectInvertFilter")) {
        Err(SessionError::PluginBridgeUnavailable) => {}
        other => panic!("expected PluginBridgeUnavailable, got {other:?}"),
    }
    assert_unchanged(&session, id, &before, "no bridge");
}

#[test]
fn session_filter_rejects_empty_class_name_without_spawning_or_mutating() {
    let (mut session, id) = session_with_ramp("empty-class", 16, 16);
    let before = transaction_state(&session, id);

    match session.dispatch(run_filter_command("")) {
        Err(SessionError::InvalidPluginFilterRequest(_)) => {}
        other => panic!("expected InvalidPluginFilterRequest, got {other:?}"),
    }
    assert_unchanged(&session, id, &before, "empty class name");
}
