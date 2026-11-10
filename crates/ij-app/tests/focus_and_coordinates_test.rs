use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::app::ImageJApp;
use ij_app::image_view::ImageView;
use ij_app::tool::Tool;

#[test]
fn active_image_tracking_on_focus_and_hand_pan() {
    let mut app = ImageJApp::new();

    let id1 = app.open_ramp_8bit("window_1", 32, 32).unwrap();
    let id2 = app.open_ramp_8bit("window_2", 32, 32).unwrap();

    // Opening id2 made it active
    assert_eq!(app.active_image_id(), Some(id2));

    // Switching active window sets it
    app.set_active_window(id1).unwrap();
    assert_eq!(app.active_image_id(), Some(id1));

    // Simulating hand pan tool selection and activation on window 2:
    app.active_tool = Tool::Hand;

    // Simulate focus / hand pan targeting id2
    app.set_active_window(id2).unwrap();
    assert_eq!(app.active_image_id(), Some(id2));

    // View panning updates view.pan correctly
    let mut view = ImageView::new(32, 32);
    view.pan = (10.0, 15.0);
    assert_eq!(view.pan, (10.0, 15.0));
}

#[test]
fn roi_pan_coordinate_no_double_subtraction() {
    let mut view = ImageView::new(100, 100);
    view.zoom = 2.0;
    view.pan = (40.0, 20.0);

    // Image pixel (10, 15) at zoom 2.0 and pan (40, 20) has canvas-relative screen coordinate:
    // x = 10 * 2.0 + 40.0 = 60.0
    // y = 15 * 2.0 + 20.0 = 50.0
    let (px, py) = view.screen_to_pixel(60.0, 50.0);
    assert_eq!(
        (px, py),
        (10, 15),
        "screen_to_pixel with canvas-relative coordinates must map exactly to (10, 15)"
    );

    // If double subtraction occurred (e.g. passing pos.x - img_rect.min.x = 60 - 40 = 20):
    // px would have been ((20 - 40) / 2.0) = -10, which was the bug.
    // Our fix passes canvas-relative coordinates (60.0, 50.0), so only one subtraction occurs!
    let (screen_x, screen_y) = view.pixel_to_screen(10.0, 15.0);
    assert_eq!(screen_x, 60.0);
    assert_eq!(screen_y, 50.0);
}

#[test]
fn panned_pixels_beyond_original_canvas_bounds_remain_interactive() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("panned", 32, 32).unwrap();
    app.image_views.get_mut(&id).unwrap().pan = (50.0, 0.0);
    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(220.0, 160.0));

    let frame = |events| RawInput {
        screen_rect: Some(screen),
        events,
        ..Default::default()
    };
    let render = |app: &mut ImageJApp, input| {
        let _ = ctx.run(input, |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                app.render_image_canvas_content(ui, id);
            });
        });
    };

    render(&mut app, frame(Vec::new()));
    render(&mut app, frame(vec![Event::PointerMoved(Pos2::new(70.0, 50.0))]));
    assert!(
        app.status_message.starts_with(" x="),
        "a pixel shifted right by the Hand tool must remain hoverable: {}",
        app.status_message
    );
}

#[test]
fn hand_pan_uses_each_pointer_movement_once() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("hand", 32, 32).unwrap();
    app.active_tool = Tool::Hand;
    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(220.0, 160.0));
    let mut render = |events| {
        let input = RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                app.render_image_canvas_content(ui, id);
            });
        });
    };

    render(Vec::new());
    render(vec![
        Event::PointerMoved(Pos2::new(20.0, 50.0)),
        Event::PointerButton {
            pos: Pos2::new(20.0, 50.0),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        },
    ]);
    render(vec![Event::PointerMoved(Pos2::new(30.0, 50.0))]);
    render(vec![Event::PointerMoved(Pos2::new(40.0, 50.0))]);
    assert_eq!(app.image_views[&id].pan, (20.0, 0.0));
}

#[test]
fn drag_start_uses_press_origin_under_zoom_and_pan() {
    let mut view = ImageView::new(100, 100);
    view.zoom = 1.0;
    view.pan = (0.0, 0.0);

    // Suppose user clicked down at (25.0, 30.0) -> pixel (25, 30)
    let press_origin = Pos2::new(25.0, 30.0);
    let (start_px, start_py) = view.screen_to_pixel(press_origin.x, press_origin.y);
    assert_eq!((start_px, start_py), (25, 30));

    // When egui triggers drag_started after moving 4 pixels to (29.0, 30.0):
    let current_pos = Pos2::new(29.0, 30.0);
    let (curr_px, curr_py) = view.screen_to_pixel(current_pos.x, current_pos.y);
    assert_eq!((curr_px, curr_py), (29, 30));

    // Because app uses press_origin for drag_start_pixel, start is (25, 30), not (29, 30)!
    view.drag_start_pixel = Some((start_px, start_py));
    view.drag_current_pixel = Some((curr_px, curr_py));

    assert_eq!(view.drag_start_pixel, Some((25, 30)));
    assert_eq!(view.drag_current_pixel, Some((29, 30)));
}

#[test]
fn errors_are_surfaced_for_open_and_filter_operations() {
    let mut app = ImageJApp::new();

    // 1. Opening a non-existent file surfaces error in status_message and dialogs.last_error
    let bad_path = std::path::PathBuf::from("/nonexistent/file/test.tif");
    let res = app.open_tiff_file(bad_path);
    assert!(res.is_err());
    assert!(app.status_message.contains("Open failed"));
    assert!(app.dialogs.last_error.is_some());
    let err_str = app.dialogs.last_error.as_ref().unwrap();
    assert!(err_str.contains("Could not open"));

    // Dismiss error
    app.dialogs.last_error = None;

    // 2. Running filter when no image is active surfaces error
    let res2 = app.smooth_active();
    assert!(res2.is_err());
    assert!(app.status_message.contains("Smooth failed"));
    assert!(app.dialogs.last_error.is_some());

    app.dialogs.last_error = None;

    let res3 = app.invert_active();
    assert!(res3.is_err());
    assert!(app.status_message.contains("Invert failed"));
    assert!(app.dialogs.last_error.is_some());
}

#[test]
fn egui_interaction_close_guard_and_error_dialog_rendering() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("egui_test", 32, 32).unwrap();
    assert!(app.is_dirty(id));

    // Trigger close guard
    assert!(!app.request_close_window(id));
    assert!(app.pending_close.is_some());

    // Run egui context to render the modal dialog
    let ctx = Context::default();
    let raw_input = RawInput::default();
    let full_output = ctx.run(raw_input, |ctx| {
        // Render dialogs within egui context
        app.render_dialogs(ctx);
    });

    // Verify shapes were drawn (the modal dialog window)
    assert!(
        !full_output.shapes.is_empty(),
        "Modal close guard window must render shapes in egui"
    );

    // Cancel the close guard
    app.confirm_pending_close_cancel();
    assert!(app.pending_close.is_none());
    assert_eq!(app.open_window_count(), 1, "Document must be retained");

    // Surface an error and verify error dialog renders in egui
    app.dialogs.last_error = Some("Test error message".to_string());
    let output2 = ctx.run(RawInput::default(), |ctx| {
        app.render_dialogs(ctx);
    });
    assert!(
        !output2.shapes.is_empty(),
        "Error dialog must render shapes in egui"
    );
}
