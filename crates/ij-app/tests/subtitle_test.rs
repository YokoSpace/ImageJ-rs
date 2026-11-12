use ij_app::image_view::ImageView;
use ij_bridge::JavaOracleBridge;
use ij_core::{Calibration, ImageDocument, Lut256, PixelPlane, RectRoi, Roi};
use ij_io::{load_tiff, save_tiff};
use std::path::PathBuf;

fn get_bridge() -> JavaOracleBridge {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf();
    JavaOracleBridge::locate_from_repo_root(repo_root).expect("bridge")
}

#[test]
fn labeled_stack_tiff_subtitle_matches_imagej() {
    let mut doc =
        ImageDocument::new_stack("labels", 2, 2, vec![PixelPlane::Gray8(vec![0; 4]); 3]).unwrap();
    doc.slice_labels = vec![
        Some("first.tif\nmetadata".into()),
        None,
        Some("third sample.tif".into()),
    ];
    doc.roi = Some(Roi::Rect(RectRoi { x: 0, y: 0, width: 1, height: 1 }));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("labels.tif");
    save_tiff(&path, &doc).unwrap();
    let mut loaded = load_tiff(&path).unwrap();
    assert_eq!(loaded.slice_labels, doc.slice_labels);
    assert_eq!(loaded.roi, doc.roi);
    assert_eq!(
        ImageView::create_subtitle(&loaded),
        "1/3 (first); 2x2 pixels; 8-bit; 0K"
    );
    loaded.set_slice(3).unwrap();
    let rust_subtitle = ImageView::create_subtitle(&loaded);
    let oracle = get_bridge()
        .run_macro_on_image(
            &loaded,
            "setSlice(3); print(call(\"tools.oracle.UiProbe.subtitle\"));",
        )
        .unwrap();
    assert_eq!(rust_subtitle, oracle.stdout.trim_end());
}

#[test]
fn single_image_label_matches_imagej() {
    let mut doc = ImageDocument::new_ramp_8bit("single", 2, 2).unwrap();
    doc.slice_labels[0] = Some("sample.tif\nextra".into());
    let rust_subtitle = ImageView::create_subtitle(&doc);
    assert_eq!(rust_subtitle, "\"sample\"; 2x2 pixels; 8-bit; 0K");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("single.tif");
    save_tiff(&path, &doc).unwrap();
    let oracle = get_bridge()
        .eval_macro(&format!(
            "open(\"{}\"); print(call(\"tools.oracle.UiProbe.subtitle\"));",
            path.display()
        ))
        .unwrap();
    assert_eq!(rust_subtitle, oracle.stdout.trim_end());
}

#[test]
fn uncalibrated_four_formats() {
    // 1. Gray8: 16x16 pixels; 8-bit; 0K
    let doc8 = ImageDocument::new_ramp_8bit("ramp8", 16, 16).unwrap();
    assert_eq!(ImageView::create_subtitle(&doc8), "16x16 pixels; 8-bit; 0K");

    // 2. Gray16: 16x16 pixels; 16-bit; 1K
    let doc16 = ImageDocument::new_ramp_16bit("ramp16", 16, 16).unwrap();
    assert_eq!(
        ImageView::create_subtitle(&doc16),
        "16x16 pixels; 16-bit; 1K"
    );

    // 3. Gray32: 16x16 pixels; 32-bit; 1K
    let doc32 = ImageDocument::new_ramp_32bit("ramp32", 16, 16).unwrap();
    assert_eq!(
        ImageView::create_subtitle(&doc32),
        "16x16 pixels; 32-bit; 1K"
    );

    // 4. RGB: 16x16 pixels; RGB; 1K
    let doc_rgb = ImageDocument::new("rgb", 16, 16, PixelPlane::ColorRgb(vec![0; 256])).unwrap();
    assert_eq!(
        ImageView::create_subtitle(&doc_rgb),
        "16x16 pixels; RGB; 1K"
    );
}

#[test]
fn rgb_four_byte_accounting() {
    // In ImageJ (ImagePlus.java line 1232), RGB is counted as 4 bytes per pixel.
    // 100x100 RGB is 100 * 100 * 4 = 40,000 bytes = 39.0625 KB -> 39K.
    // (If 3 bytes per pixel were used, 30,000 bytes would yield 29K).
    let doc_rgb =
        ImageDocument::new("rgb100", 100, 100, PixelPlane::ColorRgb(vec![0; 10000])).unwrap();
    assert_eq!(
        ImageView::create_subtitle(&doc_rgb),
        "100x100 pixels; RGB; 39K"
    );
}

