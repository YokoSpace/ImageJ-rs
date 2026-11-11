//! Model and headless-egui integration tests for Plugins > Installed Plugins.
//!
//! These tests do not automate OS-level menu clicks. They drive the same
//! `ImageJApp` methods the menu/dialog callbacks call, and run real egui frames
//! with `Context::default()` to exercise the browser viewport.
//!
//! Discovery is read-only. Explicit inspection starts the real plugin host, so
//! these tests also check that the `InspectionProbeFilter` static initializer,
//! `setup()` and `run()` marker files never appear.

use egui::{Context, Pos2, RawInput, Rect, Vec2};
use ij_app::app::{ImageJApp, bounded_plugin_scan_options};
use ij_app::dialogs::{PluginCatalogPhase, PluginInspectPhase};
use ij_bridge::PluginBridge;
use ij_bridge::plugin_discovery::PluginKind;
use ij_core::{ImageDocument, PixelPlane};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const PROBE_MARKERS: [&str; 3] = [
    "inspection-static-initializer.marker",
    "inspection-setup.marker",
    "inspection-run.marker",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("locate repo root")
        .to_path_buf()
}

fn fixture_jar_path() -> PathBuf {
    let _ = plugin_bridge();
    repo_root().join("tools/plugin-host/fixtures/fixtures.jar")
}

fn plugin_bridge() -> PluginBridge {
    PluginBridge::locate_from_repo_root(repo_root()).expect("locate plugin bridge")
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

/// Builds a plugins directory containing:
/// * the real fixture JAR (classes),
/// * a config-only JAR whose `plugins.config` points at those classes,
/// * a corrupt archive that must produce a warning.
fn build_plugins_dir() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    fs::copy(fixture_jar_path(), plugins.join("fixtures.jar")).unwrap();

    let config = b"Plugins, \"Probe Filter\", fixtures.InspectionProbeFilter(\"probe-arg\")\n\
Plugins, \"Simple Plugin\", fixtures.SimplePlugIn\n\
Plugins, \"Extended Filter\", fixtures.ExtendedProbeFilter\n\
Plugins, \"Ambiguous Probe\", fixtures.AmbiguousProbe\n\
Plugins, \"Plain Class\", fixtures.PlainClassProbe\n\
Plugins, \"Missing Class\", fixtures.DoesNotExist\n\
Plugins, \"Internal Filter\", ij.plugin.filter.GaussianBlur\n\
Plugins>Tools, \"Duplicate\", fixtures.DummyPlugInFilter\n\
Plugins>Tools, \"Duplicate\", fixtures.RectInvertFilter\n";
    write_zip(
        &plugins.join("Probe_Plugins.jar"),
        &[("plugins.config", config)],
    );
    fs::write(
        plugins.join("Broken_Plugins.jar"),
        b"this is not a zip file",
    )
    .unwrap();
    tmp
}

fn catalog(app: &ImageJApp) -> &ij_bridge::PluginCatalog {
    app.plugins_browser.catalog.as_ref().expect("catalog")
}

fn index_of(app: &ImageJApp, label: &str) -> usize {
    catalog(app)
        .plugins
        .iter()
        .position(|plugin| plugin.label == label)
        .unwrap_or_else(|| panic!("no entry labelled {label:?}"))
}

fn wait_for_scan(app: &mut ImageJApp, timeout: Duration) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        app.poll_plugin_catalog_job();
        if !app.is_plugin_catalog_busy() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("catalog scan did not finish within {timeout:?}");
}

fn wait_for_inspection(app: &mut ImageJApp, timeout: Duration) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        app.poll_plugin_catalog_job();
        if !app.is_plugin_catalog_busy() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("catalog inspection did not finish within {timeout:?}");
}

fn scan(app: &mut ImageJApp, dir: &Path) {
    app.open_plugins_browser();
    app.start_plugin_directory_scan(dir).expect("start scan");
    wait_for_scan(app, Duration::from_secs(20));
    assert_eq!(
        app.plugins_browser.phase,
        PluginCatalogPhase::Ready,
        "scan error: {:?}",
        app.plugins_browser.scan_error
    );
}

