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

#[test]
fn image_type_menu_wires_16bit() {
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
    let sixteen_bit = type_nodes
        .iter()
        .find(|node| node.label == "16-bit")
        .expect("16-bit item");
    assert!(matches!(
        sixteen_bit.kind,
        ij_app::action::MenuKind::Item(AppAction::ConvertTo16Bit)
    ));
}

#[test]
fn convert_to_16bit_action_requires_a_supported_image() {
    let mut app = ImageJApp::new();
    assert!(app.action_enabled(AppAction::ConvertTo16Bit).is_err());

    let indexed = ImageDocument::new(
        "indexed",
        2,
        1,
        PixelPlane::Color256 {
            pixels: vec![0, 1],
            lut: Box::new(ij_core::Lut256::default()),
        },
    )
    .unwrap();
    let indexed_id = app.session.insert_doc(indexed);
    app.set_active_window(indexed_id).unwrap();
    assert!(
        app.action_enabled(AppAction::ConvertTo16Bit).is_err(),
        "ImageConverter.java 59-62 rejects COLOR_256"
    );

    let rgb = ImageDocument::new(
        "rgb",
        2,
        1,
        PixelPlane::ColorRgb(vec![0x00ff_0000, 0x0000_00ff]),
    )
    .unwrap();
    let rgb_id = app.session.insert_doc(rgb);
    app.set_active_window(rgb_id).unwrap();
    assert!(app.action_enabled(AppAction::ConvertTo16Bit).is_ok());

    let rgb_stack = ImageDocument::new_stack(
        "rgb-stack",
        2,
        1,
        vec![
            PixelPlane::ColorRgb(vec![0x00ff_0000, 0x0000_00ff]),
            PixelPlane::ColorRgb(vec![0x0000_ff00, 0x00ff_ffff]),
        ],
    )
    .unwrap();
    let stack_id = app.session.insert_doc(rgb_stack);
    app.set_active_window(stack_id).unwrap();
    assert!(
        app.action_enabled(AppAction::ConvertTo16Bit).is_err(),
        "StackConverter.java 145-148 rejects RGB stacks"
    );
}

#[test]
fn convert_to_16bit_action_converts_and_records() {
    let mut app = ImageJApp::new();
    let doc = ImageDocument::new(
        "gray8",
        3,
        1,
        PixelPlane::Gray8(vec![0, 100, 250]),
    )
    .unwrap();
    let id = app.session.insert_doc(doc);
    app.session.get_image_mut(id).unwrap().display_range = ij_core::DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    assert!(app.action_enabled(AppAction::ConvertTo16Bit).is_ok());

    app.perform(AppAction::ConvertTo16Bit, &Context::default());

    let doc = app.session.get_image(id).unwrap();
    assert_eq!(doc.plane(), &PixelPlane::Gray16(vec![0, 100, 250]));
    assert_eq!(
        doc.display_range,
        ij_core::DisplayRange {
            min: 0.0,
            max: 250.0
        },
        "a new ShortProcessor reports its own data range (ShortProcessor.java 70-87)"
    );
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"16-bit\");")
    );
}

#[test]
fn menu_tree_wires_type_32bit() {
    let image_menu = MENU_TREE
        .iter()
        .find(|node| node.label == "Image")
        .expect("Image menu");
    let ij_app::action::MenuKind::Submenu(image_nodes) = image_menu.kind else {
        panic!("Image must be a submenu");
    };
    let type_menu = image_nodes
        .iter()
        .find(|node| node.label == "Type")
        .expect("Type submenu");
    let ij_app::action::MenuKind::Submenu(type_nodes) = type_menu.kind else {
        panic!("Type must be a submenu");
    };
    let thirty_two_bit = type_nodes
        .iter()
        .find(|node| node.label == "32-bit")
        .expect("32-bit item");
    assert!(matches!(
        thirty_two_bit.kind,
        ij_app::action::MenuKind::Item(AppAction::ConvertTo32Bit)
    ));
}

