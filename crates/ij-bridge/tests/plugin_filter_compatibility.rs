use ij_bridge::plugin::{
    PluginBridge, PluginBridgeError, PluginFilterInputError, PluginFilterRequest,
};
use ij_core::{Calibration, DisplayRange, ImageDocument, PixelPlane, Roi};
use ij_io::{load_tiff, save_tiff};
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("locate repo root")
        .to_path_buf()
}

fn fixture_jar_path() -> PathBuf {
    repo_root().join("tools/plugin-host/fixtures/fixtures.jar")
}

fn bridge() -> PluginBridge {
    PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge")
}

/// A sample image whose ramp profile makes ROI mix-ups obvious: every column
/// has a different value.
fn sample_image() -> ImageDocument {
    let mut doc = ImageDocument::new_ramp_8bit("fixture-input", 32, 24).expect("sample image");
    doc.set_rect_roi(4, 3, 10, 8).expect("sample ROI");
    doc
}

/// A selection stored in an input TIFF is rejected by the narrow plugin host
/// unless the caller also passes an explicit `--roi`. Tests that exercise
/// unrelated host protocol paths must therefore start from an image without a
/// selection.
fn sample_image_without_roi() -> ImageDocument {
    ImageDocument::new_ramp_8bit("fixture-input", 32, 24).expect("sample image")
}

fn filter_request(class_name: &str, image: ImageDocument) -> PluginFilterRequest {
    PluginFilterRequest::new(class_name, "", image).with_classpath_entry(fixture_jar_path())
}

fn gray8(doc: &ImageDocument) -> &[u8] {
    match doc.plane() {
        PixelPlane::Gray8(pixels) => pixels,
        other => panic!("expected 8-bit grayscale output, got {other:?}"),
    }
}

/// Run the same fixture through ImageJ's own `IJ.runPlugIn(ImagePlus, ...)`
/// path (which constructs the real PlugInFilterRunner) without the spike's
/// validation or TIFF transfer logic.
fn run_reference_filter(request: &PluginFilterRequest) -> ImageDocument {
    let tmp = tempfile::tempdir().expect("reference tempdir");
    let input = tmp.path().join("reference-input.tif");
    let output = tmp.path().join("reference-output.tif");

    let mut transfer = request.image.clone();
    transfer.roi = None;
    save_tiff(&input, &transfer).expect("save reference input");

    let plugins_dir = fixture_jar_path()
        .parent()
        .expect("fixture jar parent")
        .to_path_buf();
    let mut cmd = Command::new(repo_root().join("tools/plugin-host/run_reference_filter.sh"));
    cmd.arg("--plugins-dir")
        .arg(plugins_dir)
        .arg("--class")
        .arg(&request.class_name)
        .arg("--arg")
        .arg(&request.arg)
        .arg("--input-image")
        .arg(&input)
        .arg("--output-image")
        .arg(&output);
    if let Some(Roi::Rect(rect)) = &request.image.roi {
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
    doc.title = request.image.title.clone();
    doc.roi = request.image.roi.clone();
    doc
}

fn assert_validation_error(
    result: Result<ij_bridge::plugin::PluginFilterRun, PluginBridgeError>,
    expected: impl Fn(&PluginFilterInputError) -> bool,
    label: &str,
) {
    match result {
        Err(PluginBridgeError::UnsupportedFilterInput(error)) => {
            assert!(expected(&error), "{label}: unexpected rejection {error:?}");
        }
        other => panic!("{label}: expected UnsupportedFilterInput, got {other:?}"),
    }
}

fn assert_filter_rejected(
    result: Result<ij_bridge::plugin::PluginFilterRun, PluginBridgeError>,
    expected_fragment: &str,
) {
    match result {
        Err(PluginBridgeError::FilterRejected { message }) => {
            assert!(
                message.contains(expected_fragment),
                "rejection should mention '{expected_fragment}': {message}"
            );
        }
        other => panic!("expected FilterRejected mentioning '{expected_fragment}', got {other:?}"),
    }
}

#[test]
fn filter_rect_roi_output_plane_matches_imagej_oracle() {
    let bridge = bridge();
    let request = filter_request("fixtures.RectInvertFilter", sample_image());

    let rust = bridge
        .run_filter(&request)
        .expect("run filter through bridge");
    let oracle = run_reference_filter(&request);

    // The complete output plane must match ImageJ pixel for pixel.
    assert_eq!(
        gray8(&rust.image),
        gray8(&oracle),
        "Rust bridge output plane differs from the ImageJ oracle"
    );
    assert_eq!(rust.image.width, oracle.width);
    assert_eq!(rust.image.height, oracle.height);

    // The bridge must preserve caller-visible document state that the runner
    // does not own: title and selection survive the TIFF round trip.
    assert_eq!(rust.image.title, "fixture-input");
    assert_eq!(rust.image.roi, request.image.roi);
    assert_eq!(
        rust.filter_flags,
        1 | 64,
        "host should report the fixture's DOES_8G|SUPPORTS_MASKING flags"
    );

    let before = gray8(&request.image);
    let after = gray8(&rust.image);
    for y in 0..24usize {
        for x in 0..32usize {
            let idx = y * 32 + x;
            let inside = (4..14).contains(&x) && (3..11).contains(&y);
            if inside {
                assert_eq!(after[idx], 255 - before[idx], "ROI pixel ({x}, {y})");
            } else {
                assert_eq!(after[idx], before[idx], "outside pixel ({x}, {y})");
            }
        }
    }
}

#[test]
fn filter_without_roi_matches_imagej_oracle() {
    let bridge = bridge();
    let doc = ImageDocument::new_ramp_8bit("no-roi", 17, 13).expect("image");
    let request = filter_request("fixtures.RectInvertFilter", doc);

    let rust = bridge.run_filter(&request).expect("run filter");
    let oracle = run_reference_filter(&request);
    assert_eq!(gray8(&rust.image), gray8(&oracle));
    assert_eq!(rust.image.roi, None);
}

#[test]
fn filter_rejects_non_8bit_image() {
    let bridge = bridge();
    let doc = ImageDocument::new("sixteen-bit", 32, 24, PixelPlane::Gray16(vec![0; 32 * 24]))
        .expect("16-bit image");
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", doc));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedImageType { .. }),
        "16-bit image",
    );
}

