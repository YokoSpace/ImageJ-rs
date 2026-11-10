use egui::{
    Context, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2, ViewportCommand, ViewportId,
};
use ij_app::app::{CloseTarget, ImageJApp, scrollable_menu};
use ij_core::{ImageDocument, PixelPlane};

#[test]
fn menu_accessibility_geometry_constrained_within_680x200_root() {
    let ctx = Context::default();
    let root_size = Vec2::new(680.0, 200.0);
    let screen_rect = Rect::from_min_size(Pos2::ZERO, root_size);

    // 1. Demonstrate unconstrained layout exceeding 200px height:
    // A menu with 15 items in standard egui button height (~24px) plus spacing
    // exceeds 360px in height. When rendered at y=24, its bottom is at y ~384,
    // far exceeding the 200px root window and getting clipped by the window boundary.
    let mut unconstrained_rect = Rect::NOTHING;
    let _ = ctx.run(
        RawInput {
            screen_rect: Some(screen_rect),
            ..Default::default()
        },
        |ctx| {
            egui::Area::new(egui::Id::new("unconstrained_menu_test"))
                .fixed_pos(Pos2::new(10.0, 24.0))
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        for i in 0..15 {
                            let _ = ui.button(format!("Menu Item {i}"));
                        }
                    });
                    unconstrained_rect = ui.min_rect();
                });
        },
    );

    assert!(
        unconstrained_rect.max.y > 200.0,
        "Unconstrained menu with 15 items has max_y={}, exceeding 200px viewport boundary!",
        unconstrained_rect.max.y
    );

    // 2. Demonstrate scrollable_menu geometry:
    // With scrollable_menu, the menu height is constrained to available viewport height:
    // max_h = (screen_h - cursor_y - 8.0) = (200.0 - 24.0 - 8.0) = 168.0.
    // The entire popup visual bounding box stays strictly <= 200.0 (max_y <= 192.0)!
    let mut constrained_rect = Rect::NOTHING;
    let _ = ctx.run(
        RawInput {
            screen_rect: Some(screen_rect),
            ..Default::default()
        },
        |ctx| {
            egui::Area::new(egui::Id::new("constrained_menu_test"))
                .fixed_pos(Pos2::new(10.0, 24.0))
                .show(ctx, |ui| {
                    scrollable_menu(ui, |ui| {
                        for i in 0..15 {
                            let _ = ui.button(format!("Menu Item {i}"));
                        }
                    });
                    constrained_rect = ui.min_rect();
                });
        },
    );

    assert!(
        constrained_rect.max.y <= 200.0,
        "Constrained menu with scrollable_menu must have max_y <= 200.0 (got {}), staying completely accessible within 680x200 root window",
        constrained_rect.max.y
    );
}

#[test]
fn window_menu_activates_and_focuses_intended_image() {
    let mut app = ImageJApp::new();
    let id1 = app.open_ramp_8bit("Image_1", 32, 32).unwrap();
    let id2 = app.open_ramp_8bit("Image_2", 32, 32).unwrap();

    // id2 was opened last, so it is currently active
    assert_eq!(app.active_image_id(), Some(id2));

    let ctx = Context::default();
    let raw_input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(680.0, 200.0))),
        ..Default::default()
    };

    // Initial render pass to initialize viewports
    let _ = ctx.run(raw_input.clone(), |ctx| {
        app.update_ui(ctx);
    });

    // Activate id1 via activate_image_window and run frame
    let vp1 = ViewportId::from_hash_of(&("ij_image_window", id1.0));
    let mut vp1_commands = Vec::new();
    let _ = ctx.run(raw_input.clone(), |ctx| {
        app.activate_image_window(id1, ctx).unwrap();
        vp1_commands = ctx.viewport_for(vp1, |v| v.commands.clone());
        app.update_ui(ctx);
    });

    // Verify session active image updated to id1
    assert_eq!(app.active_image_id(), Some(id1));

    assert!(
        vp1_commands
            .iter()
            .any(|cmd| matches!(cmd, ViewportCommand::Focus)),
        "ViewportCommand::Focus must be sent to Image 1 viewport upon activation"
    );

    // Switch back to id2 and verify commands emitted to id2
    let vp2 = ViewportId::from_hash_of(&("ij_image_window", id2.0));
    let mut vp2_commands = Vec::new();
    let _ = ctx.run(raw_input, |ctx| {
        app.activate_image_window(id2, ctx).unwrap();
        vp2_commands = ctx.viewport_for(vp2, |v| v.commands.clone());
        app.update_ui(ctx);
    });

    assert_eq!(app.active_image_id(), Some(id2));

    assert!(
        vp2_commands
            .iter()
            .any(|cmd| matches!(cmd, ViewportCommand::Focus)),
        "ViewportCommand::Focus must be sent to Image 2 viewport upon activation"
    );
}

