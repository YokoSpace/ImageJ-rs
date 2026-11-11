use ij_app::ImageJApp;
use ij_bridge::plugin::PluginBridge;
use ij_core::PixelPlane;
use ij_session::SessionError;
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn fixture() -> (ImageJApp, ij_session::ImageId, PathBuf) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let bridge = PluginBridge::locate_from_repo_root(&root).unwrap();
    let mut app = ImageJApp::new().with_plugin_bridge(bridge);
    let id = app.open_ramp_8bit("parameters", 32, 24).unwrap();
    app.set_dirty(id, false);
    (
        app,
        id,
        root.join("tools/plugin-host/fixtures/fixtures.jar"),
    )
}

fn wait_for_dialog(app: &mut ImageJApp) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.plugin_parameters.is_none() {
        assert!(Instant::now() < deadline, "{}", app.status_message);
        if let Some(result) = app.poll_plugin_filter() {
            panic!("expected parameters: {result:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn finish(app: &mut ImageJApp) -> Result<ij_session::PluginFilterOutcome, SessionError> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "{}", app.status_message);
        if let Some(result) = app.poll_plugin_filter() {
            return result;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn frame(app: &mut ImageJApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.render_plugin_browser_window(ctx),
    )
}

fn text_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.visual_bounding_rect())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing rendered text {label}"))
}

#[test]
fn generic_dialog_form_ok_keeps_original_target_and_records_options() {
    let (mut app, id, jar) = fixture();
    let original = app.session.get_image(id).unwrap().clone();
    let other = app.open_ramp_8bit("other", 8, 8).unwrap();
    app.session.set_active_image(id).unwrap();
    app.start_plugin_filter_task("fixtures.ParameterFilter".into(), vec![jar], "".into())
        .unwrap();
    wait_for_dialog(&mut app);
    assert_eq!(app.session.get_image(id).unwrap(), &original);
    assert!(!app.is_dirty(id));
    app.session.set_active_image(other).unwrap();
    let other_before = app.session.get_image(other).unwrap().clone();
    assert_eq!(app.plugin_parameters.as_ref().unwrap().target(), id);
    app.plugin_parameters.as_mut().unwrap().values = vec![
        "7.25".into(),
        "two words".into(),
        "Subtract value".into(),
        "true".into(),
    ];
    let ctx = egui::Context::default();
    let _ = frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    for label in [
        "Fixture parameters",
        "Amount",
        "Text value",
        "Mode",
        "Enabled",
        "levels",
        "7.25",
        "two words",
        "Subtract value",
    ] {
        text_rect(&output, label);
    }
    let pos = text_rect(&output, "OK").center();
    let _ = frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    let _ = frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(app.plugin_parameters.is_none());
    let outcome = finish(&mut app).unwrap();
    assert!(outcome.stdout.contains("PARAMETERS=7.25:two words:1:true"));
    assert_eq!(app.session.get_image(other).unwrap(), &other_before);
    assert_ne!(app.session.get_image(id).unwrap().plane(), original.plane());
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "run(\"fixtures.ParameterFilter\", \"amount=7.25 text_value=[two words] mode=[Subtract value] enabled\");"
    );
}

#[test]
fn generic_dialog_invalid_input_and_escape_leave_document_and_recorder_unchanged() {
    let (mut app, id, jar) = fixture();
    let original = app.session.get_image(id).unwrap().clone();
    let recorder = app.session.macro_recorder.clone();
    app.start_plugin_filter_task("fixtures.ParameterFilter".into(), vec![jar], "".into())
        .unwrap();
    wait_for_dialog(&mut app);
    app.plugin_parameters.as_mut().unwrap().values[0] = "bad number".into();
    assert!(app.submit_plugin_parameters().is_err());
    assert!(app.plugin_parameters.as_ref().unwrap().error.is_some());
    let ctx = egui::Context::default();
    let _ = frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert!(!app.is_plugin_filter_running());
    assert!(app.plugin_parameters.is_none());
    assert_eq!(app.session.get_image(id).unwrap(), &original);
    assert_eq!(app.session.macro_recorder, recorder);
    assert!(!app.is_dirty(id));
}

#[test]
fn generic_dialog_image_change_while_prompting_rejects_the_result() {
    let (mut app, id, jar) = fixture();
    app.start_plugin_filter_task("fixtures.ParameterFilter".into(), vec![jar], "".into())
        .unwrap();
    wait_for_dialog(&mut app);
    if let PixelPlane::Gray8(pixels) = app.session.get_image_mut(id).unwrap().plane_mut() {
        pixels[0] = 91;
    }
    let changed = app.session.get_image(id).unwrap().clone();
    let recorder = app.session.macro_recorder.clone();
    app.submit_plugin_parameters().unwrap();
    assert!(matches!(
        finish(&mut app),
        Err(SessionError::StalePluginFilterResult { .. })
    ));
    assert_eq!(app.session.get_image(id).unwrap(), &changed);
    assert_eq!(app.session.macro_recorder, recorder);
}
