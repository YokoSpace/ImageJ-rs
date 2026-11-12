use ij_app::action::{AppAction, MENU_TREE, MenuKind};
use ij_app::app::ImageJApp;

fn menu_contains(nodes: &[ij_app::action::MenuNode], path: &[&str]) -> bool {
    let Some((head, tail)) = path.split_first() else {
        return true;
    };
    nodes.iter().any(|node| {
        node.label == *head
            && if tail.is_empty() {
                true
            } else if let MenuKind::Submenu(children) = node.kind {
                menu_contains(children, tail)
            } else {
                false
            }
    })
}

#[test]
fn rotate_and_size_menu_dialog_apply_cancel() {
    assert!(menu_contains(
        MENU_TREE,
        &["Image", "Transform", "Rotate... "]
    ));
    assert!(menu_contains(MENU_TREE, &["Image", "Adjust", "Size..."]));
    let ctx = egui::Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("t", 4, 4).unwrap();
    app.perform(AppAction::Rotate, &ctx);
    assert!(app.dialogs.rotate.is_some());
    assert_eq!(app.dialogs.rotate.as_ref().unwrap().options.angle, 15.0);
    app.cancel_rotate_dialog();
    assert_eq!(app.session.active_doc().unwrap().width, 4);
    app.perform(AppAction::Size, &ctx);
    assert!(app.dialogs.size.is_some());
    assert_eq!(app.dialogs.size.as_ref().unwrap().width, 2);
    app.dialogs.size.as_mut().unwrap().width = 3;
    app.dialogs.size.as_mut().unwrap().height = 2;
    app.dialogs.size.as_mut().unwrap().constrain = false;
    app.dialogs.size.as_mut().unwrap().average_when_downsizing = false;
    app.apply_size_dialog().unwrap();
    assert_eq!(
        (
            app.session.active_doc().unwrap().width,
            app.session.active_doc().unwrap().height
        ),
        (3, 2)
    );
    assert!(
        app.session
            .macro_recorder
            .last()
            .unwrap()
            .contains("Size...")
    );
}

#[test]
fn rotate_dialog_records_macro_and_size_cancel_preserves_image() {
    let ctx = egui::Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("t", 4, 4).unwrap();
    app.perform(AppAction::Rotate, &ctx);
    let dialog = app.dialogs.rotate.as_mut().unwrap();
    dialog.options.angle = 30.0;
    dialog.options.grid_lines = 0;
    app.apply_rotate_dialog().unwrap();
    assert!(
        app.session
            .macro_recorder
            .last()
            .unwrap()
            .contains("angle=30")
    );
    let before = app.session.active_doc().unwrap().slices.clone();
    app.perform(AppAction::Size, &ctx);
    app.cancel_size_dialog();
    assert_eq!(app.session.active_doc().unwrap().slices, before);
}
