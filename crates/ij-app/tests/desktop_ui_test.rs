use ij_app::app::ImageJApp;
use ij_app::tool::Tool;
use ij_core::{ImageDocument, PixelPlane};
use ij_io::save_tiff;
use std::fs;

#[test]
fn open_tiff_shows_pixels_and_inspects_values() {
    let mut app = ImageJApp::new();

    // Create a known test TIFF on disk
    let temp_dir = std::env::temp_dir().join("ij_app_test_tiff");
    let _ = fs::create_dir_all(&temp_dir);
    let tiff_path = temp_dir.join("test_gradient.tif");

    let doc = ImageDocument::new_ramp_8bit("gradient", 100, 50).unwrap();
    save_tiff(&tiff_path, &doc).unwrap();

    // Open via ImageJApp
    let img_id = app.open_tiff_file(tiff_path.clone()).unwrap();
    assert_eq!(app.open_window_count(), 1);
    assert_eq!(app.active_image_id(), Some(img_id));

    let opened_doc = app.session.get_image(img_id).unwrap();
    assert_eq!(opened_doc.width, 100);
    assert_eq!(opened_doc.height, 50);

    // Test pixel inspection under cursor
    app.update_status_for_pixel(img_id, 0, 0);
    assert!(app.status_message.contains("x=0, y=0, value=0"));

    app.update_status_for_pixel(img_id, 99, 25);
    assert!(app.status_message.contains("x=99, y=25"));

    // Cleanup
    let _ = fs::remove_file(tiff_path);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn multi_window_independence_and_targeted_operations() {
    let mut app = ImageJApp::new();

    // Open two independent windows
    let id1 = app.open_ramp_8bit("window_1", 20, 20).unwrap();
    let id2 = app.open_ramp_8bit("window_2", 40, 40).unwrap();

    assert_eq!(app.open_window_count(), 2);
    assert_eq!(app.active_image_id(), Some(id2));

    // Target window 1 specifically
    app.set_active_window(id1).unwrap();
    assert_eq!(app.active_image_id(), Some(id1));

    // Invert window 1
    app.invert_active().unwrap();

    // Verify window 1 was inverted, but window 2 remained original
    let doc1 = app.session.get_image(id1).unwrap();
    let doc2 = app.session.get_image(id2).unwrap();

    if let (PixelPlane::Gray8(p1), PixelPlane::Gray8(p2)) = (doc1.plane(), doc2.plane()) {
        // In ramp: pixel at (0, 0) was 0; after invert it is 255
        assert_eq!(p1[0], 255);
        // window 2 at (0, 0) was 0; untouched
        assert_eq!(p2[0], 0);
    } else {
        panic!("expected Gray8 planes");
    }

    // Zoom window 1 to 200%, window 2 to 50%
    app.image_views.get_mut(&id1).unwrap().set_zoom(2.0);
    app.image_views.get_mut(&id2).unwrap().set_zoom(0.5);

    assert_eq!(app.image_views.get(&id1).unwrap().zoom, 2.0);
    assert_eq!(app.image_views.get(&id2).unwrap().zoom, 0.5);

    // Close window 1: window 2 becomes active
    assert!(app.close_window(id1));
    assert_eq!(app.open_window_count(), 1);
    assert_eq!(app.active_image_id(), Some(id2));
}

#[test]
fn wired_session_commands_execute_and_record_macros() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("sample", 30, 30).unwrap();

    // Test Smooth, Sharpen, Find Edges
    app.smooth_active().unwrap();
    app.sharpen_active().unwrap();
    app.find_edges_active().unwrap();

    // Test Filters: Gaussian Blur, Median, Min, Max, Despeckle
    app.gaussian_blur_active(1.5).unwrap();
    app.median_active(1.0).unwrap();
    app.minimum_active(1.0).unwrap();
    app.maximum_active(1.0).unwrap();
    app.despeckle_active().unwrap();

    // Test ROI + Measure
    app.dispatch_rect_roi(5, 5, 10, 10).unwrap();
    let summary = app.measure_active().unwrap();
    assert_eq!(summary.area, 100.0);
    assert_eq!(app.session.results.rows.len(), 1);

    // Test Selection Clear
    app.clear_roi().unwrap();
    assert!(app.session.get_image(id).unwrap().roi.is_none());

    // Test Display Range
    app.set_display_range_active(10.0, 200.0).unwrap();
    assert_eq!(app.session.get_image(id).unwrap().display_range.min, 10.0);
    assert_eq!(app.session.get_image(id).unwrap().display_range.max, 200.0);

    // Verify macro recorder has recorded commands faithfully
    assert!(
        app.session
            .macro_recorder
            .iter()
            .any(|m| m.contains("makeRectangle"))
    );
    assert!(
        app.session
            .macro_recorder
            .iter()
            .any(|m| m.contains("run(\"Measure\");"))
    );
    assert!(
        app.session
            .macro_recorder
            .iter()
            .any(|m| m.contains("run(\"Smooth\");"))
    );
}

#[test]
fn unsupported_actions_guardrail() {
    // Verify that unsupported tools are correctly flagged so UI can disable them
    let unsupported_tools = [
        Tool::Polygon,
        Tool::Freehand,
        Tool::Polyline,
        Tool::Freeline,
        Tool::Point,
        Tool::Wand,
        Tool::Text,
        Tool::Dropper,
        Tool::Angle,
    ];

    for tool in unsupported_tools {
        assert!(
            !tool.is_supported(),
            "Tool {:?} must be marked unsupported so UI does not misleadingly enable it",
            tool
        );
    }
}
