use ij_app::action::{AppAction, MENU_TREE, MenuKind, MenuNode};
use ij_app::app::ImageJApp;
use ij_session::generated_commands::{COMMAND_SPECS, ExecutionTier};
use std::collections::BTreeSet;

const KNOWN_UNWIRED: &[&str] = &[];

#[test]
fn median_menu_accepts_supported_gray16_and_gray32_stacks() {
    for plane in [
        ij_core::PixelPlane::Gray16(vec![0, 0, 500, 0, 0, 0, 0, 0, 0]),
        ij_core::PixelPlane::Gray32(vec![0.0, 0.0, 500.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ] {
        let mut app = ImageJApp::new();
        app.session.insert_doc(ij_core::ImageDocument::new_stack(
            "median", 3, 3, vec![plane.clone(), plane],
        ).unwrap());
        assert!(app.action_enabled(AppAction::Median).is_ok());
        app.perform(AppAction::Median, &egui::Context::default());
        assert!(app.pending_stack_process.is_some());
        app.resolve_stack_process(ij_app::stack_process::StackProcessChoice::All).unwrap();
        assert!(app.session.active_doc().unwrap().snapshot.is_none());
    }
}

#[test]
fn cmd_s_saves_to_existing_path_and_clears_dirty_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.tif");
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("save", 8, 8).unwrap();
    app.save_active_tiff(path.clone()).unwrap();
    app.invert_active().unwrap();
    assert!(app.is_dirty(id));
    let modifiers = egui::Modifiers::COMMAND;
    let input = egui::RawInput {
        modifiers,
        events: vec![egui::Event::Key {
            key: egui::Key::S, physical_key: None, pressed: true,
            repeat: false, modifiers,
        }],
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let _ = ctx.run(input, |ctx| app.handle_image_shortcuts(ctx, id));
    assert!(!app.is_dirty(id));
    let saved = ij_io::load_tiff(&path).unwrap();
    assert_eq!(saved.slices, app.session.get_image(id).unwrap().slices);
}

#[test]
fn png_and_jpeg_save_as_actions_and_png_resave_are_wired() {
    let file = MENU_TREE.iter().find(|node| node.label == "File").unwrap();
    let MenuKind::Submenu(file_items) = file.kind else {
        panic!("File menu must be a submenu");
    };
    let save_as = file_items.iter().find(|node| node.label == "Save As").unwrap();
    let MenuKind::Submenu(formats) = save_as.kind else {
        panic!("Save As must be a submenu");
    };
    assert!(formats.iter().any(|node| {
        node.label == "PNG..." && matches!(node.kind, MenuKind::Item(AppAction::SaveAsPng))
    }));
    assert!(formats.iter().any(|node| {
        node.label == "Jpeg..." && matches!(node.kind, MenuKind::Item(AppAction::SaveAsJpeg))
    }));

    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("saved.png");
    let jpeg_path = dir.path().join("saved.jpg");
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("formats", 8, 8).unwrap();
    app.save_active_png(png_path.clone()).unwrap();
    assert!(std::fs::read(&png_path).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
    app.invert_active().unwrap();
    assert!(app.is_dirty(id));
    app.perform(AppAction::Save, &egui::Context::default());
    assert!(!app.is_dirty(id));
    assert_eq!(ij_io::load_image(&png_path).unwrap().slices, app.session.get_image(id).unwrap().slices);
    app.save_active_jpeg(jpeg_path.clone()).unwrap();
    assert!(std::fs::read(&jpeg_path).unwrap().starts_with(&[0xff, 0xd8, 0xff]));

    let mut other = ImageJApp::new();
    let opened = other.open_image_file(png_path).unwrap();
    assert!(other.image_views.contains_key(&opened));
}

#[test]
fn duplicate_and_profile_shortcuts_reach_the_target_image() {
    for (key, modifiers) in [
        (egui::Key::D, egui::Modifiers::SHIFT),
        (egui::Key::K, egui::Modifiers::NONE),
    ] {
        let mut app = ImageJApp::new();
        let id = app.open_ramp_8bit("shortcuts", 8, 8).unwrap();
        app.session.dispatch(ij_session::Command::MakeRectRoi {
            x: 1, y: 1, width: 4, height: 4,
        }).unwrap();
        let input = egui::RawInput {
            modifiers,
            events: vec![egui::Event::Key {
                key, physical_key: None, pressed: true, repeat: false, modifiers,
            }],
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let _ = ctx.run(input, |ctx| app.handle_image_shortcuts(ctx, id));
        if key == egui::Key::D {
            assert!(app.dialogs.duplicate.open);
            assert_eq!(app.dialogs.duplicate.source, Some(id));
        } else {
            assert!(app.plot_profile_window.is_some());
            assert_eq!(app.session.macro_recorder.last().map(String::as_str), Some("run(\"Plot Profile\");"));
        }
    }
}

fn collect_wired(nodes: &[MenuNode], labels: &mut BTreeSet<&'static str>) {
    for node in nodes {
        match node.kind {
            MenuKind::Item(_) => {
                labels.insert(node.label);
            }
            MenuKind::Submenu(children) => collect_wired(children, labels),
            MenuKind::Separator | MenuKind::Unsupported | MenuKind::WindowList => {}
        }
    }
}

#[test]
fn image_menu_wires_duplicate_with_shift_d() {
    let image = MENU_TREE
        .iter()
        .find(|node| node.label == "Image")
        .expect("Image menu");
    let MenuKind::Submenu(nodes) = image.kind else {
        panic!("Image must be a submenu");
    };
    let duplicate = nodes
        .iter()
        .find(|node| node.label == "Duplicate...")
        .expect("Duplicate item");
    assert!(matches!(
        duplicate.kind,
        MenuKind::Item(AppAction::Duplicate)
    ));
    assert_eq!(duplicate.shortcut, Some("Shift+D"));
}

#[test]
fn tier_one_commands_have_menu_actions_or_an_explicit_gap() {
    let mut wired = BTreeSet::new();
    collect_wired(MENU_TREE, &mut wired);

    let missing: BTreeSet<_> = COMMAND_SPECS
        .iter()
        .filter(|spec| spec.tier == ExecutionTier::RustTier1)
        .map(|spec| spec.label)
        .filter(|label| !wired.contains(label))
        .collect();
    let known: BTreeSet<_> = KNOWN_UNWIRED.iter().copied().collect();
    assert_eq!(
        missing, known,
        "update KNOWN_UNWIRED when menu wiring changes"
    );
}

#[test]
fn results_import_and_export_keep_distinct_labels_targets_and_tiers() {
    // IJ_Props.txt 37/65 and SimpleCommands.java 23-24 distinguish import
    // from MeasurementsWriter's save(path) (MeasurementsWriter.java 12-16).
    let import = COMMAND_SPECS.iter().find(|spec| spec.prop_key == "import06").unwrap();
    let export = COMMAND_SPECS.iter().find(|spec| spec.prop_key == "save16").unwrap();
    assert_eq!(import.label, "Results... ");
    assert_eq!(import.java_class, "ij.plugin.SimpleCommands");
    assert_eq!(import.java_arg, "import");
    assert_eq!(import.tier, ExecutionTier::JavaBridge);
    assert_eq!(export.label, "Results...");
    assert_eq!(export.java_class, "ij.plugin.MeasurementsWriter");
    assert_eq!(export.tier, ExecutionTier::RustTier1);
}

fn file_menu_nodes() -> &'static [MenuNode] {
    MENU_TREE
        .iter()
        .find(|node| node.label == "File")
        .and_then(|node| match node.kind {
            MenuKind::Submenu(children) => Some(children),
            _ => None,
        })
        .expect("File menu")
}

#[test]
fn file_menu_matches_imagej_save_entries() {
    let nodes = file_menu_nodes();

    let save = nodes
        .iter()
        .find(|node| node.label == "Save")
        .expect("File > Save");
    assert_eq!(save.shortcut, Some("Cmd+S"));
    assert!(matches!(save.kind, MenuKind::Item(AppAction::Save)));

    let save_as = nodes
        .iter()
        .find(|node| node.label == "Save As")
        .expect("File > Save As");
    let MenuKind::Submenu(children) = save_as.kind else {
        panic!("Save As must be a submenu");
    };
    let labels: Vec<_> = children.iter().map(|node| node.label).collect();
    assert_eq!(labels, ["Tiff...", "Jpeg...", "PNG...", "Results..."]);
    assert!(matches!(
        children[0].kind,
        MenuKind::Item(AppAction::SaveAs)
    ));
    assert!(matches!(
        children[1].kind,
        MenuKind::Item(AppAction::SaveAsJpeg)
    ));
    assert!(matches!(
        children[2].kind,
        MenuKind::Item(AppAction::SaveAsPng)
    ));
    assert!(matches!(
        children[3].kind,
        MenuKind::Item(AppAction::SaveResults)
    ));
}

#[test]
fn save_actions_are_enabled_only_when_they_can_run() {
    let mut app = ImageJApp::new();
    assert!(app.action_enabled(AppAction::Save).is_err());
    assert!(app.action_enabled(AppAction::SaveAs).is_err());
    assert!(app.action_enabled(AppAction::SaveAsPng).is_err());
    assert!(app.action_enabled(AppAction::SaveAsJpeg).is_err());
    assert!(app.action_enabled(AppAction::SaveResults).is_err());

    app.open_ramp_8bit("save", 4, 4).unwrap();
    assert!(app.action_enabled(AppAction::Save).is_ok());
    assert!(app.action_enabled(AppAction::SaveAs).is_ok());
    assert!(app.action_enabled(AppAction::SaveAsPng).is_ok());
    assert!(app.action_enabled(AppAction::SaveAsJpeg).is_ok());
    // Save As > Results... needs a row to write.
    assert!(app.action_enabled(AppAction::SaveResults).is_err());
}