#[test]
fn calibrated_dimensions_and_units() {
    // Spatial calibration in mm: width 100, height 100, pixel size 0.5 mm -> 50x50 mm (100x100); 8-bit; 10K
    let mut doc_mm = ImageDocument::new_ramp_8bit("cal_mm", 100, 100).unwrap();
    doc_mm.calibration = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.5,
        pixel_depth: 1.0,
        unit: "mm".to_string(),
        invert_y: false,
        x_origin: 0.0,
        y_origin: 0.0,
    };
    assert_eq!(
        ImageView::create_subtitle(&doc_mm),
        "50x50 mm (100x100); 8-bit; 10K"
    );

    // Fractional calibration with unit pluralization: "micron" -> "microns"
    // width 100 * 0.125 = 12.5, height 100 * 0.25 = 25.0
    let mut doc_micron = ImageDocument::new_ramp_8bit("cal_micron", 100, 100).unwrap();
    doc_micron.calibration = Calibration {
        pixel_width: 0.125,
        pixel_height: 0.25,
        pixel_depth: 1.0,
        unit: "micron".to_string(),
        invert_y: false,
        x_origin: 0.0,
        y_origin: 0.0,
    };
    assert_eq!(
        ImageView::create_subtitle(&doc_micron),
        "12.5x25.0 microns (100x100); 8-bit; 10K"
    );

    // "inch" -> "inches"
    let mut doc_inch = ImageDocument::new_ramp_8bit("cal_inch", 10, 10).unwrap();
    doc_inch.calibration = Calibration {
        pixel_width: 2.0,
        pixel_height: 2.0,
        pixel_depth: 1.0,
        unit: "inch".to_string(),
        invert_y: false,
        x_origin: 0.0,
        y_origin: 0.0,
    };
    assert_eq!(
        ImageView::create_subtitle(&doc_inch),
        "20x20 inches (10x10); 8-bit; 0K"
    );
}

#[test]
fn three_slice_stack() {
    let plane1 = PixelPlane::Gray8(vec![0; 10000]);
    let plane2 = PixelPlane::Gray8(vec![128; 10000]);
    let plane3 = PixelPlane::Gray8(vec![255; 10000]);
    let mut doc =
        ImageDocument::new_stack("stack3", 100, 100, vec![plane1, plane2, plane3]).unwrap();

    assert_eq!(
        ImageView::create_subtitle(&doc),
        "1/3; 100x100 pixels; 8-bit; 29K"
    );

    doc.set_slice(2).unwrap();
    assert_eq!(
        ImageView::create_subtitle(&doc),
        "2/3; 100x100 pixels; 8-bit; 29K"
    );
}

#[test]
fn size_thresholds_mb_and_gb() {
    // 1MB+ with decimal: 1100x1000 16-bit = 2,200,000 bytes = 2148.4375 KB = 2.098 MB -> "2.1MB"
    let plane = PixelPlane::Gray16(vec![0; 1100 * 1000]);
    let doc_1mb = ImageDocument::new("large_1mb", 1100, 1000, plane).unwrap();
    assert_eq!(
        ImageView::create_subtitle(&doc_1mb),
        "1100x1000 pixels; 16-bit; 2.1MB"
    );

    // Exactly 2MB: 1024x1024 16-bit = 2,097,152 bytes = 2048 KB = 2.0 MB -> stripped .0 -> "2MB"
    let plane_2mb = PixelPlane::Gray16(vec![0; 1024 * 1024]);
    let doc_2mb = ImageDocument::new("mb2", 1024, 1024, plane_2mb).unwrap();
    assert_eq!(
        ImageView::create_subtitle(&doc_2mb),
        "1024x1024 pixels; 16-bit; 2MB"
    );

    // 10MB+ uses integer rounding (size >= 10000K):
    // 3000x2000 16-bit = 12,000,000 bytes = 11718.75 KB = 11.444 MB -> rounded to 11 -> "11MB"
    let plane_10mb = PixelPlane::Gray16(vec![0; 3000 * 2000]);
    let doc_10mb = ImageDocument::new("large_10mb", 3000, 2000, plane_10mb).unwrap();
    assert_eq!(
        ImageView::create_subtitle(&doc_10mb),
        "3000x2000 pixels; 16-bit; 11MB"
    );
}