#[test]
fn rescan_clears_the_previous_catalog_before_a_new_directory_is_ready() {
    let first = build_plugins_dir();
    let second = tempfile::tempdir().unwrap();
    let mut app = ImageJApp::new();
    scan(&mut app, &first.path().join("plugins"));
    app.plugins_browser.selected = Some(0);

    app.start_plugin_directory_scan(second.path()).unwrap();
    assert_eq!(app.plugins_browser.phase, PluginCatalogPhase::Scanning);
    assert!(app.plugins_browser.catalog.is_none());
    assert_eq!(app.plugins_browser.selected, None);
    assert!(app.cancel_plugin_catalog_job());
    assert_eq!(app.plugins_browser.phase, PluginCatalogPhase::Idle);
    assert!(app.plugins_browser.catalog.is_none());
}

#[cfg(unix)]
#[test]
fn cancelling_inspection_reaps_the_child_before_quit_is_allowed() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = build_plugins_dir();
    let pid_file = tmp.path().join("inspection.pid");
    let runner = tmp.path().join("slow_inspection.sh");
    fs::write(
        &runner,
        format!(
            "#!/bin/sh\nset -eu\necho $$ > '{}'\nsleep 60\n",
            pid_file.display()
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&runner).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&runner, permissions).unwrap();

    let bridge = plugin_bridge().with_runner_script(&runner);
    let mut app = ImageJApp::new().with_plugin_bridge(bridge);
    scan(&mut app, &tmp.path().join("plugins"));
    let index = index_of(&app, "Probe Filter");
    app.inspect_plugin_entry(index).unwrap();
    assert!(wait_until(Duration::from_secs(5), || pid_file.is_file()));

    assert!(app.cancel_plugin_catalog_job());
    assert!(app.is_plugin_catalog_busy());
    assert_eq!(
        app.plugins_browser.inspect_phase,
        PluginInspectPhase::Cancelling { index }
    );
    assert!(!app.request_quit());
    assert!(wait_until(Duration::from_secs(5), || {
        app.poll_plugin_catalog_job();
        !app.is_plugin_catalog_busy()
    }));
    let pid: String = fs::read_to_string(&pid_file).unwrap();
    assert!(
        !std::process::Command::new("/bin/kill")
            .arg("-0")
            .arg(pid.trim())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    assert!(app.request_quit());
}

fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    predicate()
}

fn inspect(app: &mut ImageJApp, index: usize) {
    app.plugins_browser.selected = Some(index);
    app.inspect_selected_plugin_entry()
        .expect("start inspection");
    wait_for_inspection(app, Duration::from_secs(25));
    assert!(
        matches!(
            app.plugins_browser.inspect_phase,
            PluginInspectPhase::Done { index: done } if done == index
        ),
        "unexpected inspection phase: {:?}",
        app.plugins_browser.inspect_phase
    );
}

fn assert_probe_markers_absent(temp: &Path) {
    let cwd = std::env::current_dir().expect("cwd");
    let candidates = [repo_root(), cwd, temp.to_path_buf()];
    for base in candidates {
        for marker in PROBE_MARKERS {
            let path = base.join(marker);
            assert!(
                !path.exists(),
                "inspection must not initialize or run the class, but {path:?} exists"
            );
        }
    }
}

