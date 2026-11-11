use egui::{Context, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};
use ij_app::ImageJApp;
use ij_core::{ImageDocument, PixelPlane};

#[test]
fn time_only_stack_keeps_play_button_on_visible_flat_slider() {
    let mut doc = make_3slice_doc();
    doc.set_dimensions(1, 1, 3).unwrap();
    let axes = ij_app::image_view::visible_stack_axes(&doc);
    assert_eq!(axes, [ij_app::image_view::StackAxis::FlatSlice]);
    assert_eq!(
        ij_app::app::animation_slider_axis(&axes),
        ij_app::image_view::StackAxis::FlatSlice,
    );
}

fn make_3slice_doc() -> ImageDocument {
    let planes = vec![
        PixelPlane::Gray8(vec![10; 16 * 16]),
        PixelPlane::Gray8(vec![20; 16 * 16]),
        PixelPlane::Gray8(vec![30; 16 * 16]),
    ];
    ImageDocument::new_stack("stack3", 16, 16, planes).unwrap()
}

fn make_2c_3z_4t_doc() -> ImageDocument {
    let planes = (0..24)
        .map(|i| PixelPlane::Gray8(vec![i as u8; 16 * 16]))
        .collect();
    let mut doc = ImageDocument::new_stack("hyperstack_2c3z4t", 16, 16, planes).unwrap();
    doc.set_dimensions(2, 3, 4).unwrap();
    doc
}

#[test]
fn test_ordinary_3slices_timing_cycle() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(make_3slice_doc());
    app.set_active_window(id).unwrap();

    assert!(!app.is_animating(id));
    assert!(app.toggle_animation(id));
    assert!(app.is_animating(id));

    // Initial slice is 1
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 1);

    // time 0.0: initial anchor, does not advance yet
    let count0 = app.advance_animations(0.0);
    assert_eq!(count0, 0);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 1);

    // time 0.15: >= 1.0/7.0 (0.142857s), advances 1 -> 2
    let count1 = app.advance_animations(0.15);
    assert_eq!(count1, 1);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 2);

    // time 0.30: >= 0.15 + 1.0/7.0, advances 2 -> 3
    let count2 = app.advance_animations(0.30);
    assert_eq!(count2, 1);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 3);

    // time 0.45: >= 0.30 + 1.0/7.0, wraps 3 -> 1
    let count3 = app.advance_animations(0.45);
    assert_eq!(count3, 1);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 1);
}

#[test]
fn test_hyperstack_2c3z4t_time_priority_wrap() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(make_2c_3z_4t_doc());
    app.set_active_window(id).unwrap();

    // Start at c=2, z=2, t=1
    app.navigate_position_active(2, 2, 1).unwrap();
    assert_eq!(
        app.session.get_image(id).unwrap().get_position(),
        (2, 2, 1)
    );

    assert!(app.toggle_animation(id));
    assert!(app.is_animating(id));

    // advance_animations(0.0): anchor
    app.advance_animations(0.0);
    assert_eq!(
        app.session.get_image(id).unwrap().get_position(),
        (2, 2, 1)
    );

    // Step 1: t=1 -> t=2 (c=2, z=2 preserved)
    let c1 = app.advance_animations(0.15);
    assert_eq!(c1, 1);
    assert_eq!(
        app.session.get_image(id).unwrap().get_position(),
        (2, 2, 2)
    );

    // Step 2: t=2 -> t=3
    let c2 = app.advance_animations(0.30);
    assert_eq!(c2, 1);
    assert_eq!(
        app.session.get_image(id).unwrap().get_position(),
        (2, 2, 3)
    );

    // Step 3: t=3 -> t=4
    let c3 = app.advance_animations(0.45);
    assert_eq!(c3, 1);
    assert_eq!(
        app.session.get_image(id).unwrap().get_position(),
        (2, 2, 4)
    );

    // Step 4: t=4 -> t=1 (wraps t only, preserving c=2, z=2)
    let c4 = app.advance_animations(0.60);
    assert_eq!(c4, 1);
    assert_eq!(
        app.session.get_image(id).unwrap().get_position(),
        (2, 2, 1)
    );
}

#[test]
fn test_pause_and_resume() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(make_3slice_doc());
    app.set_active_window(id).unwrap();

    // Start
    assert!(app.toggle_animation(id));
    assert!(app.is_animating(id));

    app.advance_animations(0.0);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 1);

    app.advance_animations(0.15);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 2);

    // Pause via toggle
    assert!(!app.toggle_animation(id));
    assert!(!app.is_animating(id));

    // While paused, advance_animations does nothing
    let c = app.advance_animations(0.30);
    assert_eq!(c, 0);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 2);

    // Resume
    assert!(app.toggle_animation(id));
    assert!(app.is_animating(id));

    // Anchor at resume time 0.30
    app.advance_animations(0.30);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 2);

    // Advance to 0.45 -> slice 3
    let c2 = app.advance_animations(0.45);
    assert_eq!(c2, 1);
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 3);
}

