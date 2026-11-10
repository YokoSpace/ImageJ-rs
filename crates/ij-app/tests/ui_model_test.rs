use ij_app::app::ImageJApp;
use ij_app::image_view::ImageView;
use ij_app::pixel_render::plane_to_rgba;
use ij_app::tool::Tool;
use ij_core::{DisplayRange, PixelPlane};

#[test]
fn tool_definitions_match_classic_imagej() {
    assert_eq!(Tool::Rectangle.id(), 0);
    assert_eq!(Tool::Oval.id(), 1);
    assert_eq!(Tool::Polygon.id(), 2);
    assert_eq!(Tool::Line.id(), 4);
    assert_eq!(Tool::Magnifier.id(), 11);
    assert_eq!(Tool::Hand.id(), 12);

    assert!(Tool::Rectangle.is_supported());
    assert!(Tool::Oval.is_supported());
    assert!(Tool::Line.is_supported());
    assert!(Tool::Magnifier.is_supported());
    assert!(Tool::Hand.is_supported());

    // Unsupported actions must not misleadingly appear functional
    assert!(!Tool::Polygon.is_supported());
    assert!(!Tool::Wand.is_supported());
    assert!(!Tool::Text.is_supported());
    assert!(!Tool::Angle.is_supported());
}

#[test]
fn pixel_render_gray8_converts_to_rgba() {
    let plane = PixelPlane::Gray8(vec![0, 128, 255, 64]);
    let range = DisplayRange {
        min: 0.0,
        max: 255.0,
    };
    let rgba = plane_to_rgba(&plane, 2, 2, range, None);
    assert_eq!(rgba.len(), 2 * 2 * 4);
    // Pixel 0: 0 -> RGBA (0, 0, 0, 255)
    assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
    // Pixel 1: 128 -> RGBA (128, 128, 128, 255)
    assert_eq!(&rgba[4..8], &[128, 128, 128, 255]);
    // Pixel 2: 255 -> RGBA (255, 255, 255, 255)
    assert_eq!(&rgba[8..12], &[255, 255, 255, 255]);
    // Pixel 3: 64 -> RGBA (64, 64, 64, 255)
    assert_eq!(&rgba[12..16], &[64, 64, 64, 255]);
}

#[test]
fn pixel_render_gray16_scales_with_display_range() {
    let plane = PixelPlane::Gray16(vec![1000, 2000, 3000, 5000]);
    let range = DisplayRange {
        min: 1000.0,
        max: 5000.0,
    };
    let rgba = plane_to_rgba(&plane, 2, 2, range, None);
    // 1000 -> 0
    assert_eq!(rgba[0], 0);
    // 3000 -> 127 or 128
    assert!((rgba[8] as i32 - 127).abs() <= 1);
    // 5000 -> 255
    assert_eq!(rgba[12], 255);
}

#[test]
fn image_view_zoom_steps_and_screen_mapping() {
    let mut view = ImageView::new(100, 100);
    assert_eq!(view.zoom, 1.0);

    // Zoom in steps
    view.zoom_in();
    assert_eq!(view.zoom, 1.5);
    view.zoom_in();
    assert_eq!(view.zoom, 2.0);

    // Zoom out steps
    view.zoom_out();
    assert_eq!(view.zoom, 1.5);
    view.zoom_out();
    assert_eq!(view.zoom, 1.0);
    view.zoom_out();
    assert_eq!(view.zoom, 0.75);

    // Coordinate mapping at 2x zoom:
    view.zoom = 2.0;
    view.pan = (0.0, 0.0);
    // Screen point (10.0, 20.0) -> Image pixel (5, 10)
    let (ix, iy) = view.screen_to_pixel(10.0, 20.0);
    assert_eq!((ix, iy), (5, 10));

    // Screen point outside image
    let (ix, iy) = view.screen_to_pixel(-5.0, 50.0);
    assert_eq!((ix, iy), (-3, 25));
}

#[test]
fn app_multi_window_lifecycle_and_commands() {
    let mut app = ImageJApp::new();
    assert_eq!(app.open_window_count(), 0);

    // Open first image
    let id1 = app.open_ramp_8bit("ramp1", 32, 32).unwrap();
    assert_eq!(app.open_window_count(), 1);
    assert_eq!(app.active_image_id(), Some(id1));

    // Open second image (multi-window independent windows!)
    let id2 = app.open_ramp_8bit("ramp2", 64, 64).unwrap();
    assert_eq!(app.open_window_count(), 2);
    assert_eq!(app.active_image_id(), Some(id2));

    // Switch focus/active window to id1
    app.set_active_window(id1).unwrap();
    assert_eq!(app.active_image_id(), Some(id1));

    // Dispatch ROI on active window
    app.dispatch_rect_roi(4, 4, 16, 16).unwrap();
    let doc1 = app.session.get_image(id1).unwrap();
    assert!(doc1.roi.is_some());

    // Measure active window
    let summary = app.measure_active().unwrap();
    assert_eq!(summary.area, 256.0); // 16x16
    assert_eq!(app.session.results.rows.len(), 1);

    // Invert active window
    app.invert_active().unwrap();

    // Close window 1
    assert!(app.close_window(id1));
    assert_eq!(app.open_window_count(), 1);
    assert_eq!(app.active_image_id(), Some(id2));

    // Close window 2
    assert!(app.close_window(id2));
    assert_eq!(app.open_window_count(), 0);
    assert_eq!(app.active_image_id(), None);
}

#[test]
fn image_window_subtitle_matches_imagej_format() {
    let doc = ij_core::ImageDocument::new_ramp_8bit("blobs.tif", 256, 254).unwrap();
    let subtitle = ImageView::create_subtitle(&doc);
    // Format: "256x254 pixels; 8-bit; 64K"
    assert!(subtitle.starts_with("256x254 pixels; 8-bit; "));
    assert!(subtitle.ends_with("K") || subtitle.ends_with("KB"));
}