#[test]
fn filter_rejects_stack() {
    let bridge = bridge();
    let doc = ImageDocument::new_stack(
        "stack",
        32,
        24,
        vec![
            PixelPlane::Gray8(vec![0; 32 * 24]),
            PixelPlane::Gray8(vec![0; 32 * 24]),
        ],
    )
    .expect("stack");
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", doc));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedStack { .. }),
        "stack",
    );
}

#[test]
fn filter_rejects_non_rectangular_roi() {
    let bridge = bridge();
    let mut doc = ImageDocument::new_ramp_8bit("oval", 32, 24).expect("image");
    doc.set_oval_roi(4, 3, 10, 8).expect("oval ROI");
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", doc));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedRoi { .. }),
        "oval ROI",
    );
}

#[test]
fn filter_rejects_roi_outside_image_bounds() {
    let bridge = bridge();
    let mut doc = ImageDocument::new_ramp_8bit("clipped-roi", 32, 24).expect("image");
    doc.set_rect_roi(30, 20, 10, 10).expect("out-of-bounds ROI");
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", doc));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedRoi { .. }),
        "out-of-bounds ROI",
    );
}

#[test]
fn filter_rejects_non_default_calibration_metadata() {
    let bridge = bridge();
    let mut doc = sample_image();
    doc.calibration = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.5,
        pixel_depth: 1.0,
        unit: "um".to_string(),
        invert_y: false,
    };
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", doc));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedMetadata { .. }),
        "calibration metadata",
    );
}

#[test]
fn filter_rejects_non_default_display_range_metadata() {
    let bridge = bridge();
    let mut doc = sample_image();
    doc.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", doc));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedMetadata { .. }),
        "display range metadata",
    );
}

#[test]
fn filter_accepts_does_stacks_on_single_plane() {
    let bridge = bridge();
    let request = filter_request("fixtures.StackFilter", sample_image());

    let rust = bridge
        .run_filter(&request)
        .expect("DOES_STACKS on a single plane is a no-op capability in ImageJ");
    let oracle = run_reference_filter(&request);

    assert_eq!(gray8(&rust.image), gray8(&oracle));
    assert_eq!(
        rust.filter_flags,
        1 | 32,
        "host should report DOES_8G|DOES_STACKS"
    );
    assert_eq!(rust.run_invocations, 1);
    assert!(!rust.final_setup_called);
}