#[test]
fn inverting_lut_detection() {
    let inverted_lut = Lut256 {
        r: (0..=255u8).rev().collect(),
        g: (0..=255u8).rev().collect(),
        b: (0..=255u8).rev().collect(),
    };
    let plane = PixelPlane::Color256 {
        pixels: vec![0; 10000],
        lut: Box::new(inverted_lut),
    };
    let doc_inv = ImageDocument::new("inv", 100, 100, plane).unwrap();
    assert_eq!(
        ImageView::create_subtitle(&doc_inv),
        "100x100 pixels; 8-bit (inverting LUT); 10K"
    );
}

#[test]
fn oracle_ui_probe_subtitle_parity() {
    let bridge = get_bridge();

    // 1. Gray8 ramp
    let doc8 = ImageDocument::new_ramp_8bit("ramp8", 16, 16).unwrap();
    let rust_sub = ImageView::create_subtitle(&doc8);
    let out = bridge
        .run_macro_on_image(&doc8, "print(call(\"tools.oracle.UiProbe.subtitle\"));")
        .unwrap();
    assert_eq!(rust_sub, out.stdout.trim_end());

    // 2. Gray16 ramp
    let doc16 = ImageDocument::new_ramp_16bit("ramp16", 16, 16).unwrap();
    let rust_sub = ImageView::create_subtitle(&doc16);
    let out = bridge
        .run_macro_on_image(&doc16, "print(call(\"tools.oracle.UiProbe.subtitle\"));")
        .unwrap();
    assert_eq!(rust_sub, out.stdout.trim_end());

    // 3. Gray32 ramp
    let doc32 = ImageDocument::new_ramp_32bit("ramp32", 16, 16).unwrap();
    let rust_sub = ImageView::create_subtitle(&doc32);
    let out = bridge
        .run_macro_on_image(&doc32, "print(call(\"tools.oracle.UiProbe.subtitle\"));")
        .unwrap();
    assert_eq!(rust_sub, out.stdout.trim_end());

    // 4. RGB 100x100
    let doc_rgb =
        ImageDocument::new("rgb100", 100, 100, PixelPlane::ColorRgb(vec![0; 10000])).unwrap();
    let rust_sub = ImageView::create_subtitle(&doc_rgb);
    let out = bridge
        .run_macro_on_image(&doc_rgb, "print(call(\"tools.oracle.UiProbe.subtitle\"));")
        .unwrap();
    assert_eq!(rust_sub, out.stdout.trim_end());

    // 5. Calibrated mm
    let mut doc_mm = ImageDocument::new_ramp_8bit("cal_mm", 100, 100).unwrap();
    doc_mm.calibration = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.5,
        pixel_depth: 1.0,
        unit: "mm".to_string(),
        invert_y: false,
        x_origin: 0.0,
        y_origin: 0.0,
    };
    let rust_sub = ImageView::create_subtitle(&doc_mm);
    let out = bridge
        .run_macro_on_image(&doc_mm, "print(call(\"tools.oracle.UiProbe.subtitle\"));")
        .unwrap();
    assert_eq!(rust_sub, out.stdout.trim_end());

    // 6. 3-slice stack
    let plane1 = PixelPlane::Gray8(vec![0; 10000]);
    let plane2 = PixelPlane::Gray8(vec![128; 10000]);
    let plane3 = PixelPlane::Gray8(vec![255; 10000]);
    let doc_stack =
        ImageDocument::new_stack("stack3", 100, 100, vec![plane1, plane2, plane3]).unwrap();
    let rust_sub = ImageView::create_subtitle(&doc_stack);
    let out = bridge
        .run_macro_on_image(
            &doc_stack,
            "print(call(\"tools.oracle.UiProbe.subtitle\"));",
        )
        .unwrap();
    assert_eq!(rust_sub, out.stdout.trim_end());

    // 7. Inverting LUT
    let inverted_lut = Lut256 {
        r: (0..=255u8).rev().collect(),
        g: (0..=255u8).rev().collect(),
        b: (0..=255u8).rev().collect(),
    };
    let plane_inv = PixelPlane::Color256 {
        pixels: vec![0; 10000],
        lut: Box::new(inverted_lut),
    };
    let doc_inv = ImageDocument::new("inv", 100, 100, plane_inv).unwrap();
    let rust_sub = ImageView::create_subtitle(&doc_inv);
    let out = bridge
        .run_macro_on_image(&doc_inv, "print(call(\"tools.oracle.UiProbe.subtitle\"));")
        .unwrap();
    assert_eq!(rust_sub, out.stdout.trim_end());
}
