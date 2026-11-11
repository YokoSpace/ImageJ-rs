use ij_app::action::{AppAction, MENU_TREE, MenuKind, MenuNode, NativeMenuSpec, NativeShortcut};
use ij_app::app::ImageJApp;
use ij_session::Command;

fn node<'a>(spec: &'a [NativeMenuSpec], label: &str) -> &'a NativeMenuSpec {
    spec.iter()
        .find(|node| node.label == label)
        .unwrap_or_else(|| panic!("missing native menu entry {label:?}"))
}

fn children<'a>(spec: &'a [NativeMenuSpec], label: &str) -> &'a [NativeMenuSpec] {
    &node(spec, label).children
}

fn find_action<'a>(nodes: &'a [NativeMenuSpec], action: AppAction) -> Option<&'a NativeMenuSpec> {
    for node in nodes {
        if node.action == Some(action) {
            return Some(node);
        }
        if let Some(found) = find_action(&node.children, action) {
            return Some(found);
        }
    }
    None
}

fn action_node<'a>(nodes: &'a [NativeMenuSpec], action: AppAction) -> &'a NativeMenuSpec {
    find_action(nodes, action).unwrap_or_else(|| panic!("missing native action {action:?}"))
}

fn flatten<'a>(nodes: &'a [NativeMenuSpec], out: &mut Vec<&'a NativeMenuSpec>) {
    for node in nodes {
        out.push(node);
        flatten(&node.children, out);
    }
}

/// Mirrors the menu table the way `WindowList` is rendered: one
/// `ActivateImage(id)` leaf per open image, at the list's position.
fn collect_menu_actions(nodes: &[MenuNode], app: &ImageJApp, out: &mut Vec<AppAction>) {
    for node in nodes {
        match node.kind {
            MenuKind::Item(action) => out.push(action),
            MenuKind::Submenu(children) => collect_menu_actions(children, app, out),
            MenuKind::WindowList => {
                for id in app.session.image_ids() {
                    out.push(AppAction::ActivateImage(id));
                }
            }
            MenuKind::Separator | MenuKind::Unsupported => {}
        }
    }
}

fn collect_spec_actions(nodes: &[NativeMenuSpec], out: &mut Vec<AppAction>) {
    for node in nodes {
        if let Some(action) = node.action {
            out.push(action);
        }
        collect_spec_actions(&node.children, out);
    }
}

#[test]
fn native_spec_preserves_the_classic_top_level_order() {
    let app = ImageJApp::new();
    let spec = app.build_native_menu_spec();

    let labels: Vec<_> = spec.iter().map(|node| node.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "File", "Edit", "Image", "Process", "Analyze", "Plugins", "Window", "Help"
        ]
    );
    for menu in &spec {
        assert!(menu.enabled, "{:?} must stay openable", menu.label);
        assert_eq!(menu.action, None);
        assert_eq!(menu.shortcut, None);
        assert!(!menu.is_separator);
        assert!(!menu.checked);
        assert!(!menu.children.is_empty(), "{:?} needs children", menu.label);
    }
}

#[test]
fn native_spec_disables_image_actions_without_an_image() {
    let app = ImageJApp::new();
    let spec = app.build_native_menu_spec();

    for action in [
        AppAction::NewRamp,
        AppAction::OpenTiff,
        AppAction::Quit,
        AppAction::InstalledPlugins,
        AppAction::ToggleResults,
        AppAction::About,
        AppAction::SetMeasurements,
    ] {
        assert!(
            action_node(&spec, action).enabled,
            "{action:?} must stay enabled without an image"
        );
    }

    for action in [
        AppAction::Save,
        AppAction::SaveAs,
        AppAction::SaveResults,
        AppAction::Close,
        AppAction::CloseAll,
        AppAction::Undo,
        AppAction::Invert,
        AppAction::Duplicate,
        AppAction::Crop,
        AppAction::ConvertTo8Bit,
        AppAction::ConvertTo16Bit,
        AppAction::ConvertTo32Bit,
        AppAction::ConvertToRgb,
        AppAction::SelectNone,
        AppAction::BrightnessContrast,
        AppAction::Threshold,
        AppAction::InvertStack,
        AppAction::Smooth,
        AppAction::Sharpen,
        AppAction::FindEdges,
        AppAction::GaussianBlur,
        AppAction::Median,
        AppAction::Minimum,
        AppAction::Maximum,
        AppAction::Despeckle,
        AppAction::Measure,
        AppAction::Histogram,
        AppAction::PlotProfile,
        AppAction::ClearResults,
        AppAction::RunPluginFilter,
    ] {
        assert!(
            !action_node(&spec, action).enabled,
            "{action:?} must be disabled without an image"
        );
    }
}