#[test]
fn filter_rejects_genuinely_unsupported_flag_instead_of_running_partially() {
    let bridge = bridge();
    let request = filter_request("fixtures.UnsupportedFlagFilter", sample_image());
    match bridge.run_filter(&request) {
        Err(PluginBridgeError::FilterRejected { message }) => {
            assert!(
                message.contains("CONVERT_TO_FLOAT"),
                "rejection should name the unsupported flag: {message}"
            );
        }
        other => panic!("expected FilterRejected for CONVERT_TO_FLOAT, got {other:?}"),
    }
}

#[test]
fn final_processing_does_not_overwrite_or_validate_initial_flags() {
    let bridge = bridge();
    let request = filter_request("fixtures.FinalProcessingFilter", sample_image());

    let rust = bridge
        .run_filter(&request)
        .expect("final setup return value must be ignored like PlugInFilterRunner");
    let oracle = run_reference_filter(&request);

    assert_eq!(gray8(&rust.image), gray8(&oracle));
    assert_eq!(
        rust.filter_flags,
        1 | 65_536,
        "host must keep the initial DOES_8G|FINAL_PROCESSING flags"
    );
    assert_eq!(rust.run_invocations, 1);
    assert!(
        rust.final_setup_called,
        "ImageJ should have called setup(\"final\", imp)"
    );
    assert!(
        rust.stdout.contains("FinalProcessingFilter: final setup"),
        "stdout was: {}",
        rust.stdout
    );
}

#[test]
fn done_filter_is_not_reported_as_pixel_processing() {
    let bridge = bridge();
    let request = filter_request("fixtures.DoneFilter", sample_image());

    match bridge.run_filter(&request) {
        Err(PluginBridgeError::FilterNotProcessed { message }) => {
            assert!(
                message.contains("DONE") && message.contains("not processed"),
                "message was: {message}"
            );
        }
        other => panic!("expected FilterNotProcessed for DONE, got {other:?}"),
    }
}

#[test]
fn host_protocol_reports_done_as_not_processed_without_output() {
    let _bridge = bridge();
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("input.tif");
    let output = tmp.path().join("output.tif");
    let result_file = tmp.path().join("result.json");
    save_tiff(&input, &sample_image_without_roi()).expect("save input image");

    let child = Command::new(repo_root().join("tools/plugin-host/run_host.sh"))
        .arg("--plugin-cp")
        .arg(fixture_jar_path())
        .arg("--class")
        .arg("fixtures.DoneFilter")
        .arg("--arg")
        .arg("")
        .arg("--input-image")
        .arg(&input)
        .arg("--output-image")
        .arg(&output)
        .arg("--run-id")
        .arg("done-run")
        .arg("--result-file")
        .arg(&result_file)
        .output()
        .expect("run plugin host");

    assert!(child.status.success());
    let raw = std::fs::read_to_string(&result_file).expect("read protocol result");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("parse protocol result");
    assert_eq!(json["status"], "not_processed");
    assert_eq!(json["run_invocations"], 0);
    assert!(
        !output.exists(),
        "DONE must not produce an output image that looks processed"
    );
}

#[test]
fn filter_rejects_output_lut_changes() {
    let bridge = bridge();
    let request = filter_request("fixtures.OutputLutFilter", sample_image());
    assert_filter_rejected(bridge.run_filter(&request), "LUT");
}

#[test]
fn filter_rejects_output_display_range_changes() {
    let bridge = bridge();
    let request = filter_request("fixtures.OutputDisplayRangeFilter", sample_image());
    assert_filter_rejected(bridge.run_filter(&request), "display range");
}

#[test]
fn filter_rejects_output_calibration_changes() {
    let bridge = bridge();
    let request = filter_request("fixtures.OutputCalibrationFilter", sample_image());
    assert_filter_rejected(bridge.run_filter(&request), "calibration");
}

#[test]
fn filter_rejects_output_roi_changes() {
    let bridge = bridge();
    let request = filter_request("fixtures.OutputRoiFilter", sample_image());
    assert_filter_rejected(bridge.run_filter(&request), "selection");
}

#[test]
fn filter_exception_is_reported_without_output_image() {
    let bridge = bridge();
    let request = filter_request("fixtures.FailingFilter", sample_image());
    match bridge.run_filter(&request) {
        Err(PluginBridgeError::FilterException {
            exception_type,
            message,
        }) => {
            assert!(
                exception_type.contains("IllegalStateException"),
                "exception type was {exception_type}"
            );
            assert!(
                message.contains("fixture-failure-after-mutation"),
                "message was {message}"
            );
        }
        other => panic!("expected FilterException, got {other:?}"),
    }
}