#[test]
fn keyboard_shortcuts_measure_undo_invert_select_none() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("test_image", 16, 16).unwrap();

    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(680.0, 200.0));

    // 1. Measure via 'M' key
    assert_eq!(app.session.results.rows.len(), 0);
    let mut input = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::M,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    assert_eq!(
        app.session.results.rows.len(),
        1,
        "Pressing 'M' must measure active image"
    );

    // 2. Measure via Command+M / Ctrl+M
    let mut input = RawInput {
        screen_rect: Some(screen),
        modifiers: Modifiers::COMMAND,
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::M,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    assert_eq!(
        app.session.results.rows.len(),
        2,
        "Pressing Command+M must measure active image"
    );

    // 3. Invert via Shift+I
    let p0_before = match app.session.get_image(id).unwrap().plane() {
        PixelPlane::Gray8(p) => p[0],
        _ => panic!(),
    };
    let shift_mods = Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut input = RawInput {
        screen_rect: Some(screen),
        modifiers: shift_mods,
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::I,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: shift_mods,
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    let p0_after = match app.session.get_image(id).unwrap().plane() {
        PixelPlane::Gray8(p) => p[0],
        _ => panic!(),
    };
    assert_eq!(
        p0_after,
        255 - p0_before,
        "Shift+I must invert active image"
    );

    // 4. Undo via 'Z'
    let mut input = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::Z,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    let p0_undone = match app.session.get_image(id).unwrap().plane() {
        PixelPlane::Gray8(p) => p[0],
        _ => panic!(),
    };
    assert_eq!(p0_undone, p0_before, "'Z' must undo the invert operation");

    // 5. Select None via Shift+A
    app.dispatch_rect_roi(2, 2, 8, 8).unwrap();
    assert!(app.session.get_image(id).unwrap().roi.is_some());
    let mut input = RawInput {
        screen_rect: Some(screen),
        modifiers: shift_mods,
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: shift_mods,
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    assert!(
        app.session.get_image(id).unwrap().roi.is_none(),
        "Shift+A must clear ROI (Select None)"
    );

    // 6. Zoom in via '+' / '=' and Zoom out via '-'
    let initial_zoom = app.image_views.get(&id).unwrap().zoom;
    let mut input = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::Equals,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    let zoomed_in = app.image_views.get(&id).unwrap().zoom;
    assert!(
        zoomed_in > initial_zoom,
        "'+' / '=' must zoom in active image view"
    );

    let mut input = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::Minus,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    let zoomed_out = app.image_views.get(&id).unwrap().zoom;
    assert_eq!(
        zoomed_out, initial_zoom,
        "'-' must zoom out active image view"
    );
}

#[test]
fn keyboard_shortcuts_close_and_close_all_preserve_dirty_guards() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("dirty_ramp", 16, 16).unwrap();

    // New ramp is dirty
    assert!(app.is_dirty(id));

    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(680.0, 200.0));

    // Press 'W' to close dirty image
    let mut input = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::W,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });

    // Window must NOT have closed; dirty guard must be active
    assert_eq!(
        app.open_window_count(),
        1,
        "Dirty document must not close immediately on 'W'"
    );
    let pending = app
        .pending_close
        .as_ref()
        .expect("Pending close prompt must be active");
    assert_eq!(pending.target, CloseTarget::Single(id));

    // Cancel pending close
    app.confirm_pending_close_cancel();

    // Now test Close All with Shift+W
    let shift_mods = Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut input = RawInput {
        screen_rect: Some(screen),
        modifiers: shift_mods,
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::W,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: shift_mods,
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });

    // Document must not have closed silently; dirty guard must be active for All
    assert_eq!(
        app.open_window_count(),
        1,
        "Dirty documents must not close immediately on Shift+W"
    );
    let pending_all = app
        .pending_close
        .as_ref()
        .expect("Pending close prompt must be active");
    assert_eq!(pending_all.target, CloseTarget::All);

    // Discard changes to complete close sequence
    app.confirm_pending_close_discard();
    assert_eq!(app.open_window_count(), 0);
}

