use ij_app::action::{AppAction, MENU_TREE, MenuKind};
use ij_app::app::ImageJApp;
use ij_core::{ImageDocument, Lut256, PixelPlane};
use ij_session::ImageId;

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

fn gray8_stack(slices: usize) -> ImageDocument {
    ImageDocument::new_stack(
        "stk",
        4,
        3,
        (1..=slices as u8)
            .map(|s| PixelPlane::Gray8((0..12).map(|i| s * 10 + i).collect::<Vec<u8>>()))
            .collect(),
    )
    .unwrap()
}

fn app_with(doc: ImageDocument) -> (ImageJApp, ImageId) {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(doc);
    (app, id)
}

#[test]
fn zproject_menu_entry_and_enablement() {
    assert!(menu_contains(
        MENU_TREE,
        &["Image", "Stacks", "Z Project..."]
    ));
    // The plugin menu item is enabled whenever an image is open; the
    // "Stack required" check happens inside ZProjector.run2
    // (ZProjector.java 136-147).
    let app = ImageJApp::new();
    assert!(app.action_enabled(AppAction::ZProject).is_err());
    let (app, _) = app_with(gray8_stack(3));
    assert!(app.action_enabled(AppAction::ZProject).is_ok());
    let (app, _) =
        app_with(ImageDocument::new("one", 4, 3, PixelPlane::Gray8(vec![7; 12])).unwrap());
    assert!(
        app.action_enabled(AppAction::ZProject).is_ok(),
        "single-slice images keep the item enabled like Java"
    );
}

#[test]
fn zproject_dialog_defaults_and_method_persistence() {
    let ctx = egui::Context::default();
    let (mut app, source_id) = app_with(gray8_stack(5));
    app.perform(AppAction::ZProject, &ctx);
    // buildControlDialog defaults (ZProjector.java 285-297): 1..stackSize and
    // the Prefs-backed projection type, initially Average Intensity.
    let dialog = app.dialogs.zproject.clone();
    assert!(dialog.open);
    assert_eq!((dialog.start.as_str(), dialog.stop.as_str()), ("1", "5"));
    assert_eq!(dialog.method_index, 0);

    // Choosing Sum Slices persists through Prefs.set(METHOD_KEY)
    // (ZProjector.java 173) so the next dialog opens on it; the range
    // always resets (ZProjector.java 202-213).
    app.dialogs.zproject.method_index = 3;
    app.dialogs.zproject.start = "2".into();
    app.dialogs.zproject.stop = "4".into();
    let id = app.apply_zproject_dialog(&ctx).unwrap().unwrap();
    assert_eq!(app.session.get_image(id).unwrap().title, "SUM_stk");
    // The single-slice projection became active; re-select the stack like a
    // user clicking back to the source window.
    app.session.set_active_image(source_id).unwrap();
    app.perform(AppAction::ZProject, &ctx);
    let dialog = app.dialogs.zproject.clone();
    assert_eq!(dialog.method_index, 3, "method persists across invocations");
    assert_eq!(
        (dialog.start.as_str(), dialog.stop.as_str()),
        ("1", "5"),
        "range resets to the full stack each run"
    );
}

#[test]
fn zproject_ok_creates_output_records_and_activates() {
    let ctx = egui::Context::default();
    let (mut app, source_id) = app_with(gray8_stack(4));
    app.perform(AppAction::ZProject, &ctx);
    app.dialogs.zproject.method_index = 1; // Max Intensity
    app.dialogs.zproject.start = "2".into();
    app.dialogs.zproject.stop = "3".into();
    let id = app.apply_zproject_dialog(&ctx).unwrap().unwrap();
    let doc = app.session.get_image(id).unwrap();
    assert_eq!(doc.title, "MAX_stk");
    assert!(matches!(doc.plane(), PixelPlane::Gray8(_)));
    assert_eq!(app.active_image_id(), Some(id));
    assert!(app.session.is_dirty(id));
    assert!(!app.session.is_dirty(source_id));
    // GenericDialog smart recording omits unchanged fields; both were
    // edited here and the choice is recorded unconditionally
    // (ZProjector.java 164-173, GenericDialog.java 1203-1205).
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Z Project...\", \"start=2 stop=3 projection=[Max Intensity]\");")
    );
}

