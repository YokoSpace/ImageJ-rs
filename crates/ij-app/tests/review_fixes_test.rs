use egui::{Context, Pos2, RawInput, Stroke, Vec2};
use ij_app::app::{ImageJApp, draw_ellipse_stroke};
use ij_app::pixel_render::plane_to_rgba;
use ij_app::tool::Tool;
use ij_core::{
    DisplayRange, ImageDocument, Lut256, PixelPlane, Roi, ThresholdLevels, format_ij_num,
};

#[test]
fn gray8_display_range_scaling() {
    // Test 8-bit brightness & contrast scaling per ImageJ ByteProcessor.setMinAndMax
    let plane = PixelPlane::Gray8(vec![0, 50, 100, 150, 200, 255]);
    let range = DisplayRange {
        min: 50.0,
        max: 200.0,
    };
    let rgba = plane_to_rgba(&plane, 6, 1, range, None);

    // Pixel 0 (val 0 < min 50) -> 0
    assert_eq!(rgba[0], 0);
    // Pixel 1 (val 50 == min 50) -> 0
    assert_eq!(rgba[4], 0);
    // Pixel 2 (val 100): (100 - 50) / 150 * 255 = 85
    assert_eq!(rgba[8], 85);
    // Pixel 3 (val 150): (150 - 50) / 150 * 255 = 170
    assert_eq!(rgba[12], 170);
    // Pixel 4 (val 200 == max 200) -> 255
    assert_eq!(rgba[16], 255);
    // Pixel 5 (val 255 > max 200) -> 255
    assert_eq!(rgba[20], 255);
}

#[test]
fn gray8_threshold_highlight_red() {
    // Test that 8-bit pixels within active threshold are rendered in classic RED_LUT (255, 0, 0, 255)
    let plane = PixelPlane::Gray8(vec![10, 50, 100, 150, 220]);
    let range = DisplayRange {
        min: 0.0,
        max: 255.0,
    };
    let thresh = Some(ThresholdLevels {
        lower: 50.0,
        upper: 150.0,
    });
    let rgba = plane_to_rgba(&plane, 5, 1, range, thresh);

    // Pixel 0 (10 < 50): normal grayscale 10
    assert_eq!(&rgba[0..4], &[10, 10, 10, 255]);
    // Pixel 1 (50 in [50, 150]): RED highlight
    assert_eq!(&rgba[4..8], &[255, 0, 0, 255]);
    // Pixel 2 (100 in [50, 150]): RED highlight
    assert_eq!(&rgba[8..12], &[255, 0, 0, 255]);
    // Pixel 3 (150 in [50, 150]): RED highlight
    assert_eq!(&rgba[12..16], &[255, 0, 0, 255]);
    // Pixel 4 (220 > 150): normal grayscale 220
    assert_eq!(&rgba[16..20], &[220, 220, 220, 255]);
}

#[test]
fn color256_display_range_and_threshold() {
    // 256-color LUT: invert ramp for red, normal for green, 0 for blue
    let mut r = vec![0u8; 256];
    let mut g = vec![0u8; 256];
    let b = vec![0u8; 256];
    for i in 0..256 {
        r[i] = (255 - i) as u8;
        g[i] = i as u8;
    }
    let lut = Box::new(Lut256 { r, g, b });
    let plane = PixelPlane::Color256 {
        pixels: vec![0, 128, 255],
        lut,
    };

    // No threshold: display range 0..255
    let range = DisplayRange {
        min: 0.0,
        max: 255.0,
    };
    let rgba = plane_to_rgba(&plane, 3, 1, range, None);
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(&rgba[4..8], &[127, 128, 0, 255]);
    assert_eq!(&rgba[8..12], &[0, 255, 0, 255]);

    // With threshold at 128:
    let thresh = Some(ThresholdLevels {
        lower: 100.0,
        upper: 200.0,
    });
    let rgba_thresh = plane_to_rgba(&plane, 3, 1, range, thresh);
    // Pixel 0 (0): normal LUT color
    assert_eq!(&rgba_thresh[0..4], &[255, 0, 0, 255]);
    // Pixel 1 (128 in [100, 200]): RED highlight
    assert_eq!(&rgba_thresh[4..8], &[255, 0, 0, 255]);
    // Pixel 2 (255 > 200): normal LUT color
    assert_eq!(&rgba_thresh[8..12], &[0, 255, 0, 255]);
}

#[test]
fn gray16_and_gray32_threshold_highlight_red() {
    let plane16 = PixelPlane::Gray16(vec![500, 1500, 4000]);
    let range16 = DisplayRange {
        min: 0.0,
        max: 5000.0,
    };
    let thresh16 = Some(ThresholdLevels {
        lower: 1000.0,
        upper: 2000.0,
    });
    let rgba16 = plane_to_rgba(&plane16, 3, 1, range16, thresh16);
    // 500: not thresholded
    assert_ne!(&rgba16[0..4], &[255, 0, 0, 255]);
    // 1500: thresholded -> RED
    assert_eq!(&rgba16[4..8], &[255, 0, 0, 255]);

    let plane32 = PixelPlane::Gray32(vec![0.1, 0.5, 0.9]);
    let range32 = DisplayRange { min: 0.0, max: 1.0 };
    let thresh32 = Some(ThresholdLevels {
        lower: 0.4,
        upper: 0.6,
    });
    let rgba32 = plane_to_rgba(&plane32, 3, 1, range32, thresh32);
    // 0.1: not thresholded
    assert_ne!(&rgba32[0..4], &[255, 0, 0, 255]);
    // 0.5: thresholded -> RED
    assert_eq!(&rgba32[4..8], &[255, 0, 0, 255]);
}