#[test]
fn scan_lists_entries_as_uninspected_marks_duplicates_and_reports_warnings() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());

    // The visible Plugins menu action opens an independent browser window.
    assert!(app.open_plugins_browser());
    assert!(app.plugins_browser.show);

    app.start_plugin_directory_scan(&plugins_dir)
        .expect("start scan");
    // The scan is performed on a worker thread; the call itself returns
    // immediately with a pending job.
    assert!(app.is_plugin_catalog_busy());

    // A frame rendered while the worker is still pending must not block; it
    // shows the busy state and requests another repaint.
    let busy_ctx = Context::default();
    let busy_raw = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))),
        ..Default::default()
    };
    let _ = busy_ctx.run(busy_raw, |ctx| app.update_ui(ctx));

    wait_for_scan(&mut app, Duration::from_secs(20));

    assert_eq!(app.plugins_browser.phase, PluginCatalogPhase::Ready);
    assert!(!app.is_plugin_catalog_busy());
    assert_eq!(app.plugins_browser.selected, None);

    let catalog = catalog(&app);
    assert_eq!(catalog.plugins.len(), 9);
    assert_eq!(catalog.plugins_dir, plugins_dir);
    assert!(
        catalog.warnings.iter().any(|warning| matches!(
            warning,
            ij_bridge::plugin_discovery::DiscoveryWarning::ArchiveUnreadable { .. }
        )),
        "corrupt archive must be reported: {:#?}",
        catalog.warnings
    );

    // Scanning never inspects: no class was loaded.
    for plugin in &catalog.plugins {
        assert_eq!(plugin.kind, PluginKind::Uninspected);
        assert!(plugin.inspection.is_none());
    }

    // Both duplicate rows stay visible and are marked; ImageJ only flags the
    // later one, so the UI derives conflicts from all labels.
    let duplicate_indices: Vec<usize> = catalog
        .plugins
        .iter()
        .enumerate()
        .filter(|(_, plugin)| plugin.label == "Duplicate")
        .map(|(index, _)| index)
        .collect();
    assert_eq!(duplicate_indices.len(), 2);
    assert!(!catalog.plugins[duplicate_indices[0]].duplicate);
    assert!(catalog.plugins[duplicate_indices[1]].duplicate);
    for index in duplicate_indices {
        let row = ij_app::dialogs::plugin_row_text(
            &catalog.plugins[index],
            true,
            Some(index) == app.plugins_browser.selected,
        );
        assert!(row.contains("[duplicate label]"), "row was {row:?}");
    }

    // Bounded options are actually used for interactive scans.
    let options = bounded_plugin_scan_options();
    assert!(options.max_plugins <= 2_000);
    assert!(options.max_warnings <= 500);
    assert!(options.max_archive_bytes <= 64 * 1024 * 1024);

    // Drive one real egui frame with the browser open; headless embedded
    // viewports must not panic and the window stays open.
    let ctx = Context::default();
    let raw = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))),
        ..Default::default()
    };
    let _ = ctx.run(raw, |ctx| app.update_ui(ctx));
    assert!(app.plugins_browser.show);
    assert_probe_markers_absent(tmp.path());
}

#[test]
fn explicit_inspection_is_bounded_and_never_initializes_or_runs_the_class() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    scan(&mut app, &plugins_dir);

    let probe = index_of(&app, "Probe Filter");
    assert_eq!(catalog(&app).plugins[probe].kind, PluginKind::Uninspected);
    inspect(&mut app, probe);

    let entry = &catalog(&app).plugins[probe];
    assert_eq!(entry.kind, PluginKind::PlugInFilter);
    let inspection = entry.inspection.as_ref().expect("inspection detail");
    assert_eq!(inspection.class_name, "fixtures.InspectionProbeFilter");
    // The class-spec argument survives for prefill, but was stripped from the
    // name passed to Class.forName.
    assert_eq!(
        ij_app::dialogs::split_plugin_class_spec(&entry.class_spec),
        (
            "fixtures.InspectionProbeFilter".to_string(),
            Some("probe-arg".to_string())
        )
    );

    // `Class.forName(name, false, loader)` loads/links but must not initialize;
    // inspection must not instantiate or call setup()/run().
    assert_probe_markers_absent(tmp.path());

    // Inspection is per-entry and does not silently inspect the rest.
    assert_eq!(
        catalog(&app)
            .plugins
            .iter()
            .filter(|plugin| plugin.kind != PluginKind::Uninspected)
            .count(),
        1
    );
}

