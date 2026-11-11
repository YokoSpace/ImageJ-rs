use ij_app::action::{AppAction, MENU_TREE, MenuKind};
use ij_app::app::ImageJApp;
use ij_core::PixelPlane;

/// ImageJ's Edit > Clear / Fill / Draw entries (Menus.java 148-151): Clear
/// has no shortcut, Fill is bound to unmodified F and Draw to unmodified D.
const EDIT_MENU_ITEMS: &[(&str, &str)] = &[("Clear", ""), ("Fill", "F"), ("Draw", "D")];

fn edit_menu_matches() -> Vec<(String, String)> {
    for top in MENU_TREE {
        if top.label != "Edit" {
            continue;
        }
        let MenuKind::Submenu(items) = top.kind else {
            continue;
        };
        let mut found = Vec::new();
        for node in items {
            for (label, _) in EDIT_MENU_ITEMS {
                if node.label == *label {
                    found.push((
                        node.label.to_string(),
                        node.shortcut.unwrap_or("").to_string(),
                    ));
                }
            }
        }
        return found;
    }
    Vec::new()
}

#[test]
fn edit_menu_lists_clear_fill_and_draw_with_imagej_shortcuts() {
    let found = edit_menu_matches();
    assert_eq!(
        found.len(),
        EDIT_MENU_ITEMS.len(),
        "Edit menu must contain Clear, Fill and Draw exactly once"
    );
    for (label, shortcut) in EDIT_MENU_ITEMS {
        let entry = found
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("Edit menu must contain {label}"));
        assert_eq!(
            &entry.1, shortcut,
            "Edit > {label} shortcut must match ImageJ's Edit menu"
        );
    }
}

fn gray_pixel(app: &ImageJApp, x: usize, y: usize) -> Option<u8> {
    let doc = app.session.active_doc().ok()?;
    let width = doc.width as usize;
    match &doc.slices[0] {
        PixelPlane::Gray8(v) => v.get(y * width + x).copied(),
        _ => None,
    }
}

#[test]
fn perform_clear_fill_and_draw_reaches_the_active_image() {
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("edit-fgd", 8, 8).unwrap();
    app.session
        .dispatch(ij_session::Command::MakeRectRoi {
            x: 1,
            y: 1,
            width: 4,
            height: 4,
        })
        .unwrap();
    app.session
        .dispatch(ij_session::Command::SetForegroundColor {
            color: [10, 20, 30],
        })
        .unwrap();
    app.session
        .dispatch(ij_session::Command::SetBackgroundColor {
            color: [200, 200, 200],
        })
        .unwrap();

    // Clear fills the selection rectangle with the background gray
    // (Filled: getBestIndex of (200,200,200) on the default gray model
    // collapses to index 200).
    app.perform(AppAction::Clear, &egui::Context::default());
    assert_eq!(gray_pixel(&app, 1, 1), Some(200), "Clear at (1,1)");
    assert_eq!(gray_pixel(&app, 4, 4), Some(200), "Clear at (4,4)");
    assert_eq!(
        gray_pixel(&app, 5, 5),
        Some(160),
        "Clear must not touch (5,5)"
    );

    // Fill paints the selection with the foreground gray; (10,20,30)
    // maps to gray index 20 via ImageJ's wtighted-square-distance rule.
    app.perform(AppAction::Fill, &egui::Context::default());
    assert_eq!(gray_pixel(&app, 1, 1), Some(20), "Fill at (1,1)");
    assert_eq!(gray_pixel(&app, 4, 4), Some(20), "Fill at (4,4)");

    // Draw strokes the selection outline with the foreground gray;
    // at the default 1-pixel width the stroke spans the inclusive
    // rectangle (0,0)..(5,5) (Roi.drawPixels -> drawRect(w+1, h+1)).
    app.perform(AppAction::Draw, &egui::Context::default());
    assert_eq!(gray_pixel(&app, 5, 5), Some(20), "Draw strokes (5,5)");
    assert_ne!(
        gray_pixel(&app, 6, 6),
        Some(20),
        "Draw must not touch (6,6)"
    );
}
