use egui::Context;
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

    let _id = app.open_ramp_8bit("gray", 8, 6).unwrap();
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