#[test]
fn zproject_default_run_records_only_the_choice() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with(gray8_stack(3));
    app.perform(AppAction::ZProject, &ctx);
    app.apply_zproject_dialog(&ctx).unwrap().unwrap();
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Z Project...\", \"projection=[Average Intensity]\");")
    );
}

#[test]
fn zproject_cancel_creates_nothing() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with(gray8_stack(3));
    let image_count = app.session.image_ids().len();
    app.perform(AppAction::ZProject, &ctx);
    assert!(app.dialogs.zproject.open);
    app.cancel_zproject_dialog();
    assert!(!app.dialogs.zproject.open);
    assert_eq!(app.session.image_ids().len(), image_count);
    assert!(app.session.macro_recorder.is_empty());
}

#[test]
fn zproject_stack_required_error() {
    let ctx = egui::Context::default();
    let (mut app, _) =
        app_with(ImageDocument::new("one", 4, 3, PixelPlane::Gray8(vec![7; 12])).unwrap());
    // IJ.error("Z Project", "Stack required") precedes the dialog
    // (ZProjector.java 143-147).
    app.perform(AppAction::ZProject, &ctx);
    assert!(!app.dialogs.zproject.open);
    assert_eq!(app.dialogs.last_error.as_deref(), Some("Stack required"));
}

#[test]
fn zproject_invalid_numeric_field_uses_default_bounds() {
    let ctx = egui::Context::default();
    let (mut app, source) = app_with(gray8_stack(3));
    app.perform(AppAction::ZProject, &ctx);
    app.dialogs.zproject.start = "abc".into();
    // GenericDialog.getNextNumber returns NaN; ZProjector never checks
    // invalidNumber(), so Java's (int)NaN is ignored by setStartSlice.
    let id = app.apply_zproject_dialog(&ctx).unwrap().unwrap();
    assert!(!app.dialogs.zproject.open);
    assert!(app.dialogs.last_error.is_none());
    assert_eq!(app.session.get_image(source).unwrap().stack_size(), 3);
    let PixelPlane::Gray8(pixels) = app.session.get_image(id).unwrap().plane() else {
        panic!("8-bit average")
    };
    assert_eq!(pixels[0], 20);
    assert_eq!(
        app.session.macro_recorder.last().unwrap(),
        "run(\"Z Project...\", \"start=abc projection=[Average Intensity]\");"
    );
}

#[test]
fn zproject_rejects_rgb_and_hyperstack_on_ok() {
    let ctx = egui::Context::default();
    // 24-bit input takes Java's doRGBProjection branch (ZProjector.java
    // 309-311), which P4.4a does not implement; OK must fail explicitly.
    let (mut app, _) = app_with(
        ImageDocument::new_stack(
            "rgb",
            4,
            3,
            vec![
                PixelPlane::ColorRgb(vec![0xff000000; 12]),
                PixelPlane::ColorRgb(vec![0xff00ff00; 12]),
            ],
        )
        .unwrap(),
    );
    app.perform(AppAction::ZProject, &ctx);
    assert!(app.dialogs.zproject.open);
    assert!(app.apply_zproject_dialog(&ctx).is_err());
    assert!(app.dialogs.last_error.is_some());
    assert_eq!(app.session.image_ids().len(), 1, "no output created");

    let (mut app, _) = app_with(
        ImageDocument::new_hyperstack(
            "hs",
            4,
            3,
            (0..4)
                .map(|i| PixelPlane::Gray8(vec![i as u8; 12]))
                .collect(),
            2,
            2,
            1,
        )
        .unwrap(),
    );
    app.perform(AppAction::ZProject, &ctx);
    assert!(app.dialogs.zproject.open);
    assert!(app.apply_zproject_dialog(&ctx).is_err());
    assert_eq!(app.session.image_ids().len(), 1, "no output created");
}

