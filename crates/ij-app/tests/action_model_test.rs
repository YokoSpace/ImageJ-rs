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
