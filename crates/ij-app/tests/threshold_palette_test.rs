use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::action::AppAction;
use ij_app::ImageJApp;
use ij_bridge::JavaOracleBridge;
use ij_core::{ImageDocument, PixelPlane};
use ij_session::Command;

fn render_palette(ctx: &Context, app: &mut ImageJApp, events: Vec<Event>) {
    let mut input = RawInput::default();
    input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 420.0)));
    input.events = events;
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_threshold_palette_content(ui));
    });
}

fn click_button(ctx: &Context, app: &mut ImageJApp, label: &str) {
    render_palette(ctx, app, vec![]);
    let pos = ImageJApp::threshold_button_rect(ctx, label)
        .unwrap()
        .center();
    render_palette(
        ctx,
        app,
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    render_palette(
        ctx,
        app,
        vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
}

#[test]
fn lower_slider_previews_threshold_on_each_drag_frame() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("slider", 32, 32).unwrap();
    app.session.set_dirty(id, false);
    render_palette(&ctx, &mut app, vec![]);
    let rect = ImageJApp::threshold_lower_slider_rect(&ctx).unwrap();
    let start = Pos2::new(rect.left() + 45.0, rect.center().y);
    let first = Pos2::new(start.x + 30.0, start.y);
    let second = Pos2::new(start.x + 60.0, start.y);
    let records = app.session.macro_recorder.len();
    render_palette(
        &ctx,
        &mut app,
        vec![
            Event::PointerMoved(start),
            Event::PointerButton {
                pos: start,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    render_palette(&ctx, &mut app, vec![Event::PointerMoved(first)]);
    let lower_one = app.session.get_image(id).unwrap().threshold.unwrap().lower;
    render_palette(&ctx, &mut app, vec![Event::PointerMoved(second)]);
    let lower_two = app.session.get_image(id).unwrap().threshold.unwrap().lower;
    assert_ne!(lower_one, lower_two);
    assert_eq!(app.session.macro_recorder.len(), records);
    assert!(!app.session.is_dirty(id));
}

#[test]
fn auto_otsu_button_matches_java_oracle() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("otsu", 32, 32).unwrap();
    let doc = app.session.get_image(id).unwrap().clone();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let oracle = JavaOracleBridge::locate_from_repo_root(repo).unwrap()
        .run_macro_on_image(&doc, "setAutoThreshold(\"Otsu\"); getThreshold(l,u); print(\"THRESH \"+d2s(l,6)+\" \"+d2s(u,6));")
        .unwrap();
    let line = oracle
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("THRESH "))
        .unwrap();
    let mut parts = line
        .split_whitespace()
        .skip(1)
        .map(|n| n.parse::<f64>().unwrap());
    let expected = (parts.next().unwrap(), parts.next().unwrap());
    app.dialogs.threshold_method = "Otsu".into();
    click_button(&ctx, &mut app, "Auto");
    let levels = app.session.get_image(id).unwrap().threshold.unwrap();
    assert_eq!((levels.lower, levels.upper), expected);
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "setAutoThreshold(\"Otsu\");"
    );
}

#[test]
fn apply_button_converts_thresholded_image_to_mask() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("opening", 32, 32).unwrap();
    render_palette(&ctx, &mut app, vec![]);
    assert!(app.session.get_image(id).unwrap().threshold.is_some());
    let doc = app.session.get_image(id).unwrap().clone();
    // The palette's auto-set levels are not saved in TIFFs, so the oracle
    // needs the matching setThreshold prelude.
    let levels = doc.threshold.unwrap();
    let oracle = JavaOracleBridge::locate_from_repo_root(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap(),
    )
    .unwrap()
    .run_macro_on_image(
        &doc,
        &format!(
            "setThreshold({}, {}); run(\"Convert to Mask\");",
            ij_core::format_ij_num(levels.lower),
            ij_core::format_ij_num(levels.upper)
        ),
    )
    .unwrap()
    .output_image
    .unwrap();
    click_button(&ctx, &mut app, "Apply");
    let applied = app.session.get_image(id).unwrap();
    assert_eq!(applied.slices, oracle.slices);
    assert_ne!(applied.plane(), doc.plane());
    assert!(app.session.is_dirty(id));
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "run(\"Convert to Mask\");"
    );
}

