//! P4.4b: `Image > Stacks > Add Slice` / `Delete Slice` in the desktop app.
//!
//! Menu labels and the absence of shortcuts come from IJ_Props.txt 147-148
//! (`stacks01`/`stacks02`). Insertion position, Alt-before, the blank slice,
//! the selected slice after each command and the recorded macro statement come
//! from original_imagej/ij/plugin/StackEditor.java 30-71 and
//! Recorder.saveCommand (Recorder.java 675-676).

use egui::{Context, Modifiers, RawInput};
use ij_app::action::{AppAction, MENU_TREE, MenuKind, MenuNode};
use ij_app::app::ImageJApp;
use ij_app::image_view::{StackAxis, visible_stack_axes};
use ij_core::{ImageDocument, Lut256, PixelPlane};

fn image_stacks_menu() -> &'static [MenuNode] {
    let image = MENU_TREE
        .iter()
        .find(|node| node.label == "Image")
        .expect("Image menu");
    let MenuKind::Submenu(nodes) = image.kind else {
        panic!("Image must be a submenu");
    };
    let stacks = nodes
        .iter()
        .find(|node| node.label == "Stacks")
        .expect("Stacks submenu");
    let MenuKind::Submenu(nodes) = stacks.kind else {
        panic!("Stacks must be a submenu");
    };
    nodes
}

fn gray8_stack(title: &str, values: &[u8]) -> ImageDocument {
    let planes = values
        .iter()
        .map(|value| PixelPlane::Gray8(vec![*value; 6]))
        .collect();
    ImageDocument::new_stack(title, 3, 2, planes).unwrap()
}

fn slice_values(app: &ImageJApp, id: ij_session::ImageId) -> Vec<u8> {
    app.session
        .get_image(id)
        .unwrap()
        .slices
        .iter()
        .map(|plane| match plane {
            PixelPlane::Gray8(pixels) => pixels[0],
            other => panic!("expected Gray8, got {other:?}"),
        })
        .collect()
}

fn perform_with_modifiers(app: &mut ImageJApp, action: AppAction, modifiers: Modifiers) {
    let ctx = Context::default();
    let input = RawInput {
        modifiers,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| app.perform(action, ctx));
}

#[test]
fn image_stacks_menu_wires_add_and_delete_slice_without_shortcuts() {
    let nodes = image_stacks_menu();
    let add = nodes
        .iter()
        .find(|node| node.label == "Add Slice")
        .expect("Add Slice item");
    assert!(matches!(add.kind, MenuKind::Item(AppAction::AddSlice)));
    assert_eq!(add.shortcut, None, "stacks01 has no shortcut");
    let delete = nodes
        .iter()
        .find(|node| node.label == "Delete Slice")
        .expect("Delete Slice item");
    assert!(matches!(
        delete.kind,
        MenuKind::Item(AppAction::DeleteSlice)
    ));
    assert_eq!(delete.shortcut, None, "stacks02 has no shortcut");
    // IJ_Props.txt lists stacks01/stacks02 before the rest of the submenu.
    let add_index = nodes
        .iter()
        .position(|node| node.label == "Add Slice")
        .unwrap();
    let delete_index = nodes
        .iter()
        .position(|node| node.label == "Delete Slice")
        .unwrap();
    assert_eq!(delete_index, add_index + 1);
}

#[test]
fn native_menu_spec_exposes_add_and_delete_slice() {
    let mut app = ImageJApp::new();
    let _ = app.open_ramp_8bit("one", 4, 4).unwrap();
    let spec = app.build_native_menu_spec();
    let image = spec.iter().find(|menu| menu.label == "Image").unwrap();
    let stacks = image
        .children
        .iter()
        .find(|menu| menu.label == "Stacks")
        .unwrap();
    let labels: Vec<&str> = stacks
        .children
        .iter()
        .map(|node| node.label.as_str())
        .collect();
    assert!(labels.contains(&"Add Slice"), "{labels:?}");
    assert!(labels.contains(&"Delete Slice"), "{labels:?}");
    let add = stacks
        .children
        .iter()
        .find(|node| node.label == "Add Slice")
        .unwrap();
    assert_eq!(add.action, Some(AppAction::AddSlice));
    assert!(add.enabled);
}

#[test]
fn add_slice_action_inserts_a_blank_slice_after_the_current_one() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(gray8_stack("stack", &[10, 20, 30]));
    app.set_active_window(id).unwrap();
    app.session.get_image_mut(id).unwrap().set_slice(2).unwrap();

    perform_with_modifiers(&mut app, AppAction::AddSlice, Modifiers::NONE);

    let doc = app.session.get_image(id).unwrap();
    assert_eq!(slice_values(&app, id), vec![10, 20, 0, 30]);
    assert_eq!(doc.current_slice, 3);
    assert_eq!(doc.get_dimensions(), [3, 2, 1, 4, 1]);
    assert_eq!(doc.slice_labels, vec![None, None, None, None]);
    assert!(app.is_dirty(id));
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Add Slice\");")
    );
    assert_eq!(app.status_message, "Add Slice");
    // The same window stays selected; no new image is created.
    assert_eq!(app.active_image_id(), Some(id));
    assert_eq!(app.session.image_ids(), vec![id]);
}

