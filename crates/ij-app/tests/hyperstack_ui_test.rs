use ij_app::image_view::{visible_stack_axes, ImageView, StackAxis};
use ij_app::ImageJApp;
use ij_core::{Calibration, ImageDocument, PixelPlane};

fn make_2c_3z_4t_doc() -> ImageDocument {
    // 2 * 3 * 4 = 24 slices, each 16x16 with distinct pixel values per slice
    let planes = (0..24)
        .map(|i| PixelPlane::Gray8(vec![i as u8; 16 * 16]))
        .collect();
    let mut doc = ImageDocument::new_stack("hyperstack_2c3z4t", 16, 16, planes).unwrap();
    doc.set_dimensions(2, 3, 4).unwrap();
    doc
}

#[test]
fn hyperstack_visible_axes_and_subtitle_2c3z4t() {
    let doc = make_2c_3z_4t_doc();
    assert!(doc.is_hyperstack());

    // visible_stack_axes should return Channel, Slice, Frame for axes > 1
    let axes = visible_stack_axes(&doc);
    assert_eq!(
        axes,
        vec![StackAxis::Channel, StackAxis::Slice, StackAxis::Frame],
        "2c*3z*4t must expose Channel, Slice, Frame sliders"
    );

    // Subtitle must match StackWindow.java lines 259-288: "c:1/2 z:1/3 t:1/4; ..."
    let subtitle = ImageView::create_subtitle(&doc);
    assert_eq!(
        subtitle, "c:1/2 z:1/3 t:1/4; 16x16 pixels; 8-bit; 6K",
        "Initial 2c*3z*4t subtitle must match StackWindow format"
    );
}

#[test]
fn hyperstack_calibrated_subtitle() {
    let mut doc = make_2c_3z_4t_doc();
    doc.calibration = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.5,
        pixel_depth: 2.0,
        unit: "micron".into(),
        invert_y: false,
        x_origin: 0.0,
        y_origin: 0.0,
    };

    // Subtitle must format calibrated dimensions and units like StackWindow.java:259-288
    // Tools.getDecimalPlaces(8.0, 8.0) == 0 -> "8x8 microns" (tested against oracle UiProbe)
    let subtitle = ImageView::create_subtitle(&doc);
    assert_eq!(
        subtitle, "c:1/2 z:1/3 t:1/4; 8x8 microns (16x16); 8-bit; 6K",
        "Calibrated hyperstack subtitle must preserve C/Z/T prefix and calibrated physical dimensions"
    );
}

#[test]
fn hyperstack_navigation_flat_slice_and_no_macro_record() {
    let doc = make_2c_3z_4t_doc();
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();

    // Verify initial position (c=1, z=1, t=1) -> flat slice 1
    let active_doc = app.session.active_doc().unwrap();
    assert_eq!(active_doc.get_position(), (1, 1, 1));
    assert_eq!(active_doc.current_slice, 1);

    // Navigate time axis to 3: (c=1, z=1, t=3) -> flat slice 13
    // ImagePlus.getStackIndex(1, 1, 3): (3-1)*2*3 + (1-1)*2 + 1 = 12 + 0 + 1 = 13
    app.navigate_position_active(1, 1, 3).unwrap();

    let active_doc = app.session.active_doc().unwrap();
    assert_eq!(active_doc.get_position(), (1, 1, 3));
    assert_eq!(
        active_doc.current_slice, 13,
        "Time navigation to t=3 must move flat slice to 13"
    );

    // Navigation must NOT be recorded in macro recorder (StackWindow / NavigateSlice behavior)
    assert!(
        app.session.macro_recorder.is_empty(),
        "NavigatePosition must not be recorded in macro transcript"
    );

    // Subtitle must reflect new position
    let subtitle = ImageView::create_subtitle(active_doc);
    assert_eq!(
        subtitle, "c:1/2 z:1/3 t:3/4; 16x16 pixels; 8-bit; 6K",
        "Subtitle must update to c:1/2 z:1/3 t:3/4"
    );

    // Retaining other axes: pass channel=0 or slice=0 to retain current values (ImagePlus.java 1850-1875)
    app.navigate_position_active(0, 2, 0).unwrap();
    let active_doc = app.session.active_doc().unwrap();
    assert_eq!(
        active_doc.get_position(),
        (1, 2, 3),
        "Retaining other axes when navigating slice to 2"
    );
    // (t=3 - 1)*6 + (z=2 - 1)*2 + 1 = 12 + 2 + 1 = 15
    assert_eq!(active_doc.current_slice, 15);
}

#[test]
fn ordinary_stack_fallback_axes_and_subtitle() {
    let planes = (0..5)
        .map(|_| PixelPlane::Gray8(vec![0; 16 * 16]))
        .collect();
    let doc = ImageDocument::new_stack("ordinary_stack", 16, 16, planes).unwrap();
    assert!(!doc.is_hyperstack());

    // Fallback: ordinary 1D stack must return single FlatSlice axis
    let axes = visible_stack_axes(&doc);
    assert_eq!(
        axes,
        vec![StackAxis::FlatSlice],
        "Ordinary 1D stack must produce single FlatSlice axis"
    );

    // Ordinary subtitle: "1/5; ..."
    let subtitle = ImageView::create_subtitle(&doc);
    assert_eq!(
        subtitle, "1/5; 16x16 pixels; 8-bit; 1K",
        "Ordinary stack subtitle must start with flat slice 1/5"
    );
}

#[test]
fn hyperstack_texture_and_cursor_status_refresh() {
    let mut app = ImageJApp::new();
    let doc = make_2c_3z_4t_doc();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();

    // Hover at (5, 5) on slice 1 (value is 0, z is 0)
    app.update_status_for_pixel(id, 5, 5);
    assert_eq!(app.status_message, " x=5, y=5, z=0, value=0");

    let initial_rev = app.session.image_revision(id);

    // Navigate to (c=2, z=3, t=4):
    // flat slice = (4-1)*6 + (3-1)*2 + 2 = 18 + 4 + 2 = 24
    // pixel value at slice 24 is 23
    // z coordinate (0-based) is 2 (slice 3)
    app.navigate_position_active(2, 3, 4).unwrap();

    // Revision must be bumped for texture cache reload
    assert!(
        app.session.image_revision(id) > initial_rev,
        "Navigation must bump image revision for texture cache"
    );

    // Status message must be refreshed with new z and pixel value
    assert_eq!(
        app.status_message, " x=5, y=5, z=2, value=23",
        "Cursor status must refresh with z=2 and value=23 on slice 24"
    );
}