#[test]
fn native_window_menu_lists_open_images_and_tracks_the_active_window() {
    let mut app = ImageJApp::new();
    let first = app.open_ramp_8bit("first", 4, 4).unwrap();
    let second = app.open_ramp_8bit("second", 4, 4).unwrap();

    let spec = app.build_native_menu_spec();
    let window = children(&spec, "Window");
    assert_eq!(window[0].label, "Results");
    assert_eq!(window[0].action, Some(AppAction::ToggleResults));
    assert!(window[1].is_separator, "Window list follows its separator");

    let entries: Vec<&NativeMenuSpec> = window[2..]
        .iter()
        .filter(|node| matches!(node.action, Some(AppAction::ActivateImage(_))))
        .collect();
    assert_eq!(
        entries
            .iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let expected_actions: Vec<_> = app
        .session
        .image_ids()
        .into_iter()
        .map(|id| Some(AppAction::ActivateImage(id)))
        .collect();
    assert_eq!(
        entries.iter().map(|node| node.action).collect::<Vec<_>>(),
        expected_actions
    );
    for entry in &entries {
        assert!(entry.enabled);
        assert!(!entry.is_separator);
        assert!(entry.children.is_empty());
        assert_eq!(entry.shortcut, None);
    }
    let active = app.active_image_id().unwrap();
    assert!(
        active == first || active == second,
        "newly opened images become active"
    );
    for entry in &entries {
        assert_eq!(
            entry.checked,
            entry.action == Some(AppAction::ActivateImage(active))
        );
    }
    assert_eq!(entries.iter().filter(|node| node.checked).count(), 1);

    app.set_active_window(first).unwrap();
    let spec = app.build_native_menu_spec();
    let window = children(&spec, "Window");
    let entries: Vec<&NativeMenuSpec> = window
        .iter()
        .filter(|node| matches!(node.action, Some(AppAction::ActivateImage(_))))
        .collect();
    for entry in &entries {
        assert_eq!(
            entry.checked,
            entry.action == Some(AppAction::ActivateImage(first))
        );
    }
    assert_eq!(entries.iter().filter(|node| node.checked).count(), 1);
}

#[test]
fn native_crop_enablement_tracks_the_active_roi() {
    let mut app = ImageJApp::new();
    let _ = app.open_ramp_8bit("crop", 8, 6).unwrap();

    let spec = app.build_native_menu_spec();
    assert!(!action_node(&spec, AppAction::Crop).enabled);

    app.session
        .dispatch(Command::MakeLineRoi {
            x1: 1,
            y1: 1,
            x2: 5,
            y2: 4,
        })
        .unwrap();
    let spec = app.build_native_menu_spec();
    assert!(
        !action_node(&spec, AppAction::Crop).enabled,
        "a line selection is not an area"
    );

    app.session
        .dispatch(Command::MakeRectRoi {
            x: 2,
            y: 1,
            width: 4,
            height: 3,
        })
        .unwrap();
    let spec = app.build_native_menu_spec();
    assert!(action_node(&spec, AppAction::Crop).enabled);
}

#[test]
fn native_shortcuts_only_come_from_cmd_chords() {
    let mut app = ImageJApp::new();
    let _ = app.open_ramp_8bit("shortcuts", 4, 4).unwrap();
    let spec = app.build_native_menu_spec();

    assert_eq!(
        action_node(&spec, AppAction::Save).shortcut,
        Some(NativeShortcut {
            key: "S".to_string(),
            shift: false,
        })
    );
    assert_eq!(
        action_node(&spec, AppAction::Quit).shortcut,
        Some(NativeShortcut {
            key: "Q".to_string(),
            shift: false,
        })
    );
    assert_eq!(
        action_node(&spec, AppAction::SelectAll).shortcut,
        Some(NativeShortcut {
            key: "A".to_string(),
            shift: false,
        })
    );

    for action in [
        AppAction::Close,
        AppAction::CloseAll,
        AppAction::Undo,
        AppAction::Invert,
        AppAction::Duplicate,
        AppAction::Crop,
        AppAction::SelectNone,
        AppAction::Measure,
        AppAction::Histogram,
        AppAction::PlotProfile,
    ] {
        assert_eq!(
            action_node(&spec, action).shortcut,
            None,
            "{action:?} is a bare key or Shift-only chord and must stay with shortcuts.rs"
        );
    }

    let mut nodes = Vec::new();
    flatten(&spec, &mut nodes);
    for node in nodes {
        if let Some(shortcut) = &node.shortcut {
            assert!(!shortcut.shift, "{:?} unexpectedly uses Shift", node.label);
            assert!(
                matches!(shortcut.key.as_str(), "A" | "S" | "Q"),
                "{:?} unexpectedly maps {:?}",
                node.label,
                shortcut.key
            );
        }
    }
}

#[test]
fn native_spec_preserves_unsupported_entries_and_separators() {
    let app = ImageJApp::new();
    let spec = app.build_native_menu_spec();
    let edit = children(&spec, "Edit");

    assert_eq!(edit[0].label, "Undo");
    assert!(edit[1].is_separator);
    let cut = node(edit, "Cut");
    assert!(!cut.enabled);
    assert_eq!(cut.action, None);
    assert_eq!(cut.shortcut, None);
    assert!(!cut.is_separator);
    assert!(cut.children.is_empty());

    let mut nodes = Vec::new();
    flatten(&spec, &mut nodes);
    for separator in nodes.iter().filter(|node| node.is_separator) {
        assert_eq!(separator.label, "");
        assert_eq!(separator.action, None);
        assert_eq!(separator.shortcut, None);
        assert!(!separator.enabled);
        assert!(!separator.checked);
        assert!(separator.children.is_empty());
    }
}

#[test]
fn native_spec_covers_every_supported_leaf_action_in_menu_order() {
    let mut app = ImageJApp::new();
    let _ = app.open_ramp_8bit("one", 2, 2).unwrap();
    let _ = app.open_ramp_8bit("two", 2, 2).unwrap();
    let spec = app.build_native_menu_spec();

    let mut expected = Vec::new();
    collect_menu_actions(MENU_TREE, &app, &mut expected);
    let mut actual = Vec::new();
    collect_spec_actions(&spec, &mut actual);
    assert_eq!(actual, expected);

    let mut nodes = Vec::new();
    flatten(&spec, &mut nodes);
    for node in nodes {
        if node.action.is_some() {
            assert!(
                !node.is_separator,
                "{:?} is both item and separator",
                node.label
            );
            assert!(
                node.children.is_empty(),
                "{:?} mixes item and submenu",
                node.label
            );
        }
    }
}

#[test]
fn building_the_native_spec_does_not_touch_session_or_macro_state() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("stable", 4, 4).unwrap();
    app.session
        .dispatch(Command::MakeRectRoi {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        })
        .unwrap();
    app.perform(AppAction::Invert, &egui::Context::default());

    let macro_before = app.session.macro_recorder.clone();
    let active_before = app.active_image_id();
    let ids_before = app.session.image_ids();
    let rows_before = app.session.results.rows.len();
    let status_before = app.status_message.clone();
    let dirty_before = app.is_dirty(id);
    let pixels_before = match app.session.get_image(id).unwrap().plane() {
        ij_core::PixelPlane::Gray8(pixels) => pixels.clone(),
        plane => panic!("expected 8-bit ramp, got {plane:?}"),
    };

    let first = app.build_native_menu_spec();
    let second = app.build_native_menu_spec();
    assert_eq!(first, second, "spec building is deterministic");

    assert_eq!(app.session.macro_recorder, macro_before);
    assert_eq!(app.active_image_id(), active_before);
    assert_eq!(app.session.image_ids(), ids_before);
    assert_eq!(app.session.results.rows.len(), rows_before);
    assert_eq!(app.status_message, status_before);
    assert_eq!(app.is_dirty(id), dirty_before);
    match app.session.get_image(id).unwrap().plane() {
        ij_core::PixelPlane::Gray8(pixels) => assert_eq!(pixels, &pixels_before),
        plane => panic!("expected 8-bit ramp, got {plane:?}"),
    }
}
#[test]
fn native_action_queue_dispatches_through_the_app() {
    let mut app = ij_app::ImageJApp::new();
    let ctx = egui::Context::default();
    ij_app::macos_bridge::native_menu::queue_action(ij_app::action::AppAction::NewRamp, &ctx);
    app.dispatch_native_menu_actions(&ctx);
    assert_eq!(app.open_window_count(), 1);
    app.dispatch_native_menu_actions(&ctx);
    assert_eq!(app.open_window_count(), 1);
}