#[test]
fn add_slice_action_with_alt_inserts_before_the_current_slice() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(gray8_stack("stack", &[10, 20, 30]));
    app.set_active_window(id).unwrap();
    app.session.get_image_mut(id).unwrap().set_slice(2).unwrap();

    perform_with_modifiers(&mut app, AppAction::AddSlice, Modifiers::ALT);

    let doc = app.session.get_image(id).unwrap();
    assert_eq!(slice_values(&app, id), vec![10, 0, 20, 30]);
    // StackEditor.java 45 calls setSlice(n+1); with Alt that is the current
    // slice, so the selection does not move.
    assert_eq!(doc.current_slice, 2);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Add Slice\");")
    );
}

#[test]
fn add_slice_action_on_a_single_image_reveals_the_slice_slider() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(gray8_stack("single", &[42]));
    app.set_active_window(id).unwrap();
    assert!(visible_stack_axes(app.session.get_image(id).unwrap()).is_empty());

    perform_with_modifiers(&mut app, AppAction::AddSlice, Modifiers::NONE);

    let doc = app.session.get_image(id).unwrap();
    assert_eq!(doc.stack_size(), 2);
    assert_eq!(doc.current_slice, 2);
    assert_eq!(
        visible_stack_axes(doc),
        vec![StackAxis::FlatSlice],
        "a one-slice image becomes a plain Z-stack"
    );
    assert_eq!(slice_values(&app, id), vec![42, 0]);
}

#[test]
fn delete_slice_action_removes_the_current_slice_for_first_middle_and_last() {
    for (current, expected, expected_current) in [
        (1u32, vec![20u8, 30], 1u32),
        (2, vec![10, 30], 1),
        (3, vec![10, 20], 2),
    ] {
        let mut app = ImageJApp::new();
        let id = app.session.insert_doc(gray8_stack("stack", &[10, 20, 30]));
        app.set_active_window(id).unwrap();
        app.session
            .get_image_mut(id)
            .unwrap()
            .set_slice(current as i32)
            .unwrap();

        perform_with_modifiers(&mut app, AppAction::DeleteSlice, Modifiers::NONE);

        let doc = app.session.get_image(id).unwrap();
        assert_eq!(slice_values(&app, id), expected, "current slice {current}");
        assert_eq!(
            doc.current_slice, expected_current,
            "current slice {current}"
        );
        assert_eq!(doc.get_dimensions(), [3, 2, 1, 2, 1]);
        assert!(app.is_dirty(id));
        assert_eq!(
            app.session.macro_recorder.last().map(String::as_str),
            Some("run(\"Delete Slice\");")
        );
        assert_eq!(app.active_image_id(), Some(id));
    }
}

#[test]
fn delete_slice_action_keeps_the_remaining_slice_label() {
    let mut app = ImageJApp::new();
    let mut doc = gray8_stack("stack", &[10, 20]);
    doc.slice_labels = vec![Some("L1".to_string()), Some("L2".to_string())];
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();

    perform_with_modifiers(&mut app, AppAction::DeleteSlice, Modifiers::NONE);

    let doc = app.session.get_image(id).unwrap();
    assert_eq!(doc.stack_size(), 1);
    // StackEditor.java 62-65 copies the surviving label to the Slice_Label
    // property; the Rust title path reads slice_labels[0] for a one-slice
    // image (ImageWindow.java 327-340), so the label stays in the stack.
    assert_eq!(doc.slice_labels, vec![Some("L2".to_string())]);
}

#[test]
fn delete_slice_action_on_a_single_image_reports_the_java_error() {
    let mut app = ImageJApp::new();
    let id = app.session.insert_doc(gray8_stack("single", &[42]));
    app.set_active_window(id).unwrap();
    let before = app.session.get_image(id).unwrap().clone();

    perform_with_modifiers(&mut app, AppAction::DeleteSlice, Modifiers::NONE);

    assert_eq!(
        app.session.get_image(id).unwrap(),
        &before,
        "state preserved"
    );
    assert!(!app.is_dirty(id));
    assert!(app.session.macro_recorder.is_empty());
    let error = app
        .dialogs
        .last_error
        .as_deref()
        .expect("Delete Slice must surface an error");
    assert!(
        error.contains("\"Delete Slice\" requires a stack"),
        "unexpected error: {error}"
    );
    assert!(
        app.status_message
            .contains("\"Delete Slice\" requires a stack"),
        "unexpected status: {}",
        app.status_message
    );
}