#[test]
fn uninspected_and_unsupported_entries_cannot_prefill_or_run() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    let id = app.open_ramp_8bit("browser-target", 24, 20).unwrap();
    app.set_dirty(id, false);
    scan(&mut app, &plugins_dir);

    let probe = index_of(&app, "Probe Filter");
    // Uninspected is never treated as runnable.
    let reason = app.plugin_prefill_block_reason(probe).expect("blocked");
    assert!(reason.contains("Uninspected"), "reason was {reason}");
    assert!(app.prefill_plugin_filter_from_catalog(probe).is_err());
    assert!(!app.dialogs.show_plugin_filter);
    assert_eq!(app.dialogs.plugin_filter_target_id, None);

    // Each unsupported classification remains explicitly disabled.
    let unsupported = [
        ("Simple Plugin", PluginKind::PlugIn),
        ("Extended Filter", PluginKind::ExtendedPlugInFilter),
        ("Ambiguous Probe", PluginKind::Ambiguous),
        ("Plain Class", PluginKind::NotAPlugin),
        ("Missing Class", PluginKind::LoadFailed),
        ("Internal Filter", PluginKind::Internal),
    ];
    for (label, expected_kind) in unsupported {
        let index = index_of(&app, label);
        inspect(&mut app, index);
        let entry = &catalog(&app).plugins[index];
        assert_eq!(entry.kind, expected_kind, "entry {label}");
        assert!(!entry.kind.is_supported_filter());

        let reason = app
            .plugin_prefill_block_reason(index)
            .unwrap_or_else(|| panic!("{label} must be blocked"));
        assert!(
            reason.contains("verified PlugInFilter") || reason.contains("not compatible"),
            "{label} reason was {reason}"
        );
        assert!(
            app.prefill_plugin_filter_from_catalog(index).is_err(),
            "{label} must not prefill"
        );
        assert!(!app.dialogs.show_plugin_filter);
        assert_eq!(app.dialogs.plugin_filter_target_id, None);
    }

    // No accidental execution and no dialog target were introduced.
    assert!(!app.is_plugin_filter_running());
    assert!(app.dialogs.plugin_filter_class.is_empty());
    assert_eq!(app.active_image_id(), Some(id));
    assert_probe_markers_absent(tmp.path());
}

#[test]
fn verified_filter_prefills_dialog_and_pins_the_displayed_target() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    let id1 = app.open_ramp_8bit("first-target", 24, 20).unwrap();
    let id2 = app.open_ramp_8bit("second-target", 18, 14).unwrap();
    app.set_dirty(id1, false);
    app.set_dirty(id2, false);

    // Activate the older image deliberately.
    app.set_active_window(id1).unwrap();
    scan(&mut app, &plugins_dir);
    let probe = index_of(&app, "Probe Filter");
    inspect(&mut app, probe);
    assert!(app.plugin_prefill_block_reason(probe).is_none());

    app.prefill_plugin_filter_from_catalog(probe)
        .expect("verified PlugInFilter may prefill");
    assert!(app.dialogs.show_plugin_filter);
    assert_eq!(
        app.dialogs.plugin_filter_class,
        "fixtures.InspectionProbeFilter"
    );
    assert_eq!(app.dialogs.plugin_filter_arg, "probe-arg");
    assert_eq!(app.dialogs.plugin_filter_target_id, Some(id1));
    assert!(app.dialogs.plugin_filter_classpath.contains(&plugins_dir));
    assert!(
        app.dialogs
            .plugin_filter_classpath
            .iter()
            .any(|path| path.ends_with("fixtures.jar"))
    );

    // Switching the active window must not retarget the pinned dialog.
    app.set_active_window(id2).unwrap();
    assert_eq!(app.dialogs.plugin_filter_target_id, Some(id1));
    // The prefill gate itself always evaluates the image active *now*.
    assert!(
        app.plugin_prefill_block_reason(probe)
            .is_none_or(|reason| !reason.contains("no active"))
    );

    // Closing the pinned window clears the pin rather than leaving a stale ID.
    assert!(app.close_window(id1));
    assert_eq!(app.dialogs.plugin_filter_target_id, None);
    assert_eq!(app.active_image_id(), Some(id2));

    // A dialog left pointing at a document the session no longer has is
    // rejected explicitly, never silently retargeted.
    app.dialogs.plugin_filter_target_id = Some(id1);
    app.dialogs.plugin_filter_class = "fixtures.RectInvertFilter".to_string();
    match app.start_plugin_filter_from_dialog() {
        Err(ij_session::SessionError::InvalidPluginFilterRequest(message)) => {
            assert!(message.contains("no longer open"), "message was {message}");
        }
        other => panic!("expected stale-target rejection, got {other:?}"),
    }
    assert!(!app.is_plugin_filter_running());
}