#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn native_appkit_menu_and_unsaved_quit_smoke() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("native_menu_smoke.rs");
    let binary = temp.path().join("native_menu_smoke");
    std::fs::write(&source, NATIVE_MENU_SMOKE_SOURCE).unwrap();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    rustc
        .arg("--edition=2024")
        .arg("-L")
        .arg(format!("dependency={}", deps.display()));
    for name in ["ij_app", "eframe", "egui"] {
        let prefix = format!("lib{name}-");
        let lib = std::fs::read_dir(&deps)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_string_lossy().starts_with(&prefix)
                    && entry.path().extension().is_some_and(|ext| ext == "rlib")
            })
            .max_by_key(|entry| entry.metadata().unwrap().modified().unwrap())
            .unwrap();
        rustc
            .arg("--extern")
            .arg(format!("{name}={}", lib.path().display()));
    }
    let compile = rustc.arg(&source).arg("-o").arg(&binary).output().unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut child = Command::new(&binary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "native menu probe timed out: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("NATIVE_MENU_SMOKE_ALL_OK"), "{stdout}");
}

#[cfg(all(target_os = "macos", debug_assertions))]
const NATIVE_MENU_SMOKE_SOURCE: &str = r##"use ij_app::{ImageJApp, action::AppAction, macos_bridge::{self, native_menu}};
struct Smoke { app: ImageJApp, phase: u8, titles: Vec<String> }
impl eframe::App for Smoke {
 fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
  self.app.update_ui(ctx);
  match self.phase {
   0 => {
    self.titles = native_menu::probe_titles();
    assert!(macos_bridge::is_installed());
    assert_eq!(&self.titles[self.titles.len()-8..], ["File","Edit","Image","Process","Analyze","Plugins","Window","Help"]);
    assert!(self.titles.len() >= 9);
    assert_eq!(native_menu::probe_item(AppAction::Save), Some((false,"s".into())));
    assert_eq!(native_menu::probe_item(AppAction::Measure), Some((false,"".into())));
    assert_eq!(native_menu::probe_item(AppAction::Quit), Some((true,"".into())));
    assert!(native_menu::probe_activate(AppAction::NewRamp));
    println!("NATIVE_MENU_INSTALL_OK {:?}", self.titles);
   }
   1 => {
    assert_eq!(self.app.open_window_count(), 1);
    assert!(self.app.any_dirty());
    assert_eq!(native_menu::probe_item(AppAction::Save), Some((true,"s".into())));
    let id = self.app.active_image_id().unwrap();
    assert_eq!(native_menu::probe_item(AppAction::ActivateImage(id)), Some((true,"".into())));
    assert!(native_menu::probe_activate(AppAction::Quit));
    println!("NATIVE_MENU_CALLBACK_AND_STATE_OK");
   }
   2 => {
    assert!(self.app.pending_close.is_some());
    assert!(!self.app.quit_app);
    assert_eq!(native_menu::probe_item(AppAction::Save).unwrap().0, false);
    self.app.confirm_pending_close_cancel();
    println!("NATIVE_MENU_UNSAVED_QUIT_GUARD_OK");
   }
   3 => {
    assert!(self.app.pending_close.is_none());
    assert_eq!(native_menu::probe_item(AppAction::Save).unwrap().0, true);
    assert_eq!(native_menu::probe_titles(), self.titles);
    for _ in 0..100 { self.app.update_ui(ctx); }
    assert_eq!(native_menu::probe_titles(), self.titles);
    self.app.close_all_windows();
    self.app.request_quit();
    println!("NATIVE_MENU_SMOKE_ALL_OK");
   }
   _ => {}
  }
  self.phase += 1;
  ctx.request_repaint();
 }
}
fn main() -> eframe::Result<()> {
 eframe::run_native("ImageJ native menu probe", eframe::NativeOptions::default(), Box::new(|_| Ok(Box::new(Smoke {app:ImageJApp::new(),phase:0,titles:vec![]}))))
}
"##;
