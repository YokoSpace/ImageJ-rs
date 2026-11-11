use egui::{Context, Event, Key, Modifiers, RawInput};
use ij_app::ImageJApp;
use ij_app::action::{AppAction, MENU_TREE};
use ij_core::{ImageDocument, PixelPlane};

#[test]
fn actions_report_when_they_can_run() {
    let mut app = ImageJApp::new();
    assert!(app.action_enabled(AppAction::Measure).is_err());

    let gray = app.open_ramp_8bit("gray", 4, 4).unwrap();
    assert!(app.action_enabled(AppAction::Measure).is_ok());
    assert!(app.action_enabled(AppAction::Median).is_ok());

    let rgb = ImageDocument::new("rgb", 4, 4, PixelPlane::ColorRgb(vec![0; 16])).unwrap();
    let rgb_id = app.session.insert_doc(rgb);
    app.set_active_window(rgb_id).unwrap();
    assert!(app.action_enabled(AppAction::Median).is_err());
    app.set_active_window(gray).unwrap();
}

#[test]
fn invert_action_preserves_pixels_and_macro_recording() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("gray", 4, 4).unwrap();
    let before = match app.session.get_image(id).unwrap().plane() {
        PixelPlane::Gray8(pixels) => pixels[0],
        _ => panic!("expected 8-bit image"),
    };

    app.perform(AppAction::Invert, &Context::default());

    let after = match app.session.get_image(id).unwrap().plane() {
        PixelPlane::Gray8(pixels) => pixels[0],
        _ => panic!("expected 8-bit image"),
    };
    assert_eq!(after, 255 - before);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Invert\");")
    );
}

#[test]
fn menu_has_the_classic_top_level_order() {
    let labels: Vec<_> = MENU_TREE.iter().map(|node| node.label).collect();
    assert_eq!(
        labels,
        [
            "File", "Edit", "Image", "Process", "Analyze", "Plugins", "Window", "Help"
        ]
    );
}

#[test]
fn crop_requires_an_area_selection() {
    let mut app = ImageJApp::new();
    assert!(app.action_enabled(AppAction::Crop).is_err());

    let id = app.open_ramp_8bit("gray", 8, 6).unwrap();
    assert!(app.action_enabled(AppAction::Crop).is_err());

    app.session
        .dispatch(ij_session::Command::MakeLineRoi {
            x1: 1,
            y1: 1,
            x2: 5,
            y2: 4,
        })
        .unwrap();
    assert!(
        app.action_enabled(AppAction::Crop).is_err(),
        "line selections are not areas"
    );

    app.session
        .dispatch(ij_session::Command::MakeRectRoi {
            x: 2,
            y: 1,
            width: 4,
            height: 3,
        })
        .unwrap();
    assert!(app.action_enabled(AppAction::Crop).is_ok());

    app.session
        .dispatch(ij_session::Command::MakeOvalRoi {
            x: 2,
            y: 1,
            width: 4,
            height: 3,
        })
        .unwrap();
    assert!(app.action_enabled(AppAction::Crop).is_ok());

    app.session
        .get_image_mut(id)
        .unwrap()
        .set_oval_roi(-2, 1, 6, 4)
        .unwrap();
    assert_eq!(
        app.action_enabled(AppAction::Crop),
        Err("Crop of clipped non-rectangular ROI requires ShapeRoi".into())
    );
}

#[test]
fn crop_action_crops_the_active_document_and_records_the_macro() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("gray", 8, 6).unwrap();
    app.session
        .dispatch(ij_session::Command::MakeRectRoi {
            x: 2,
            y: 1,
            width: 4,
            height: 3,
        })
        .unwrap();

    app.perform(AppAction::Crop, &Context::default());

    let doc = app.session.get_image(id).unwrap();
    assert_eq!((doc.width, doc.height), (4, 3));
    assert_eq!(doc.roi, None);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Crop\");")
    );
}

#[test]
fn image_menu_wires_crop_with_shift_x() {
    let image = MENU_TREE
        .iter()
        .find(|node| node.label == "Image")
        .expect("Image menu");
    let ij_app::action::MenuKind::Submenu(nodes) = image.kind else {
        panic!("Image must be a submenu");
    };
    let crop = nodes
        .iter()
        .find(|node| node.label == "Crop")
        .expect("Crop item");
    assert_eq!(crop.shortcut, Some("Shift+X"));
    assert!(matches!(
        crop.kind,
        ij_app::action::MenuKind::Item(AppAction::Crop)
    ));
}

#[test]
fn shift_x_crops_the_active_image() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("gray", 8, 6).unwrap();
    app.session
        .dispatch(ij_session::Command::MakeRectRoi {
            x: 2,
            y: 1,
            width: 4,
            height: 3,
        })
        .unwrap();
    let modifiers = Modifiers::SHIFT;
    let input = RawInput {
        modifiers,
        events: vec![Event::Key {
            key: Key::X,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
        ..Default::default()
    };
    let ctx = Context::default();
    let _ = ctx.run(input, |ctx| app.handle_image_shortcuts(ctx, id));
    let doc = app.session.get_image(id).unwrap();
    assert_eq!((doc.width, doc.height), (4, 3));
    assert_eq!(app.session.macro_recorder.last().map(String::as_str), Some("run(\"Crop\");"));
}

#[test]
fn image_type_menu_wires_8bit() {
    let image = MENU_TREE
        .iter()
        .find(|node| node.label == "Image")
        .expect("Image menu");
    let ij_app::action::MenuKind::Submenu(nodes) = image.kind else {
        panic!("Image must be a submenu");
    };
    let type_menu = nodes
        .iter()
        .find(|node| node.label == "Type")
        .expect("Type submenu");
    let ij_app::action::MenuKind::Submenu(type_nodes) = type_menu.kind else {
        panic!("Type must be a submenu");
    };
    let eight_bit = type_nodes
        .iter()
        .find(|node| node.label == "8-bit")
        .expect("8-bit item");
    assert!(matches!(
        eight_bit.kind,
        ij_app::action::MenuKind::Item(AppAction::ConvertTo8Bit)
    ));
}

#[test]
fn convert_to_8bit_action_requires_an_image_and_converts_it() {
    let mut app = ImageJApp::new();
    assert!(app.action_enabled(AppAction::ConvertTo8Bit).is_err());

    let doc = ImageDocument::new(
        "gray16",
        4,
        2,
        PixelPlane::Gray16(vec![0, 65535, 1000, 2000, 3000, 4000, 5000, 6000]),
    )
    .unwrap();
    let id = app.session.insert_doc(doc);
    app.session.get_image_mut(id).unwrap().display_range = ij_core::DisplayRange {
        min: 0.0,
        max: 65535.0,
    };
    assert!(app.action_enabled(AppAction::ConvertTo8Bit).is_ok());

    app.perform(AppAction::ConvertTo8Bit, &Context::default());

    let doc = app.session.get_image(id).unwrap();
    assert!(matches!(doc.plane(), PixelPlane::Gray8(_)));
    assert_eq!(
        doc.display_range,
        ij_core::DisplayRange {
            min: 0.0,
            max: 255.0
        }
    );
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"8-bit\");")
    );
}
