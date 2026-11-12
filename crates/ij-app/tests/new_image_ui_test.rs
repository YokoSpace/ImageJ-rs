//! P4.1b: `File > New > Image...` — field order, Java defaults, OK/Cancel,
//! validation, macro recording and persisted defaults.
//!
//! Expected labels, order and defaults come from NewImage.showDialog
//! (original_imagej/ij/gui/NewImage.java 393-442) and the recorded macro
//! format from Recorder.appendNewImage
//! (original_imagej/ij/plugin/frame/Recorder.java 726-753).

use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::action::{AppAction, MENU_TREE, MenuKind};
use ij_app::app::ImageJApp;
use ij_core::PixelPlane;

fn file_new_menu() -> &'static [ij_app::action::MenuNode] {
    let file = MENU_TREE.iter().find(|node| node.label == "File").expect("File menu");
    let MenuKind::Submenu(nodes) = file.kind else {
        panic!("File must be a submenu");
    };
    let new = nodes.iter().find(|node| node.label == "New").expect("New submenu");
    let MenuKind::Submenu(nodes) = &new.kind else {
        panic!("New must be a submenu");
    };
    nodes
}

fn render_dialog(ctx: &Context, app: &mut ImageJApp, events: Vec<Event>) {
    let mut input = RawInput::default();
    input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(460.0, 340.0)));
    input.events = events;
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_new_image_content(ui));
    });
}

fn click_button(ctx: &Context, app: &mut ImageJApp, label: &str) {
    render_dialog(ctx, app, vec![]);
    let pos = ImageJApp::new_image_button_rect(ctx, label)
        .unwrap_or_else(|| panic!("missing button rect for {label}"))
        .center();
    render_dialog(ctx, app, vec![Event::PointerMoved(pos)]);
    render_dialog(
        ctx,
        app,
        vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
    );
    render_dialog(
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
fn file_new_menu_wires_new_image() {
    let item = file_new_menu()
        .iter()
        .find(|node| node.label == "Image...")
        .expect("Image... item");
    assert!(matches!(item.kind, MenuKind::Item(AppAction::NewImage)));
    assert_eq!(item.shortcut, None);
}

#[test]
fn new_image_dialog_opens_with_java_defaults() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.perform(AppAction::NewImage, &ctx);
    let dialog = &app.dialogs.new_image;
    assert!(dialog.open);
    // NewImage.java 26-31: "Untitled", GRAY8, FILL_BLACK, 512x512x1.
    assert_eq!(dialog.name, "Untitled");
    assert_eq!(dialog.type_index, 0);
    assert_eq!(dialog.fill_index, 1);
    assert_eq!(dialog.width, "512");
    assert_eq!(dialog.height, "512");
    assert_eq!(dialog.slices, "1");
}

#[test]
fn ok_creates_default_image_and_records_macro() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.perform(AppAction::NewImage, &ctx);
    click_button(&ctx, &mut app, "OK");
    assert!(!app.dialogs.new_image.open);
    let ids = app.session.image_ids();
    assert_eq!(ids.len(), 1);
    let doc = app.session.get_image(ids[0]).unwrap();
    assert_eq!(doc.title, "Untitled");
    assert_eq!((doc.width, doc.height, doc.slices.len()), (512, 512, 1));
    assert!(matches!(doc.plane(), PixelPlane::Gray8(p) if p.iter().all(|v| *v == 0)));
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "newImage(\"Untitled\", \"8-bit black\", 512, 512, 1);"
    );
}

#[test]
fn cancel_creates_no_image_and_records_nothing() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.perform(AppAction::NewImage, &ctx);
    click_button(&ctx, &mut app, "Cancel");
    assert!(!app.dialogs.new_image.open);
    assert!(app.session.image_ids().is_empty());
    assert!(app.session.macro_recorder.is_empty());
}

#[test]
fn width_or_height_below_one_shows_java_error() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.perform(AppAction::NewImage, &ctx);
    app.dialogs.new_image.width = "0".into();
    click_button(&ctx, &mut app, "OK");
    // showDialog aborts with IJ.error("Width and height must be >0")
    // (NewImage.java 430-433); the dialog closes and nothing is created.
    assert!(!app.dialogs.new_image.open);
    assert!(app.session.image_ids().is_empty());
    assert_eq!(
        app.dialogs.last_error.as_deref(),
        Some("Width and height must be >0")
    );
}

#[test]
fn defaults_persist_across_invocations() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.perform(AppAction::NewImage, &ctx);
    app.dialogs.new_image.name = "Foo".into();
    app.dialogs.new_image.type_index = 1;
    app.dialogs.new_image.fill_index = 0;
    app.dialogs.new_image.width = "64".into();
    app.dialogs.new_image.height = "32".into();
    app.dialogs.new_image.slices = "2".into();
    click_button(&ctx, &mut app, "OK");
    assert_eq!(app.session.image_ids().len(), 1);
    // showDialog writes the accepted values back into the static fields
    // (NewImage.java 435-441).
    app.perform(AppAction::NewImage, &ctx);
    let dialog = &app.dialogs.new_image;
    assert!(dialog.open);
    assert_eq!(dialog.name, "Foo");
    assert_eq!(dialog.type_index, 1);
    assert_eq!(dialog.fill_index, 0);
    assert_eq!(dialog.width, "64");
    assert_eq!(dialog.height, "32");
    assert_eq!(dialog.slices, "2");
}

#[test]
fn every_type_fill_combo_builds_and_records() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let cases = [
        (0usize, 0usize, "8-bit white"),
        (0, 2, "8-bit ramp"),
        (1, 1, "16-bit black"),
        (2, 1, "32-bit black"),
        (3, 1, "RGB black"),
    ];
    for (type_index, fill_index, recorded_type) in cases {
        app.perform(AppAction::NewImage, &ctx);
        app.dialogs.new_image.type_index = type_index;
        app.dialogs.new_image.fill_index = fill_index;
        app.dialogs.new_image.width = "8".into();
        app.dialogs.new_image.height = "4".into();
        click_button(&ctx, &mut app, "OK");
        assert_eq!(
            app.session.macro_recorder.last().unwrap(),
            &format!("newImage(\"Untitled\", \"{recorded_type}\", 8, 4, 1);"),
            "recorded macro for {recorded_type}"
        );
    }
    assert_eq!(app.session.image_ids().len(), cases.len());
    for (index, (_, _, recorded_type)) in cases.iter().enumerate() {
        let id = app.session.image_ids()[index];
        let doc = app.session.get_image(id).unwrap();
        match recorded_type.split(' ').next().unwrap() {
            "8-bit" => assert!(matches!(doc.plane(), PixelPlane::Gray8(_))),
            "16-bit" => assert!(matches!(doc.plane(), PixelPlane::Gray16(_))),
            "32-bit" => assert!(matches!(doc.plane(), PixelPlane::Gray32(_))),
            "RGB" => assert!(matches!(doc.plane(), PixelPlane::ColorRgb(_))),
            other => panic!("unexpected type {other}"),
        }
    }
}
