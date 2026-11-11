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

fn gray16(doc: &ImageDocument, slice: usize) -> &[u16] {
    match &doc.slices[slice] {
        PixelPlane::Gray16(pixels) => pixels,
        other => panic!("expected 16-bit grayscale output, got {other:?}"),
    }
}

fn gray32(doc: &ImageDocument, slice: usize) -> &[f32] {
    match &doc.slices[slice] {
        PixelPlane::Gray32(pixels) => pixels,
        other => panic!("expected 32-bit float output, got {other:?}"),
    }
}

fn rgb(doc: &ImageDocument, slice: usize) -> &[u32] {
    match &doc.slices[slice] {
        PixelPlane::ColorRgb(pixels) => pixels,
        other => panic!("expected RGB output, got {other:?}"),
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
    if request.image.slices.len() > 1 {
        // ImageJ's PlugInFilterRunner processes the current slice when the
        // filter lacks DOES_STACKS, so the oracle must pin the same slice.
        cmd.arg("--current-slice")
            .arg(request.image.current_slice.to_string());
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

/// A 16-bit image whose ramp and display range make transfer bugs obvious.
fn sample_image_16bit() -> ImageDocument {
    let pixels: Vec<u16> = (0..32 * 24)
        .map(|i| ((i as u32 * 37) % 0x1_0000) as u16)
        .collect();
    let mut doc = ImageDocument::new("short-input", 32, 24, PixelPlane::Gray16(pixels))
        .expect("16-bit sample image");
    doc.display_range = DisplayRange {
        min: 12.0,
        max: 60_000.0,
    };
    doc.set_rect_roi(4, 3, 10, 8).expect("sample ROI");
    doc
}

/// A 32-bit float image with signed fractional values.
fn sample_image_32bit() -> ImageDocument {
    let pixels: Vec<f32> = (0..32 * 24)
        .map(|i| i as f32 * 0.5 - 100.0)
        .collect();
    let mut doc = ImageDocument::new("float-input", 32, 24, PixelPlane::Gray32(pixels))
        .expect("32-bit sample image");
    doc.display_range = DisplayRange {
        min: -100.0,
        max: 252.0,
    };
    doc.set_rect_roi(5, 6, 9, 7).expect("sample ROI");
    doc
}

/// An RGB image with distinct bytes per channel.
fn sample_image_rgb() -> ImageDocument {
    let pixels: Vec<u32> = (0..32u32 * 24)
        .map(|i| 0xff00_0000 | (((i * 13) & 0xff) << 16) | (((i * 7) & 0xff) << 8) | ((i * 29) & 0xff))
        .collect();
    let mut doc = ImageDocument::new("rgb-input", 32, 24, PixelPlane::ColorRgb(pixels))
        .expect("RGB sample image");
    doc.set_rect_roi(3, 4, 8, 9).expect("sample ROI");
    doc
}

/// A three-slice 8-bit stack with per-slice offset pixel values. The current
/// slice is 2, matching `position` after ImageJ's Slice slider.
fn sample_stack_8bit() -> ImageDocument {
    let planes: Vec<PixelPlane> = (0..3)
        .map(|k| {
            let pixels: Vec<u8> = (0..32usize * 24)
                .map(|i| ((i as u16 + k * 91) % 256) as u8)
                .collect();
            PixelPlane::Gray8(pixels)
        })
        .collect();
    let mut doc =
        ImageDocument::new_stack("stack-input", 32, 24, planes).expect("8-bit stack");
    doc.set_slice(2).expect("select slice 2");
    doc.set_rect_roi(4, 3, 10, 8).expect("sample ROI");
    doc
}

/// A three-slice 16-bit stack with per-slice distinct non-zero values.
fn sample_stack_16bit() -> ImageDocument {
    let planes: Vec<PixelPlane> = (0..3)
        .map(|k| {
            let pixels: Vec<u16> = (0..32usize * 24)
                .map(|i| ((i as u32 * 41 + k * 7_777) % 0x1_0000) as u16)
                .collect();
            PixelPlane::Gray16(pixels)
        })
        .collect();
    let mut doc =
        ImageDocument::new_stack("short-stack-input", 32, 24, planes).expect("16-bit stack");
    doc.display_range = DisplayRange {
        min: 8.0,
        max: 40_000.0,
    };
    doc.set_rect_roi(2, 2, 6, 5).expect("sample ROI");
    doc
}

#[test]
fn filter_runs_16bit_image_and_matches_imagej_oracle() {
    let bridge = bridge();
    let request = filter_request("fixtures.ShortInvertFilter", sample_image_16bit());

    let rust = bridge.run_filter(&request).expect("16-bit filter run");
    let oracle = run_reference_filter(&request);

    assert_eq!(
        gray16(&rust.image, 0),
        gray16(&oracle, 0),
        "16-bit output plane differs from the ImageJ oracle"
    );
    let before = gray16(&request.image, 0);
    let after = gray16(&rust.image, 0);
    for y in 0..24usize {
        for x in 0..32usize {
            let idx = y * 32 + x;
            let inside = (4..14).contains(&x) && (3..11).contains(&y);
            if inside {
                assert_eq!(after[idx], 0xffff - before[idx], "ROI pixel ({x}, {y})");
            } else {
                assert_eq!(after[idx], before[idx], "outside pixel ({x}, {y})");
            }
        }
    }
    assert_eq!(
        rust.image.display_range,
        request.image.display_range,
        "the 16-bit display range must survive the TIFF round trip"
    );
    assert_eq!(rust.image.roi, request.image.roi);
    assert_eq!(
        rust.filter_flags,
        4 | 64,
        "host should report the fixture's DOES_16|SUPPORTS_MASKING flags"
    );
    assert_eq!(rust.run_invocations, 1);
}

#[test]
fn filter_runs_32bit_image_and_matches_imagej_oracle() {
    let bridge = bridge();
    let request = filter_request("fixtures.FloatInvertFilter", sample_image_32bit());

    let rust = bridge.run_filter(&request).expect("32-bit filter run");
    let oracle = run_reference_filter(&request);

    assert_eq!(
        gray32(&rust.image, 0),
        gray32(&oracle, 0),
        "32-bit output plane differs from the ImageJ oracle"
    );
    // Multiplying by 2 is exact in f32, so the doubling is bit-exact.
    let before = gray32(&request.image, 0);
    let after = gray32(&rust.image, 0);
    for y in 0..24usize {
        for x in 0..32usize {
            let idx = y * 32 + x;
            let inside = (5..14).contains(&x) && (6..13).contains(&y);
            if inside {
                assert_eq!(after[idx], before[idx] * 2.0, "ROI pixel ({x}, {y})");
            } else {
                assert_eq!(after[idx], before[idx], "outside pixel ({x}, {y})");
            }
        }
    }
    assert_eq!(
        rust.image.display_range,
        request.image.display_range,
        "the 32-bit display range must survive the TIFF round trip"
    );
    assert_eq!(rust.filter_flags, 8 | 64, "DOES_32|SUPPORTS_MASKING");
    assert_eq!(rust.run_invocations, 1);
}

#[test]
fn filter_runs_rgb_image_and_matches_imagej_oracle() {
    let bridge = bridge();
    let request = filter_request("fixtures.RgbInvertFilter", sample_image_rgb());

    let rust = bridge.run_filter(&request).expect("RGB filter run");
    let oracle = run_reference_filter(&request);

    assert_eq!(
        rgb(&rust.image, 0),
        rgb(&oracle, 0),
        "RGB output plane differs from the ImageJ oracle"
    );
    // The TIFF transfer carries 24 bits per pixel; compare color bits with
    // the request plane and the full pixels with the (equally TIFF-loaded)
    // oracle above.
    let before = rgb(&request.image, 0);
    let after = rgb(&rust.image, 0);
    for y in 0..24usize {
        for x in 0..32usize {
            let idx = y * 32 + x;
            let inside = (3..11).contains(&x) && (4..13).contains(&y);
            let before_color = before[idx] & 0x00ff_ffff;
            let after_color = after[idx] & 0x00ff_ffff;
            if inside {
                assert_eq!(
                    after_color,
                    !before_color & 0x00ff_ffff,
                    "ROI pixel ({x}, {y})"
                );
            } else {
                assert_eq!(after_color, before_color, "outside pixel ({x}, {y})");
            }
        }
    }
    assert_eq!(rust.filter_flags, 16 | 64, "DOES_RGB|SUPPORTS_MASKING");
    assert_eq!(rust.run_invocations, 1);
}

#[test]
fn filter_rejects_bit_depth_missing_from_setup_flags_like_checkimageplus() {
    let bridge = bridge();

    // A 16-bit image offered to an 8-bit-only filter: ImageJ's wrongType
    // path reports the required image type instead of running anything.
    let result16 = bridge.run_filter(&filter_request(
        "fixtures.RectInvertFilter",
        sample_image_16bit(),
    ));
    assert_filter_rejected(result16, "8-bit grayscale");

    // The mirror case: an 8-bit image offered to a 16-bit-only filter.
    let result8 = bridge.run_filter(&filter_request(
        "fixtures.ShortInvertFilter",
        sample_image(),
    ));
    assert_filter_rejected(result8, "16-bit grayscale");
}

#[test]
fn stack_with_does_stacks_processes_every_slice_like_imagej_oracle() {
    let bridge = bridge();
    let request = filter_request("fixtures.StackInvertFilter", sample_stack_8bit());

    let rust = bridge.run_filter(&request).expect("stack filter run");
    let oracle = run_reference_filter(&request);

    // Every slice must match the oracle and have actually been processed.
    for slice in 0..3 {
        match (&rust.image.slices[slice], &oracle.slices[slice], &request.image.slices[slice]) {
            (
                PixelPlane::Gray8(processed),
                PixelPlane::Gray8(reference),
                PixelPlane::Gray8(original),
            ) => {
                assert_eq!(processed, reference, "slice {slice} differs from the oracle");
                assert_ne!(
                    processed, original,
                    "slice {slice} was not processed by the DOES_STACKS run"
                );
                for y in 3..11usize {
                    for x in 4..14usize {
                        let idx = y * 32 + x;
                        assert_eq!(
                            processed[idx],
                            255 - original[idx],
                            "ROI pixel ({x}, {y}) slice {slice}"
                        );
                    }
                }
            }
            other => panic!("expected 8-bit planes, got {other:?}"),
        }
    }
    assert_eq!(rust.filter_flags, 1 | 32 | 64, "DOES_8G|DOES_STACKS|SUPPORTS_MASKING");
    assert_eq!(
        rust.run_invocations, 3,
        "PlugInFilterRunner must call run(ip) once per slice for DOES_STACKS"
    );
}

#[test]
fn stack_without_does_stacks_processes_only_the_current_slice() {
    let bridge = bridge();
    let doc = sample_stack_8bit();
    assert_eq!(doc.current_slice, 2);
    let request = filter_request("fixtures.RectInvertFilter", doc);

    let rust = bridge.run_filter(&request).expect("current-slice run");
    let oracle = run_reference_filter(&request);

    for slice in 0..3 {
        let (after_plane, before_plane) = match (&rust.image.slices[slice], &request.image.slices[slice])
        {
            (PixelPlane::Gray8(a), PixelPlane::Gray8(b)) => (a, b),
            other => panic!("expected 8-bit planes, got {other:?}"),
        };
        match &oracle.slices[slice] {
            PixelPlane::Gray8(reference) => {
                assert_eq!(after_plane, reference, "slice {slice} differs from the oracle")
            }
            other => panic!("expected 8-bit planes, got {other:?}"),
        }
        if slice == 1 {
            assert_ne!(
                after_plane, before_plane,
                "the current slice (2) must be processed"
            );
            for y in 3..11usize {
                for x in 4..14usize {
                    let idx = y * 32 + x;
                    assert_eq!(
                        after_plane[idx],
                        255 - before_plane[idx],
                        "ROI pixel ({x}, {y}) on the current slice"
                    );
                }
            }
        } else {
            assert_eq!(
                after_plane, before_plane,
                "slice {} must be untouched without DOES_STACKS",
                slice + 1
            );
        }
    }
    assert_eq!(rust.run_invocations, 1, "a non-DOES_STACKS filter runs once");
    assert_eq!(rust.image.current_slice, 2, "current slice must be preserved");
}

#[test]
fn sixteen_bit_stack_with_does_stacks_matches_imagej_oracle() {
    let bridge = bridge();
    let request = filter_request("fixtures.ShortStackFilter", sample_stack_16bit());

    let rust = bridge.run_filter(&request).expect("16-bit stack run");
    let oracle = run_reference_filter(&request);

    for slice in 0..3 {
        match (&rust.image.slices[slice], &oracle.slices[slice]) {
            (PixelPlane::Gray16(a), PixelPlane::Gray16(b)) => assert_eq!(a, b, "slice {slice}"),
            other => panic!("expected 16-bit planes, got {other:?}"),
        }
    }
    assert_eq!(rust.filter_flags, 4 | 32 | 64, "DOES_16|DOES_STACKS|SUPPORTS_MASKING");
    assert_eq!(rust.run_invocations, 3);
    assert_eq!(
        rust.image.display_range,
        request.image.display_range,
        "the 16-bit stack display range must be preserved"
    );
}

#[test]
fn filter_rejects_hyperstack_and_inconsistent_stack_dimensions() {
    let bridge = bridge();

    // A C=2 hyperstack cannot be transferred as a plain ImageJ Z-stack.
    let hyperstack = ImageDocument::new_hyperstack(
        "hyper",
        8,
        8,
        vec![
            PixelPlane::Gray8(vec![0; 64]),
            PixelPlane::Gray8(vec![0; 64]),
        ],
        2,
        1,
        1,
    )
    .expect("hyperstack");
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", hyperstack));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedStack { .. }),
        "hyperstack",
    );

    // current_slice outside 1..=slices (or a position that disagrees with it)
    // is inconsistent document state.
    let mut bad_slice = sample_stack_8bit();
    bad_slice.current_slice = 4;
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", bad_slice));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedStackPosition { .. }),
        "current slice out of range",
    );
}

#[test]
fn filter_rejects_8bit_indexed_color_image() {
    let bridge = bridge();
    let doc = ImageDocument::new(
        "indexed",
        32,
        24,
        PixelPlane::Color256 {
            pixels: vec![0; 32 * 24],
            lut: Box::new(ij_core::Lut256::default()),
        },
    )
    .expect("8-bit indexed color image");
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", doc));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedImageType { .. }),
        "8-bit indexed color",
    );
}

#[test]
fn filter_rejects_rgb_and_8bit_non_default_display_range() {
    let bridge = bridge();

    // FileSaver/FileOpener cannot preserve a non-default display range for
    // RGB images (FileSaver.java 775-784), and the 8-bit contract keeps the
    // 0..255 requirement it already had.
    let mut rgb_range = sample_image_rgb();
    rgb_range.display_range = DisplayRange {
        min: 5.0,
        max: 200.0,
    };
    let result = bridge.run_filter(&filter_request("fixtures.RgbInvertFilter", rgb_range));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedMetadata { .. }),
        "RGB display range",
    );

    let mut gray8_range = sample_image();
    gray8_range.display_range = DisplayRange {
        min: 5.0,
        max: 200.0,
    };
    let result = bridge.run_filter(&filter_request("fixtures.RectInvertFilter", gray8_range));
    assert_validation_error(
        result,
        |e| matches!(e, PluginFilterInputError::UnsupportedMetadata { .. }),
        "8-bit display range",
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