#[test]
fn add_and_delete_slice_are_disabled_for_hyperstacks_and_ct_stacks() {
    let mut app = ImageJApp::new();
    let planes: Vec<PixelPlane> = (0..4)
        .map(|value| PixelPlane::Gray8(vec![value as u8; 6]))
        .collect();
    let mut doc = ImageDocument::new_stack("hyper", 3, 2, planes).unwrap();
    doc.set_dimensions(2, 2, 1).unwrap();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();

    for action in [AppAction::AddSlice, AppAction::DeleteSlice] {
        let reason = app
            .action_enabled(action)
            .expect_err("hyperstack editing is P4.4d2");
        assert!(reason.contains("hyperstack"), "unexpected reason: {reason}");
    }
    let before = app.session.get_image(id).unwrap().clone();
    perform_with_modifiers(&mut app, AppAction::AddSlice, Modifiers::NONE);
    assert_eq!(
        app.session.get_image(id).unwrap(),
        &before,
        "state preserved"
    );
    assert!(app.session.macro_recorder.is_empty());

    // A channel-only stack is not an ImageJ hyperstack but editing it would
    // grow the channel axis, so it is disabled too.
    let mut app = ImageJApp::new();
    let planes: Vec<PixelPlane> = (0..3)
        .map(|value| PixelPlane::Gray8(vec![value as u8; 6]))
        .collect();
    let mut doc = ImageDocument::new_stack("channels", 3, 2, planes).unwrap();
    doc.set_dimensions(3, 1, 1).unwrap();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();
    assert!(app.action_enabled(AppAction::AddSlice).is_err());
    assert!(app.action_enabled(AppAction::DeleteSlice).is_err());
}

#[test]
fn add_slice_keeps_every_pixel_type_and_its_palette() {
    let lut = Lut256 {
        r: (0..=255u8).collect(),
        g: (0..=255u8).rev().collect(),
        b: (0..=255u8).map(|value| value / 2).collect(),
    };
    let cases: Vec<(&str, ImageDocument)> = vec![
        (
            "gray16",
            ImageDocument::new_stack(
                "gray16",
                3,
                2,
                vec![
                    PixelPlane::Gray16(vec![1, 2, 3, 4, 5, 6]),
                    PixelPlane::Gray16(vec![7, 8, 9, 10, 11, 12]),
                ],
            )
            .unwrap(),
        ),
        (
            "gray32",
            ImageDocument::new_stack(
                "gray32",
                3,
                2,
                vec![
                    PixelPlane::Gray32(vec![1.5, 2.5, 3.5, 4.5, 5.5, 6.5]),
                    PixelPlane::Gray32(vec![7.5, 8.5, 9.5, 10.5, 11.5, 12.5]),
                ],
            )
            .unwrap(),
        ),
        (
            "rgb",
            ImageDocument::new_stack(
                "rgb",
                3,
                2,
                vec![
                    PixelPlane::ColorRgb(vec![
                        0x010203, 0x010204, 0x010205, 0x010206, 0x010207, 0x010208,
                    ]),
                    PixelPlane::ColorRgb(vec![
                        0x020406, 0x020407, 0x020408, 0x020409, 0x02040a, 0x02040b,
                    ]),
                ],
            )
            .unwrap(),
        ),
        (
            "color256",
            ImageDocument::new_stack(
                "color256",
                3,
                2,
                vec![
                    PixelPlane::Color256 {
                        pixels: vec![10, 11, 12, 13, 14, 15],
                        lut: Box::new(lut.clone()),
                    },
                    PixelPlane::Color256 {
                        pixels: vec![20, 21, 22, 23, 24, 25],
                        lut: Box::new(lut.clone()),
                    },
                ],
            )
            .unwrap(),
        ),
    ];

    for (tag, doc) in cases {
        let mut app = ImageJApp::new();
        let id = app.session.insert_doc(doc);
        app.set_active_window(id).unwrap();
        perform_with_modifiers(&mut app, AppAction::AddSlice, Modifiers::NONE);
        let doc = app.session.get_image(id).unwrap();
        assert_eq!(doc.stack_size(), 3, "{tag}");
        assert_eq!(doc.current_slice, 2, "{tag}");
        match (&doc.slices[1], tag) {
            (PixelPlane::Gray16(pixels), "gray16") => assert_eq!(pixels, &vec![0u16; 6]),
            (PixelPlane::Gray32(pixels), "gray32") => assert_eq!(pixels, &vec![0f32; 6]),
            (PixelPlane::ColorRgb(pixels), "rgb") => assert_eq!(pixels, &vec![0u32; 6]),
            (
                PixelPlane::Color256 {
                    pixels,
                    lut: new_lut,
                },
                "color256",
            ) => {
                assert_eq!(pixels, &vec![0u8; 6]);
                assert_eq!(**new_lut, lut, "the palette is preserved");
            }
            (plane, tag) => panic!("{tag}: unexpected blank slice {plane:?}"),
        }
    }
}

#[test]
fn slice_edit_clears_threshold_and_disables_threshold_only_mask() {
    for action in [AppAction::AddSlice, AppAction::DeleteSlice] {
        let mut app = ImageJApp::new();
        let mut doc = gray8_stack("threshold", &[10, 20, 30]);
        doc.threshold = Some(ij_core::ThresholdLevels {
            lower: 10.,
            upper: 100.,
        });
        let id = app.session.insert_doc(doc);
        assert!(app.action_enabled(AppAction::CreateMask).is_ok());
        perform_with_modifiers(&mut app, action, Modifiers::NONE);
        assert!(app.session.get_image(id).unwrap().threshold.is_none());
        assert!(app.action_enabled(AppAction::CreateMask).is_err());
    }
}