#[test]
fn failed_filter_never_writes_partial_output_file() {
    let _bridge = bridge(); // force fixture/host build before direct invocation

    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("input.tif");
    let output = tmp.path().join("output.tif");
    let result_file = tmp.path().join("result.json");
    save_tiff(&input, &sample_image_without_roi()).expect("save input image");

    // A pre-existing caller file must survive any failed filter run.
    std::fs::write(&output, b"preexisting-user-output").expect("seed output");

    let runner = repo_root().join("tools/plugin-host/run_host.sh");
    let child = Command::new(&runner)
        .arg("--plugin-cp")
        .arg(fixture_jar_path())
        .arg("--class")
        .arg("fixtures.FailingFilter")
        .arg("--arg")
        .arg("")
        .arg("--input-image")
        .arg(&input)
        .arg("--output-image")
        .arg(&output)
        .arg("--run-id")
        .arg("failure-run")
        .arg("--result-file")
        .arg(&result_file)
        .output()
        .expect("run plugin host");

    assert!(
        child.status.success(),
        "host should report plugin exceptions in protocol JSON, stderr: {}",
        String::from_utf8_lossy(&child.stderr)
    );
    let raw = std::fs::read_to_string(&result_file).expect("read protocol result");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("parse protocol result");
    assert_eq!(json["status"], "exception_thrown");
    assert_eq!(
        std::fs::read(&output).expect("preexisting output still exists"),
        b"preexisting-user-output",
        "a failed filter must not delete or overwrite a pre-existing output file"
    );
}

#[test]
fn rejected_output_metadata_preserves_preexisting_output() {
    let _bridge = bridge();
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("input.tif");
    let output = tmp.path().join("output.tif");
    let result_file = tmp.path().join("result.json");
    save_tiff(&input, &sample_image()).expect("save input image");
    std::fs::write(&output, b"preexisting-user-output").expect("seed output");

    let child = Command::new(repo_root().join("tools/plugin-host/run_host.sh"))
        .arg("--plugin-cp")
        .arg(fixture_jar_path())
        .arg("--class")
        .arg("fixtures.OutputLutFilter")
        .arg("--arg")
        .arg("")
        .arg("--input-image")
        .arg(&input)
        .arg("--output-image")
        .arg(&output)
        .arg("--run-id")
        .arg("lut-run")
        .arg("--result-file")
        .arg(&result_file)
        .output()
        .expect("run plugin host");

    assert!(child.status.success());
    let raw = std::fs::read_to_string(&result_file).expect("read protocol result");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("parse protocol result");
    assert_eq!(json["status"], "unsupported_filter");
    assert_eq!(
        std::fs::read(&output).expect("preexisting output still exists"),
        b"preexisting-user-output",
        "a rejected unrepresentable output must not replace the caller's file"
    );
}

#[test]
fn host_reports_missing_input_as_io_error_not_unsupported_filter() {
    let _bridge = bridge();
    let tmp = tempfile::tempdir().expect("tempdir");
    let result_file = tmp.path().join("result.json");
    let output = tmp.path().join("output.tif");

    let child = Command::new(repo_root().join("tools/plugin-host/run_host.sh"))
        .arg("--plugin-cp")
        .arg(fixture_jar_path())
        .arg("--class")
        .arg("fixtures.DummyPlugInFilter")
        .arg("--arg")
        .arg("")
        .arg("--input-image")
        .arg(tmp.path().join("does-not-exist.tif"))
        .arg("--output-image")
        .arg(&output)
        .arg("--run-id")
        .arg("io-run")
        .arg("--result-file")
        .arg(&result_file)
        .output()
        .expect("run plugin host");

    assert!(
        child.status.success(),
        "the host reports I/O failures in protocol JSON, stderr: {}",
        String::from_utf8_lossy(&child.stderr)
    );
    let raw = std::fs::read_to_string(&result_file).expect("read protocol result");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("parse protocol result");
    assert_eq!(json["status"], "io_error");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or_default()
            .contains("could not open"),
        "message was: {}",
        json["message"]
    );
    assert!(
        !output.exists(),
        "an I/O failure must not create an output file"
    );
}