#[test]
fn test_manual_navigation_and_close_stops_playback() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(make_3slice_doc());
    app.set_active_window(id).unwrap();

    // 1. Manual slider navigation on flat stack stops playback
    app.toggle_animation(id);
    assert!(app.is_animating(id));
    app.set_slice_active(2).unwrap();
    assert!(
        !app.is_animating(id),
        "set_slice_active must stop animation playback"
    );

    // 2. Manual navigation on hyperstack stops playback
    let id2 = app.session.insert_doc(make_2c_3z_4t_doc());
    app.set_active_window(id2).unwrap();
    app.toggle_animation(id2);
    assert!(app.is_animating(id2));
    app.navigate_position_active(1, 2, 3).unwrap();
    assert!(
        !app.is_animating(id2),
        "navigate_position_active must stop animation playback"
    );

    // 3. Window close stops playback and closing last window does not panic
    app.toggle_animation(id);
    assert!(app.is_animating(id));
    app.toggle_animation(id2);
    assert!(app.is_animating(id2));

    app.close_window(id);
    assert!(!app.is_animating(id));

    app.close_window(id2);
    assert!(!app.is_animating(id2));

    // With all images closed, advance_animations must not panic
    let c = app.advance_animations(1.0);
    assert_eq!(c, 0);
}

#[test]
fn test_animation_preserves_dirty_state_and_macro_recorder() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(make_3slice_doc());
    app.set_active_window(id).unwrap();

    assert!(!app.session.is_dirty(id));
    let initial_macro_len = app.session.macro_recorder.len();

    app.toggle_animation(id);
    for t in &[0.0, 0.15, 0.30, 0.45, 0.60, 0.75] {
        app.advance_animations(*t);
    }

    assert!(
        !app.session.is_dirty(id),
        "Animation playback must not mark document dirty"
    );
    assert_eq!(
        app.session.macro_recorder.len(),
        initial_macro_len,
        "Animation playback must not record macro commands"
    );
}

#[test]
fn test_backslash_shortcut_toggles_animation_and_respects_guards() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(make_3slice_doc());
    app.set_active_window(id).unwrap();

    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(680.0, 200.0));

    // 1. Backslash toggles animation ON
    let mut input1 = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input1.events.push(Event::Key {
        key: Key::Backslash,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input1, |ctx| {
        app.update_ui(ctx);
    });
    assert!(
        app.is_animating(id),
        "Backslash key must toggle animation on"
    );

    // 2. Backslash toggles animation OFF
    let mut input2 = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input2.events.push(Event::Key {
        key: Key::Backslash,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input2, |ctx| {
        app.update_ui(ctx);
    });
    assert!(
        !app.is_animating(id),
        "Second backslash key must toggle animation off"
    );

    // 3. Text edit focus blocks Backslash
    let input3 = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    let _ = ctx.run(input3, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut val = "sample".to_string();
            let resp = ui.text_edit_singleline(&mut val);
            resp.request_focus();
        });
        app.update_ui(ctx);
    });
    assert!(ctx.wants_keyboard_input());

    let mut input4 = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input4.events.push(Event::Key {
        key: Key::Backslash,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input4, |ctx| {
        app.update_ui(ctx);
    });
    assert!(
        !app.is_animating(id),
        "Text edit focus must block backslash shortcut"
    );
}

#[test]
fn test_headless_render_update_ui_animating() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(make_3slice_doc());
    app.set_active_window(id).unwrap();

    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(680.0, 400.0));

    assert!(app.toggle_animation(id));

    // Run frame at time 0.0
    let input = RawInput {
        screen_rect: Some(screen),
        time: Some(0.0),
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 1);
    assert!(
        ctx.has_requested_repaint(),
        "ctx must request repaint while animating"
    );

    // Run frame at time 0.15
    let input2 = RawInput {
        screen_rect: Some(screen),
        time: Some(0.15),
        ..Default::default()
    };
    let _ = ctx.run(input2, |ctx| {
        app.update_ui(ctx);
    });
    assert_eq!(
        app.session.get_image(id).unwrap().current_slice,
        2,
        "update_ui must advance animation when time elapses"
    );
}