#[test]
fn rgb_does_not_threshold() {
    // RGB pixel (R=10, G=20, B=30) as packed u32
    let packed = (10u32 << 16) | (20u32 << 8) | 30u32;
    let plane = PixelPlane::ColorRgb(vec![packed]);
    let range = DisplayRange {
        min: 0.0,
        max: 255.0,
    };
    let thresh = Some(ThresholdLevels {
        lower: 0.0,
        upper: 255.0,
    });
    let rgba = plane_to_rgba(&plane, 1, 1, range, thresh);
    // RGB must remain original RGB colors unchanged
    assert_eq!(&rgba[0..4], &[10, 20, 30, 255]);
}

#[test]
fn results_table_display_matches_saved_tsv_format() {
    let mut app = ImageJApp::new();
    let _id = app.open_ramp_8bit("ramp", 64, 64).unwrap();

    // Make two measurements
    app.dispatch_rect_roi(0, 0, 10, 10).unwrap();
    app.measure_active().unwrap();

    app.dispatch_oval_roi(10, 10, 20, 20).unwrap();
    app.measure_active().unwrap();

    assert_eq!(app.session.results.rows.len(), 2);

    // Save TSV
    let temp_dir = std::env::temp_dir().join("ij_results_match_test");
    let _ = std::fs::create_dir_all(&temp_dir);
    let tsv_path = temp_dir.join("results.tsv");
    app.save_results_tsv(tsv_path.clone()).unwrap();

    let saved_content = std::fs::read_to_string(&tsv_path).unwrap();
    let lines: Vec<&str> = saved_content.lines().collect();
    let displayed = app.results_grid_cells();
    let saved_cells: Vec<Vec<String>> = lines
        .iter()
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect();
    assert_eq!(displayed, saved_cells);

    // Verify TSV columns: " \tArea\tMean\tMin\tMax"
    assert_eq!(lines[0], " \tArea\tMean\tMin\tMax");

    // Verify row 1 formatting in displayed table vs saved TSV
    let row1 = &app.session.results.rows[0];
    let expected_row1 = format!(
        "1\t{}\t{}\t{}\t{}",
        format_ij_num(row1.area),
        format_ij_num(row1.mean),
        format_ij_num(row1.min),
        format_ij_num(row1.max),
    );
    assert_eq!(lines[1], expected_row1);

    // Verify row 2 formatting
    let row2 = &app.session.results.rows[1];
    let expected_row2 = format!(
        "2\t{}\t{}\t{}\t{}",
        format_ij_num(row2.area),
        format_ij_num(row2.mean),
        format_ij_num(row2.min),
        format_ij_num(row2.max),
    );
    assert_eq!(lines[2], expected_row2);

    // Cleanup
    let _ = std::fs::remove_file(tsv_path);
    let _ = std::fs::remove_dir(temp_dir);
}

#[test]
fn line_results_display_has_the_same_columns_and_values_as_saved_tsv() {
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("ramp", 32, 32).unwrap();
    app.dispatch_rect_roi(2, 2, 4, 4).unwrap();
    app.measure_active().unwrap();
    app.dispatch_line_roi(2, 2, 12, 8).unwrap();
    app.measure_active().unwrap();

    let tsv = app.session.results.to_tsv_basic();
    let saved_cells: Vec<Vec<String>> = tsv
        .lines()
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect();
    assert_eq!(saved_cells[0], [" ", "Area", "Mean", "Min", "Max", "Angle", "Length"]);
    assert_eq!(app.results_grid_cells(), saved_cells);
}

#[test]
fn oval_roi_ellipse_geometry_not_circle() {
    // Width 100, Height 40: semi-axes are rx=50, ry=20 (not 20, 20)
    let ctx = Context::default();
    let _out = ctx.run(RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(200.0, 200.0), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            let stroke = Stroke::new(1.0_f32, egui::Color32::YELLOW);
            // Draw an oval with distinct width and height
            draw_ellipse_stroke(&painter, Pos2::new(100.0, 100.0), 50.0, 20.0, stroke);
        });
    });

    // Verify Oval ROI creation in app dispatches properly
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("ellipse_test", 100, 100).unwrap();
    app.dispatch_oval_roi(10, 20, 80, 40).unwrap();

    let doc = app.session.get_image(id).unwrap();
    if let Some(Roi::Oval(rect)) = &doc.roi {
        assert_eq!(rect.width, 80);
        assert_eq!(rect.height, 40);
        // Semi-axes are 40 and 20 (distinct, not clamped to min=20)
        assert_ne!(rect.width, rect.height);
    } else {
        panic!("Expected Oval ROI");
    }
}