#[test]
fn apply_button_without_threshold_reports_threshold_is_not_set() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("no-threshold", 32, 32).unwrap();
    render_palette(&ctx, &mut app, vec![]);
    click_button(&ctx, &mut app, "Reset");
    assert!(app.session.get_image(id).unwrap().threshold.is_none());
    let pixels = app.session.get_image(id).unwrap().plane().clone();
    click_button(&ctx, &mut app, "Apply");
    // ThresholdAdjuster.apply rejects an image without threshold
    // (ThresholdAdjuster.java 776-781).
    assert_eq!(app.dialogs.last_error.as_deref(), Some("Threshold is not set"));
    assert_eq!(app.session.get_image(id).unwrap().plane(), &pixels);
}

#[test]
fn set_button_records_raw_threshold_once() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("set", 32, 32).unwrap();
    render_palette(&ctx, &mut app, vec![]);
    app.session
        .dispatch(Command::PreviewThreshold {
            lower: 40.0,
            upper: 180.0,
        })
        .unwrap();
    render_palette(&ctx, &mut app, vec![]);
    let records = app.session.macro_recorder.len();
    click_button(&ctx, &mut app, "Set");
    assert_eq!(
        app.session.get_image(id).unwrap().threshold.unwrap().lower,
        40.0
    );
    assert_eq!(app.session.macro_recorder.len(), records + 1);
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "setThreshold(40, 180, \"raw\");"
    );
}

#[test]
fn preview_threshold_updates_red_overlay_without_macro_or_dirty_state() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("threshold-preview", 32, 32).unwrap();
    app.session.set_dirty(id, false);
    let _ = ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, id));
    });
    let uploads_before = app.texture_upload_count();
    let records_before = app.session.macro_recorder.len();
    let revision_before = app.session.image_revision(id);

    for (lower, upper) in [(40.0, 180.0), (80.0, 140.0)] {
        app.session
            .dispatch(Command::PreviewThreshold { lower, upper })
            .unwrap();
        let levels = app.session.get_image(id).unwrap().threshold.unwrap();
        assert_eq!((levels.lower, levels.upper), (lower, upper));
        assert!(app.session.image_revision(id) > revision_before);
        assert_eq!(app.session.macro_recorder.len(), records_before);
        assert!(!app.session.is_dirty(id));
        let _ = ctx.run(RawInput::default(), |ctx| {
            CentralPanel::default().show(ctx, |ui| app.render_image_canvas_content(ui, id));
        });
    }
    assert_eq!(app.texture_upload_count(), uploads_before + 2);
}

fn open_document(app: &mut ImageJApp, doc: ImageDocument) -> ij_session::ImageId {
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();
    id
}