#[test]
fn prefill_uses_current_document_preflight_and_refuses_incompatible_images() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    scan(&mut app, &plugins_dir);
    let probe = index_of(&app, "Probe Filter");
    inspect(&mut app, probe);

    // Since P3.10 the image preflight accepts transferable 16-bit images;
    // the plugin's own depth support is checked by its setup() flags in the
    // Java host, so prefill must not be blocked for the image type.
    let gray16 =
        ImageDocument::new("sixteen-bit", 16, 16, PixelPlane::Gray16(vec![0; 16 * 16])).unwrap();
    let id16 = app.session.insert_doc(gray16);
    app.set_active_window(id16).unwrap();
    assert!(
        app.plugin_prefill_block_reason(probe).is_none(),
        "16-bit prefill must be allowed"
    );
    app.prefill_plugin_filter_from_catalog(probe)
        .expect("16-bit prefill only fills the dialog");
    assert!(app.dialogs.show_plugin_filter);
    app.dismiss_plugin_filter_dialog();

    // A document outside the transferable subset is still blocked up front.
    let mut bad_range = ImageDocument::new_ramp_8bit("bad-range", 16, 16).unwrap();
    bad_range.display_range = ij_core::DisplayRange {
        min: 5.0,
        max: 200.0,
    };
    let id_bad = app.session.insert_doc(bad_range);
    app.set_active_window(id_bad).unwrap();
    let reason = app
        .plugin_prefill_block_reason(probe)
        .expect("a non-transferable image must be blocked");
    assert!(reason.contains("not compatible"), "reason was {reason}");
    assert!(app.prefill_plugin_filter_from_catalog(probe).is_err());
    assert!(!app.dialogs.show_plugin_filter);
    app.session.close_image(id_bad);

    // Switching to a compatible 8-bit image unblocks prefill.
    let id8 = app.open_ramp_8bit("eight-bit", 20, 20).unwrap();
    app.set_dirty(id8, false);
    assert!(app.plugin_prefill_block_reason(probe).is_none());
    app.prefill_plugin_filter_from_catalog(probe)
        .expect("8-bit prefill");
    assert_eq!(app.dialogs.plugin_filter_target_id, Some(id8));
    let _ = id16;
}