#[test]
fn zproject_inverted_lut_warns_before_dialog() {
    let ctx = egui::Context::default();
    // A strictly descending LUT makes isInvertedLut() true
    // (ImageProcessor.java 299-318); ZProjector warns via
    // showMessageWithCancel before building the dialog
    // (ZProjector.java 149-153).
    let lut = Lut256 {
        r: (0..=255).rev().collect(),
        g: (0..=255).rev().collect(),
        b: (0..=255).rev().collect(),
    };
    let (mut app, _) = app_with(
        ImageDocument::new_stack(
            "inv",
            4,
            3,
            vec![
                PixelPlane::Color256 {
                    pixels: vec![1; 12],
                    lut: Box::new(lut.clone()),
                },
                PixelPlane::Color256 {
                    pixels: vec![2; 12],
                    lut: Box::new(lut),
                },
            ],
        )
        .unwrap(),
    );
    app.perform(AppAction::ZProject, &ctx);
    assert!(app.dialogs.zproject_lut_pending.is_some());
    assert!(!app.dialogs.zproject.open);

    // Cancel aborts the command without a dialog or output.
    app.resolve_zproject_lut_warning(false);
    assert!(app.dialogs.zproject_lut_pending.is_none());
    assert!(!app.dialogs.zproject.open);
    assert_eq!(app.session.image_ids().len(), 1);

    // OK continues into the projection dialog, which projects the 8-bit
    // indexed stack like Java's ByteProcessor path.
    app.perform(AppAction::ZProject, &ctx);
    app.resolve_zproject_lut_warning(true);
    assert!(app.dialogs.zproject.open);
    let id = app.apply_zproject_dialog(&ctx).unwrap().unwrap();
    assert!(matches!(
        app.session.get_image(id).unwrap().plane(),
        PixelPlane::Color256 { .. }
    ));
}

#[test]
fn zproject_records_original_numeric_fields() {
    // GenericDialog 1200-1205 compares original text; trim (1245-1250)
    // removes .0/.00, and Recorder.recordOption keeps hex text verbatim.
    for (start, stop, expected) in [
        ("1.0", "3.00", "start=1 stop=3"),
        ("0x2", "3", "start=0x2"),
        ("0xz", "3", "start=0xz"),
        ("2d", "3", "start=2d"),
    ] {
        let ctx = egui::Context::default();
        let (mut app, _) = app_with(gray8_stack(3));
        app.perform(AppAction::ZProject, &ctx);
        app.dialogs.zproject.start = start.into();
        app.dialogs.zproject.stop = stop.into();
        let id = app.apply_zproject_dialog(&ctx).unwrap().unwrap();
        assert_eq!(
            app.session.macro_recorder.last().unwrap(),
            &format!("run(\"Z Project...\", \"{expected} projection=[Average Intensity]\");")
        );
        let PixelPlane::Gray8(pixels) = app.session.get_image(id).unwrap().plane() else {
            panic!("8-bit average")
        };
        assert_eq!(
            pixels[0],
            if start == "0x2" || start == "2d" {
                25
            } else {
                20
            }
        );
    }
}

#[test]
fn zproject_failed_projection_still_remembers_method() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with(gray8_stack(3));
    app.perform(AppAction::ZProject, &ctx);
    app.dialogs.zproject.method_index = 5;
    app.dialogs.zproject.start = "3".into();
    app.dialogs.zproject.stop = "1".into();
    assert!(app.apply_zproject_dialog(&ctx).is_err());
    assert_eq!(app.dialogs.zproject_method, 5);
    app.perform(AppAction::ZProject, &ctx);
    assert_eq!(app.dialogs.zproject.method_index, 5);
}