fn drag_lower_slider(ctx: &Context, app: &mut ImageJApp) {
    render_palette(ctx, app, vec![]);
    let rect = ImageJApp::threshold_lower_slider_rect(ctx).unwrap();
    let start = Pos2::new(rect.left() + 45.0, rect.center().y);
    let end = Pos2::new(start.x + 30.0, start.y);
    render_palette(
        ctx,
        app,
        vec![
            Event::PointerMoved(start),
            Event::PointerButton {
                pos: start,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    render_palette(ctx, app, vec![Event::PointerMoved(end)]);
    render_palette(
        ctx,
        app,
        vec![Event::PointerButton {
            pos: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
}

#[test]
fn threshold_action_accepts_grayscale_images_and_stacks() {
    for plane in [
        PixelPlane::Gray8(vec![0; 4]),
        PixelPlane::Gray16(vec![0; 4]),
        PixelPlane::Gray32(vec![0.0; 4]),
    ] {
        for slice_count in [1, 2] {
            let mut app = ImageJApp::new();
            let doc = ImageDocument::new_stack("grayscale", 2, 2, vec![plane.clone(); slice_count])
                .unwrap();
            open_document(&mut app, doc);
            assert!(
                app.action_enabled(AppAction::Threshold).is_ok(),
                "threshold must be available for {plane:?}, {slice_count} slices"
            );
            app.perform(AppAction::Threshold, &Context::default());
            assert!(app.dialogs.show_threshold);
        }
    }

    let mut app = ImageJApp::new();
    open_document(
        &mut app,
        ImageDocument::new("RGB", 2, 2, PixelPlane::ColorRgb(vec![0; 4])).unwrap(),
    );
    assert!(app.action_enabled(AppAction::Threshold).is_err());
}

#[test]
fn gray32_palette_allows_manual_threshold_and_applies_java_mask() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let doc = ImageDocument::new_ramp_32bit("float", 16, 16).unwrap();
    let id = open_document(&mut app, doc);
    app.perform(AppAction::Threshold, &ctx);
    assert!(app.dialogs.show_threshold);

    render_palette(&ctx, &mut app, vec![]);
    click_button(&ctx, &mut app, "Auto");
    assert!(app.session.get_image(id).unwrap().threshold.is_none());
    assert!(app.dialogs.last_error.is_none());
    drag_lower_slider(&ctx, &mut app);
    let levels = app.session.get_image(id).unwrap().threshold.unwrap();
    click_button(&ctx, &mut app, "Set");
    assert_eq!(app.session.get_image(id).unwrap().threshold, Some(levels));

    let doc = app.session.get_image(id).unwrap().clone();
    let oracle = JavaOracleBridge::locate_from_repo_root(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap(),
    )
    .unwrap()
    .run_macro_on_image(
        &doc,
        &format!(
            "setThreshold({}, {}); run(\"Convert to Mask\");",
            ij_core::format_ij_num(levels.lower),
            ij_core::format_ij_num(levels.upper)
        ),
    )
    .unwrap()
    .output_image
    .unwrap();
    click_button(&ctx, &mut app, "Apply");
    assert_eq!(app.session.get_image(id).unwrap().slices, oracle.slices);
    assert!(app.session.is_dirty(id));
}

#[test]
fn set_without_existing_threshold_uses_display_range() {
    let float = ImageDocument::new_ramp_32bit("float", 16, 16).unwrap();
    let stack = ImageDocument::new_stack(
        "stack",
        2,
        2,
        vec![PixelPlane::Gray16(vec![0, 10_000, 40_000, 60_000]); 2],
    )
    .unwrap();
    for doc in [float, stack] {
        // ThresholdAdjuster.doSet starts with the display range when no
        // threshold is set (ThresholdAdjuster.java 749-755, 583-590).
        let expected = doc.display_range;
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        let id = open_document(&mut app, doc);
        click_button(&ctx, &mut app, "Set");
        let levels = app.session.get_image(id).unwrap().threshold.unwrap();
        assert_eq!((levels.lower, levels.upper), (expected.min, expected.max));
    }
}

#[test]
fn stack_palette_manual_threshold_opens_binary_dialog_and_applies_java_mask() {
    for plane in [
        PixelPlane::Gray8((0..64).map(|i| i * 4).collect()),
        PixelPlane::Gray16((0..64).map(|i| i * 1000).collect()),
        PixelPlane::Gray32((0..64).map(|i| i as f32 / 63.0).collect()),
    ] {
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        let mut reversed = plane.clone();
        match &mut reversed {
            PixelPlane::Gray8(pixels) => pixels.reverse(),
            PixelPlane::Gray16(pixels) => pixels.reverse(),
            PixelPlane::Gray32(pixels) => pixels.reverse(),
            _ => unreachable!(),
        }
        let mut doc = ImageDocument::new_stack("stack", 8, 8, vec![plane, reversed]).unwrap();
        doc.set_slice(2).unwrap();
        let id = open_document(&mut app, doc);
        app.dialogs.threshold_method = "Otsu".into();
        app.dialogs.threshold_dark_bg = true;
        app.perform(AppAction::Threshold, &ctx);
        assert!(app.dialogs.show_threshold);

        render_palette(&ctx, &mut app, vec![]);
        click_button(&ctx, &mut app, "Auto");
        assert!(app.session.get_image(id).unwrap().threshold.is_none());
        assert!(app.dialogs.last_error.is_none());
        drag_lower_slider(&ctx, &mut app);
        let levels = app.session.get_image(id).unwrap().threshold.unwrap();
        click_button(&ctx, &mut app, "Set");
        assert_eq!(app.session.get_image(id).unwrap().threshold, Some(levels));

        let doc = app.session.get_image(id).unwrap().clone();
        let oracle = JavaOracleBridge::locate_from_repo_root(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap(),
        )
        .unwrap()
        .run_macro_on_image(
            &doc,
            &format!(
                "setSlice(2); setThreshold({}, {}); run(\"Convert to Mask\", \"method=Otsu background=Dark black\");",
                ij_core::format_ij_num(levels.lower),
                ij_core::format_ij_num(levels.upper)
            ),
        )
        .unwrap()
        .output_image
        .unwrap();
        click_button(&ctx, &mut app, "Apply");
        let pending = app.dialogs.pending_stack_binary.as_mut().unwrap();
        assert_eq!(pending.macro_command, "Convert to Mask");
        assert_eq!(pending.options.method, "Otsu");
        assert_eq!(pending.options.background, "Dark");
        pending.options.calculate = false;
        pending.black_background = true;
        assert_eq!(app.session.get_image(id).unwrap().slices, doc.slices);
        app.resolve_stack_binary_dialog(true);
        assert_eq!(app.session.get_image(id).unwrap().slices, oracle.slices);
        assert_eq!(app.session.get_image(id).unwrap().current_slice, 2);
        assert!(app.session.is_dirty(id));
    }
}