#[test]
fn scan_errors_cancel_and_close_quit_guards_are_explicit() {
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());

    // Missing directory.
    let missing = tempfile::tempdir().unwrap().path().join("does-not-exist");
    app.start_plugin_directory_scan(&missing)
        .expect("start scan");
    wait_for_scan(&mut app, Duration::from_secs(20));
    assert_eq!(app.plugins_browser.phase, PluginCatalogPhase::Failed);
    assert_eq!(app.plugins_browser.catalog, None);
    assert!(
        app.plugins_browser
            .scan_error
            .as_deref()
            .is_some_and(|error| error.contains("failed to read"))
    );
    assert!(!app.is_plugin_catalog_busy());

    // A regular file is not a directory.
    let file = tempfile::NamedTempFile::new().unwrap();
    app.start_plugin_directory_scan(file.path())
        .expect("start scan");
    wait_for_scan(&mut app, Duration::from_secs(20));
    assert_eq!(app.plugins_browser.phase, PluginCatalogPhase::Failed);
    assert!(
        app.plugins_browser
            .scan_error
            .as_deref()
            .is_some_and(|error| error.contains("not a directory"))
    );

    // Empty input is rejected before a worker starts.
    app.plugins_browser.directory_input = "   ".into();
    assert!(app.start_plugin_directory_scan_from_input().is_err());
    assert!(!app.is_plugin_catalog_busy());

    // Cancellation abandons the worker and rejects its late result.
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    app.start_plugin_directory_scan(&plugins_dir)
        .expect("start scan");
    assert!(app.is_plugin_catalog_busy());
    assert!(app.cancel_plugin_catalog_job());
    assert!(!app.is_plugin_catalog_busy());
    assert_eq!(app.plugins_browser.phase, PluginCatalogPhase::Idle);
    // Polling after cancellation must not resurrect the old job.
    assert!(!app.poll_plugin_catalog_job());
    assert_eq!(app.plugins_browser.catalog, None);

    // A close confirmation blocks opening and scanning.
    let id = app.open_ramp_8bit("dirty", 8, 8).unwrap();
    app.set_dirty(id, true);
    assert!(!app.request_close_all());
    assert!(app.close_prompt_active());
    assert!(!app.open_plugins_browser());
    assert!(
        app.start_plugin_directory_scan(&plugins_dir)
            .unwrap_err()
            .contains("close/quit")
    );
    app.confirm_pending_close_cancel();
    assert!(app.close_window(id));
    assert!(!app.close_prompt_active());

    // Quitting cancels an in-flight catalog job.
    app.start_plugin_directory_scan(&plugins_dir)
        .expect("start scan");
    assert!(app.is_plugin_catalog_busy());
    assert!(app.request_quit());
    assert!(!app.is_plugin_catalog_busy());
}

#[test]
fn closing_the_browser_cancels_pending_work_and_releases_selection() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    assert!(app.open_plugins_browser());
    app.start_plugin_directory_scan(&plugins_dir)
        .expect("start scan");
    app.plugins_browser.selected = Some(3);
    assert!(app.is_plugin_catalog_busy());

    app.close_plugins_browser();
    assert!(!app.plugins_browser.show);
    assert!(!app.is_plugin_catalog_busy());
    assert_eq!(app.plugins_browser.selected, None);
    // Closing during a scan must not publish a partial/late catalog.
    assert!(!app.poll_plugin_catalog_job());
    assert_eq!(app.plugins_browser.catalog, None);
}

#[test]
fn duplicate_rows_select_by_index_so_inspection_does_not_leak_between_them() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    scan(&mut app, &plugins_dir);

    let duplicates: Vec<usize> = catalog(&app)
        .plugins
        .iter()
        .enumerate()
        .filter(|(_, plugin)| plugin.label == "Duplicate")
        .map(|(index, _)| index)
        .collect();
    assert_eq!(duplicates.len(), 2);

    // Inspect only the second duplicate. The first keeps its own state even
    // though both rows render the same label.
    inspect(&mut app, duplicates[1]);
    assert_eq!(
        catalog(&app).plugins[duplicates[1]].kind,
        PluginKind::PlugInFilter
    );
    assert_eq!(
        catalog(&app).plugins[duplicates[0]].kind,
        PluginKind::Uninspected
    );
    assert_eq!(app.plugins_browser.selected, Some(duplicates[1]));

    // The verified second row can prefill; the uninspected first cannot.
    let id = app.open_ramp_8bit("dup-target", 12, 12).unwrap();
    app.set_dirty(id, false);
    assert!(
        app.prefill_plugin_filter_from_catalog(duplicates[0])
            .is_err()
    );
    app.prefill_plugin_filter_from_catalog(duplicates[1])
        .expect("second duplicate is verified");
    assert_eq!(app.dialogs.plugin_filter_target_id, Some(id));
    assert_eq!(app.dialogs.plugin_filter_class, "fixtures.RectInvertFilter");
}