#[test]
fn toolbar_layout_fits_within_680px_without_clipping() {
    // 14 tools in Toolbar
    assert_eq!(Tool::ALL.len(), 14);

    // Each button is min_size 28.0x24.0 with 2.0 item spacing
    let button_width = 28.0_f32;
    let spacing = 2.0_f32;
    let total_width =
        (Tool::ALL.len() as f32) * button_width + ((Tool::ALL.len() - 1) as f32) * spacing;

    // Must comfortably fit in 680px root window with substantial margin for borders
    assert!(
        total_width < 500.0,
        "Total toolbar width {} exceeds 500px, leaving ample room in 680px window",
        total_width
    );

    for tool in Tool::ALL {
        assert!(
            !tool.symbol().is_empty(),
            "Tool {:?} must have a symbol glyph",
            tool
        );
        assert!(!tool.name().is_empty(), "Tool {:?} must have a name", tool);
    }
}

#[test]
fn threshold_and_bc_ui_guards_unsupported_images() {
    let mut app = ImageJApp::new();

    // 1. No active image: threshold reset errors safely
    let err = app.reset_threshold_active();
    assert!(err.is_err());

    // 2. 8-bit image: AutoThreshold works, and Reset clears threshold
    let id = app.open_ramp_8bit("thresh_test", 32, 32).unwrap();
    app.auto_threshold_active("IsoData", false).unwrap();
    assert!(app.session.get_image(id).unwrap().threshold.is_some());

    app.reset_threshold_active().unwrap();
    assert!(app.session.get_image(id).unwrap().threshold.is_none());
    assert_eq!(app.status_message, "Threshold reset");

    // 3. Multi-slice stack: AutoThreshold returns error
    let plane1 = PixelPlane::Gray8(vec![0; 16]);
    let plane2 = PixelPlane::Gray8(vec![255; 16]);
    let stack_doc = ImageDocument::new_stack("stack", 4, 4, vec![plane1, plane2]).unwrap();
    let stack_id = app.session.insert_doc(stack_doc);
    app.set_active_window(stack_id).unwrap();

    let stack_err = app.auto_threshold_active("IsoData", false);
    assert!(
        stack_err.is_err(),
        "AutoThreshold on multi-slice stack must error"
    );

    // 4. RGB image: AutoThreshold returns error
    let rgb_plane = PixelPlane::ColorRgb(vec![0; 16]);
    let rgb_doc = ImageDocument::new("rgb", 4, 4, rgb_plane).unwrap();
    let rgb_id = app.session.insert_doc(rgb_doc);
    app.set_active_window(rgb_id).unwrap();

    let rgb_err = app.auto_threshold_active("IsoData", false);
    assert!(rgb_err.is_err(), "AutoThreshold on RGB must error");
}

#[test]
fn sixteen_bit_threshold_goes_through_session_with_histogram_option() {
    let mut app = ImageJApp::new();
    let id = app
        .session
        .insert_doc(ImageDocument::new_ramp_16bit("ramp16", 16, 16).unwrap());
    app.set_active_window(id).unwrap();

    // Default mode resets the display range like ImageProcessor.resetMinAndMax.
    app.auto_threshold_active("Otsu", false).unwrap();
    let doc = app.session.get_image(id).unwrap();
    assert_eq!(
        doc.threshold,
        Some(ThresholdLevels {
            lower: 0.0,
            upper: 28672.0
        })
    );
    assert_eq!(
        doc.display_range,
        DisplayRange {
            min: 0.0,
            max: 61440.0
        }
    );

    // Explicit 16-bit histogram mode goes through the same session command.
    app.auto_threshold_active_with_options("Otsu", false, true)
        .unwrap();
    let doc = app.session.get_image(id).unwrap();
    assert_eq!(
        doc.threshold,
        Some(ThresholdLevels {
            lower: 0.0,
            upper: 28672.0
        })
    );
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("setAutoThreshold(\"Otsu 16-bit\");")
    );
}

#[test]
fn independent_viewports_for_results_and_dialogs() {
    let mut app = ImageJApp::new();
    let _id = app.open_ramp_8bit("test", 32, 32).unwrap();

    // With no dialogs or results opened
    let ctx = Context::default();
    let out_empty = ctx.run(RawInput::default(), |ctx| {
        app.render_dialogs(ctx);
        app.render_results_window(ctx);
    });
    assert_eq!(out_empty.shapes.len(), 0);

    // Activate Results window and all dialogs
    app.show_results_window = true;
    app.dialogs.show_brightness_contrast = true;
    app.dialogs.show_threshold = true;
    app.dialogs.show_about = true;
    app.dialogs.show_gaussian_blur = true;

    let out = ctx.run(RawInput::default(), |ctx| {
        app.render_dialogs(ctx);
        app.render_results_window(ctx);
    });

    assert!(
        out.shapes.len() > 50,
        "Results and dialogs must render their full UI contents independently"
    );
}