#[test]
fn convert_to_32bit_action_requires_a_supported_image() {
    let mut app = ImageJApp::new();
    assert!(app.action_enabled(AppAction::ConvertTo32Bit).is_err());

    let indexed = ImageDocument::new(
        "indexed",
        2,
        1,
        PixelPlane::Color256 {
            pixels: vec![0, 1],
            lut: Box::new(ij_core::Lut256::default()),
        },
    )
    .unwrap();
    let indexed_id = app.session.insert_doc(indexed);
    app.set_active_window(indexed_id).unwrap();
    assert!(
        app.action_enabled(AppAction::ConvertTo32Bit).is_err(),
        "ImageConverter.java 174-175 rejects COLOR_256"
    );

    let rgb = ImageDocument::new(
        "rgb",
        2,
        1,
        PixelPlane::ColorRgb(vec![0x00ff_0000, 0x0000_00ff]),
    )
    .unwrap();
    let rgb_id = app.session.insert_doc(rgb);
    app.set_active_window(rgb_id).unwrap();
    assert!(app.action_enabled(AppAction::ConvertTo32Bit).is_ok());

    let rgb_stack = ImageDocument::new_stack(
        "rgb-stack",
        2,
        1,
        vec![
            PixelPlane::ColorRgb(vec![0x00ff_0000, 0x0000_00ff]),
            PixelPlane::ColorRgb(vec![0x0000_ff00, 0x00ff_ffff]),
        ],
    )
    .unwrap();
    let stack_id = app.session.insert_doc(rgb_stack);
    app.set_active_window(stack_id).unwrap();
    assert!(
        app.action_enabled(AppAction::ConvertTo32Bit).is_ok(),
        "StackConverter.java 184-185 accepts RGB stacks"
    );
}

#[test]
fn convert_to_32bit_action_converts_and_records() {
    let mut app = ImageJApp::new();
    let doc = ImageDocument::new(
        "gray8",
        3,
        1,
        PixelPlane::Gray8(vec![0, 100, 250]),
    )
    .unwrap();
    let id = app.session.insert_doc(doc);
    app.session.get_image_mut(id).unwrap().display_range = ij_core::DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    assert!(app.action_enabled(AppAction::ConvertTo32Bit).is_ok());

    app.perform(AppAction::ConvertTo32Bit, &Context::default());

    let doc = app.session.get_image(id).unwrap();
    assert_eq!(doc.plane(), &PixelPlane::Gray32(vec![0.0, 100.0, 250.0]));
    assert_eq!(
        doc.display_range,
        ij_core::DisplayRange {
            min: 10.0,
            max: 200.0
        },
        "ImageConverter.java 188 restores the original display range via IJ.setMinAndMax"
    );
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"32-bit\");")
    );
}

#[test]
fn image_type_menu_wires_rgb_color() {
    let image_menu = ij_app::action::MENU_TREE
        .iter()
        .find(|node| node.label == "Image")
        .expect("Image menu");
    let ij_app::action::MenuKind::Submenu(image_nodes) = image_menu.kind else {
        panic!("Image must be a submenu");
    };
    let type_menu = image_nodes
        .iter()
        .find(|node| node.label == "Type")
        .expect("Type submenu");
    let ij_app::action::MenuKind::Submenu(type_nodes) = type_menu.kind else {
        panic!("Type must be a submenu");
    };
    let rgb = type_nodes
        .iter()
        .find(|node| node.label == "RGB Color")
        .expect("RGB Color item");
    assert!(matches!(
        rgb.kind,
        ij_app::action::MenuKind::Item(AppAction::ConvertToRgb)
    ));
}

#[test]
fn convert_to_rgb_action_requires_an_image_and_accepts_every_type() {
    let mut app = ImageJApp::new();
    assert!(app.action_enabled(AppAction::ConvertToRgb).is_err());

    for plane in [
        PixelPlane::Gray8(vec![0, 1]),
        PixelPlane::Gray16(vec![0, 1]),
        PixelPlane::Gray32(vec![0.0, 1.0]),
        PixelPlane::Color256 {
            pixels: vec![0, 1],
            lut: Box::new(ij_core::Lut256::default()),
        },
        PixelPlane::ColorRgb(vec![0xff00_0000, 0xffff_ffff]),
    ] {
        let id = app
            .session
            .insert_doc(ImageDocument::new("any", 2, 1, plane).unwrap());
        app.set_active_window(id).unwrap();
        assert!(app.action_enabled(AppAction::ConvertToRgb).is_ok());
    }
}

#[test]
fn convert_to_rgb_action_converts_with_display_range_and_records() {
    let mut app = ImageJApp::new();
    let doc = ImageDocument::new(
        "gray16",
        3,
        1,
        PixelPlane::Gray16(vec![100, 300, 500]),
    )
    .unwrap();
    let id = app.session.insert_doc(doc);
    app.session.get_image_mut(id).unwrap().display_range = ij_core::DisplayRange {
        min: 100.0,
        max: 500.0,
    };

    app.perform(AppAction::ConvertToRgb, &Context::default());

    let doc = app.session.get_image(id).unwrap();
    // TypeConverter.java 50-61: (300-100)*256/401 + 0.5 = 128.2, which truncates to 128.
    assert_eq!(
        doc.plane(),
        &PixelPlane::ColorRgb(vec![0xff00_0000, 0xff80_8080, 0xffff_ffff])
    );
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"RGB Color\");")
    );
}