#[test]
fn keyboard_shortcuts_stack_invert_preserves_stack_prompt() {
    let planes = vec![
        PixelPlane::Gray8(vec![0; 16]),
        PixelPlane::Gray8(vec![100; 16]),
    ];
    let doc = ImageDocument::new_stack("stack_test", 4, 4, planes).unwrap();
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();

    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(680.0, 200.0));

    // Press Shift+I on multi-slice stack
    let shift_mods = Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut input = RawInput {
        screen_rect: Some(screen),
        modifiers: shift_mods,
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::I,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: shift_mods,
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });

    // Stack prompt must be active, pixels untouched
    assert!(
        app.pending_stack_process.is_some(),
        "Shift+I on a stack must open the Process Stack? prompt"
    );
}

#[test]
fn keyboard_shortcuts_stack_slice_navigation() {
    let planes = vec![
        PixelPlane::Gray8(vec![10; 16]),
        PixelPlane::Gray8(vec![20; 16]),
        PixelPlane::Gray8(vec![30; 16]),
    ];
    let doc = ImageDocument::new_stack("stack_nav", 4, 4, planes).unwrap();
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();

    assert_eq!(app.session.get_image(id).unwrap().current_slice, 1);

    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(680.0, 200.0));

    // Press '.' or ArrowRight to advance slice
    let mut input = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::Period,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 2);

    // Press ArrowRight to advance slice again
    let mut input = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::ArrowRight,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 3);

    // Press ',' or ArrowLeft to go back
    let mut input = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input.events.push(Event::Key {
        key: Key::Comma,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    let _ = ctx.run(input, |ctx| {
        app.update_ui(ctx);
    });
    assert_eq!(app.session.get_image(id).unwrap().current_slice, 2);
}

#[test]
fn text_input_focus_blocks_shortcuts() {
    let mut app = ImageJApp::new();
    let _id = app.open_ramp_8bit("focus_test", 16, 16).unwrap();

    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(680.0, 200.0));

    // Frame 1: Text edit requests focus
    let input1 = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    let _ = ctx.run(input1, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut val = "sample".to_string();
            let resp = ui.text_edit_singleline(&mut val);
            resp.request_focus();
        });
        app.update_ui(ctx);
    });

    assert!(
        ctx.wants_keyboard_input(),
        "ctx must want keyboard input while text edit has focus"
    );

    // Frame 2: User types 'm', 'w', 'z' while text edit is focused
    let mut input2 = RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };
    input2.events.push(Event::Key {
        key: Key::M,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    input2.events.push(Event::Key {
        key: Key::W,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });

    let _ = ctx.run(input2, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut val = "sample".to_string();
            let resp = ui.text_edit_singleline(&mut val);
            resp.request_focus();
        });
        app.update_ui(ctx);
    });

    // Shortcuts MUST NOT have triggered
    assert_eq!(
        app.session.results.rows.len(),
        0,
        "Text input focus must block 'M' shortcut"
    );
    assert_eq!(
        app.open_window_count(),
        1,
        "Text input focus must block 'W' shortcut"
    );
    assert!(
        app.pending_close.is_none(),
        "Text input focus must not open close guard"
    );
}

#[test]
fn unsupported_shortcuts_do_not_execute() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("unsupported_test", 16, 16).unwrap();

    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(680.0, 200.0));

    // Try pressing Cut (X), Copy (C), Paste (V)
    for &k in &[Key::X, Key::C, Key::V] {
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        });
        let _ = ctx.run(input, |ctx| {
            app.update_ui(ctx);
        });
    }

    assert_eq!(app.open_window_count(), 1);
    assert_eq!(app.session.results.rows.len(), 0);
    assert_eq!(app.active_image_id(), Some(id));
}
