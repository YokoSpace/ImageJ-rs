//! P1.1/P1.2: bounded JSON plugin-catalog cache and one-click Plugins menu runs.
//!
//! These tests use the real Java plugin host and the real fixture JAR. The
//! cache is a plain JSON document written to a temp path so the developer's
//! platform config directory is never touched.

use ij_app::app::{
    CloseTarget, ImageJApp, PendingClose, PluginCatalogCacheState, load_plugin_catalog_cache,
    save_plugin_catalog_cache,
};
use ij_app::dialogs::PluginCatalogPhase;
use ij_bridge::PluginBridge;
use ij_bridge::PluginCatalog;
use ij_bridge::plugin_discovery::PluginKind;
use ij_core::{ImageDocument, PixelPlane};
use ij_session::{ImageId, PluginFilterOutcome, SessionError};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn native_plugin_menu_runs_verified_filter_and_rejects_stale_catalog_actions() {
    use ij_app::action::AppAction;
    let tmp = build_plugins_dir();
    let plugins = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    app.set_plugin_catalog_cache_path(Some(tmp.path().join("cache.json")));
    let id = open_ramp(&mut app, "native-target", 8, 6);
    scan(&mut app, &plugins);
    let invert = index_of(&app, "Invert 8-bit");
    inspect(&mut app, invert);
    let tree = app.build_native_menu_spec();
    let menu = tree.iter().find(|node| node.label == "Plugins").unwrap();
    let node = menu.children.iter().find(|node| node.label == "Invert 8-bit").unwrap();
    assert!(node.enabled);
    let action = node.action.unwrap();
    assert!(matches!(action, AppAction::RunCatalogPlugin { index, .. } if index == invert));
    let blocked = menu.children.iter().find(|node| node.label == "Dummy Filter").unwrap();
    assert!(!blocked.enabled);
    assert!(blocked.disabled_reason.is_some());
    let before = gray8_pixels(&app, id);
    let ctx = egui::Context::default();
    ij_app::macos_bridge::native_menu::queue_action(action, &ctx);
    app.dispatch_native_menu_actions(&ctx);
    assert!(app.is_plugin_filter_running());
    wait_for_filter(&mut app, Duration::from_secs(20)).unwrap();
    let after = gray8_pixels(&app, id);
    assert!(before.iter().zip(&after).all(|(a,b)| *b == 255-*a));

    scan(&mut app, &plugins);
    assert!(app.action_enabled(action).is_err());
    app.perform(action, &ctx);
    assert!(!app.is_plugin_filter_running());
    assert_eq!(gray8_pixels(&app, id), after);
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
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

/// Builds a plugins directory with the real fixture JAR on the classpath and a
/// config-only JAR that exposes four entries. `fixtures.jar` deliberately has
/// no underscore so it is classpath-only and does not add auto-generated rows.
fn build_plugins_dir() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    fs::copy(fixture_jar_path(), plugins.join("fixtures.jar")).unwrap();

    let config = b"Plugins, \"Invert 8-bit\", fixtures.RectInvertFilter\n\
Plugins, \"Probe Filter\", fixtures.InspectionProbeFilter(\"probe-arg\")\n\
Plugins, \"Simple Plugin\", fixtures.SimplePlugIn\n\
Plugins, \"Dummy Filter\", fixtures.DummyPlugInFilter\n";
    write_zip(
        &plugins.join("Probe_Plugins.jar"),
        &[("plugins.config", config)],
    );
    tmp
}

fn catalog(app: &ImageJApp) -> &PluginCatalog {
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

fn inspect(app: &mut ImageJApp, index: usize) {
    app.inspect_plugin_entry(index).expect("start inspection");
    wait_for_inspection(app, Duration::from_secs(25));
}

fn open_ramp(app: &mut ImageJApp, title: &str, width: u32, height: u32) -> ImageId {
    let id = app.open_ramp_8bit(title, width, height).expect("open ramp");
    app.set_dirty(id, false);
    id
}

fn gray8_pixels(app: &ImageJApp, id: ImageId) -> Vec<u8> {
    match app.session.get_image(id).expect("image").plane() {
        PixelPlane::Gray8(pixels) => pixels.clone(),
        other => panic!("expected 8-bit grayscale plane, got {other:?}"),
    }
}

fn wait_for_filter(
    app: &mut ImageJApp,
    timeout: Duration,
) -> Result<PluginFilterOutcome, SessionError> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(result) = app.poll_plugin_filter() {
            return result;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("plugin filter did not finish within {timeout:?}");
}

// ---------------------------------------------------------------------------
// P1.1: bounded JSON catalog cache
// ---------------------------------------------------------------------------

#[test]
fn cache_roundtrip_preserves_entries_sources_classpath_and_inspection() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    scan(&mut app, &plugins_dir);

    let probe = index_of(&app, "Probe Filter");
    inspect(&mut app, probe);
    assert_eq!(catalog(&app).plugins[probe].kind, PluginKind::PlugInFilter);
    assert!(catalog(&app).plugins[probe].inspection.is_some());

    let cache_path = tmp.path().join("cache").join("plugin_catalog.json");
    save_plugin_catalog_cache(&cache_path, catalog(&app)).expect("save cache");

    let loaded = load_plugin_catalog_cache(&cache_path);
    assert_eq!(
        loaded.state,
        PluginCatalogCacheState::Loaded {
            entries: catalog(&app).plugins.len()
        }
    );
    let loaded_catalog = loaded.catalog.expect("catalog");
    assert_eq!(&loaded_catalog, catalog(&app));

    // Explicitly check the metadata P1 requires.
    assert_eq!(loaded_catalog.plugins_dir, plugins_dir);
    assert_eq!(loaded_catalog.classpath, catalog(&app).classpath);
    assert!(
        loaded_catalog
            .classpath
            .iter()
            .any(|path| path.ends_with("fixtures.jar"))
    );
    assert_eq!(
        loaded_catalog.plugins[probe].source,
        catalog(&app).plugins[probe].source
    );
    assert_eq!(
        loaded_catalog.plugins[probe].inspection,
        catalog(&app).plugins[probe].inspection
    );
}

#[test]
fn cache_missing_corrupt_unreadable_and_unsupported_schema_fall_back_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing.json");
    let missing_load = load_plugin_catalog_cache(&missing);
    assert_eq!(missing_load.state, PluginCatalogCacheState::Missing);
    assert!(missing_load.catalog.is_none());

    let corrupt = tmp.path().join("corrupt.json");
    fs::write(&corrupt, b"{ this is not json").unwrap();
    let corrupt_load = load_plugin_catalog_cache(&corrupt);
    assert!(
        matches!(corrupt_load.state, PluginCatalogCacheState::Corrupt { .. }),
        "state was {:?}",
        corrupt_load.state
    );
    assert!(corrupt_load.catalog.is_none());

    let unsupported = tmp.path().join("unsupported.json");
    fs::write(
        &unsupported,
        br#"{"schema_version":999,"catalog":{"plugins_dir":"/tmp","classpath":[],"plugins":[],"archives":[],"warnings":[],"warnings_truncated":false}}"#,
    )
    .unwrap();
    let unsupported_load = load_plugin_catalog_cache(&unsupported);
    assert_eq!(
        unsupported_load.state,
        PluginCatalogCacheState::UnsupportedSchema {
            found: 999,
            supported: 1
        }
    );
    assert!(unsupported_load.catalog.is_none());

    let unreadable = tmp.path().join("a-directory");
    fs::create_dir(&unreadable).unwrap();
    let unreadable_load = load_plugin_catalog_cache(&unreadable);
    assert!(
        matches!(
            unreadable_load.state,
            PluginCatalogCacheState::Unreadable { .. }
        ),
        "state was {:?}",
        unreadable_load.state
    );
    assert!(unreadable_load.catalog.is_none());

    // A broken cache must not break startup: the app keeps an explicit state
    // and simply has no catalog.
    let mut app = ImageJApp::new();
    app.set_plugin_catalog_cache_path(Some(corrupt.clone()));
    assert!(!app.load_plugin_catalog_cache_at_startup());
    assert!(matches!(
        app.plugin_catalog_cache_state,
        PluginCatalogCacheState::Corrupt { .. }
    ));
    assert!(app.plugins_browser.catalog.is_none());
}

#[test]
fn startup_autoload_restores_saved_inspection_and_scan_saves_after_completion() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let cache_path = tmp.path().join("cache").join("plugin_catalog.json");

    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    app.set_plugin_catalog_cache_path(Some(cache_path.clone()));
    scan(&mut app, &plugins_dir);

    // The scan itself is saved immediately after completion.
    assert!(cache_path.is_file(), "scan completion must save the cache");
    let after_scan = load_plugin_catalog_cache(&cache_path);
    assert!(after_scan.catalog.is_some());
    assert!(
        after_scan
            .catalog
            .as_ref()
            .unwrap()
            .plugins
            .iter()
            .all(|plugin| plugin.kind == PluginKind::Uninspected)
    );

    let probe = index_of(&app, "Probe Filter");
    inspect(&mut app, probe);
    let after_inspection = load_plugin_catalog_cache(&cache_path);
    let saved_catalog = after_inspection.catalog.expect("saved catalog");
    let saved_probe = saved_catalog
        .plugins
        .iter()
        .find(|plugin| plugin.label == "Probe Filter")
        .expect("probe entry");
    assert_eq!(saved_probe.kind, PluginKind::PlugInFilter);
    assert!(
        saved_probe.inspection.is_some(),
        "inspection completion must be saved"
    );

    // A fresh app auto-loads exactly once from the configured path.
    let mut restored = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    restored.set_plugin_catalog_cache_path(Some(cache_path.clone()));
    assert_eq!(
        restored.plugin_catalog_cache_state,
        PluginCatalogCacheState::NotLoaded
    );
    let id = open_ramp(&mut restored, "restored-target", 20, 16);
    assert!(restored.load_plugin_catalog_cache_at_startup());
    assert_eq!(
        restored.plugin_catalog_cache_state,
        PluginCatalogCacheState::Loaded {
            entries: saved_catalog.plugins.len()
        }
    );
    assert_eq!(restored.plugins_browser.phase, PluginCatalogPhase::Ready);
    assert_eq!(
        restored.plugins_browser.directory_input,
        plugins_dir.display().to_string()
    );
    let restored_probe = index_of(&restored, "Probe Filter");
    assert_eq!(
        catalog(&restored).plugins[restored_probe].inspection,
        saved_probe.inspection
    );
    assert_eq!(
        catalog(&restored).plugins[restored_probe].source,
        saved_probe.source
    );
    assert_eq!(catalog(&restored).classpath, saved_catalog.classpath);
    assert!(
        restored
            .plugin_prefill_block_reason(restored_probe)
            .is_none()
    );
    assert_eq!(restored.active_image_id(), Some(id));

    // Loading is once-only; a second call is a no-op.
    assert!(!restored.load_plugin_catalog_cache_at_startup());
}

#[test]
fn canceled_or_failed_jobs_are_not_saved_to_the_cache() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let cache_path = tmp.path().join("cache").join("plugin_catalog.json");

    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    app.set_plugin_catalog_cache_path(Some(cache_path.clone()));
    app.open_plugins_browser();
    app.start_plugin_directory_scan(&plugins_dir).expect("scan");
    assert!(app.cancel_plugin_catalog_job());
    wait_for_scan(&mut app, Duration::from_secs(20));
    assert!(
        !cache_path.exists(),
        "a canceled scan must not be restored as a completed cache"
    );

    let missing = tmp.path().join("does-not-exist");
    app.start_plugin_directory_scan(&missing).expect("scan");
    wait_for_scan(&mut app, Duration::from_secs(20));
    assert_eq!(app.plugins_browser.phase, PluginCatalogPhase::Failed);
    assert!(
        !cache_path.exists(),
        "a failed scan must not be saved to the cache"
    );
}

#[test]
fn reloaded_entries_with_missing_source_or_classpath_are_blocked_with_a_reason() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let cache_path = tmp.path().join("cache").join("plugin_catalog.json");

    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    app.set_plugin_catalog_cache_path(Some(cache_path.clone()));
    open_ramp(&mut app, "cache-source-target", 16, 16);
    scan(&mut app, &plugins_dir);
    let invert = index_of(&app, "Invert 8-bit");
    inspect(&mut app, invert);
    assert_eq!(catalog(&app).plugins[invert].kind, PluginKind::PlugInFilter);
    assert!(app.plugin_prefill_block_reason(invert).is_none());

    // Remove the source archive and its classpath entry after the cache was
    // written. A reloaded entry must not look executable.
    fs::remove_file(plugins_dir.join("Probe_Plugins.jar")).unwrap();
    fs::remove_file(plugins_dir.join("fixtures.jar")).unwrap();

    let mut restored = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    restored.set_plugin_catalog_cache_path(Some(cache_path));
    open_ramp(&mut restored, "stale-cache-target", 16, 16);
    assert!(restored.load_plugin_catalog_cache_at_startup());
    let stale = index_of(&restored, "Invert 8-bit");
    let reason = restored
        .plugin_prefill_block_reason(stale)
        .expect("stale source must be blocked");
    assert!(
        reason.contains("no longer exists") || reason.contains("Re-scan"),
        "reason was {reason}"
    );
    assert!(restored.prefill_plugin_filter_from_catalog(stale).is_err());
    assert!(!restored.dialogs.show_plugin_filter);
}

// ---------------------------------------------------------------------------
// P1.2: one-click Plugins menu execution
// ---------------------------------------------------------------------------

#[test]
fn invert_filter_runs_from_catalog_and_inverts_pixels() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let cache_path = tmp.path().join("cache").join("plugin_catalog.json");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    app.set_plugin_catalog_cache_path(Some(cache_path));
    let id = open_ramp(&mut app, "invert-target", 8, 6);
    scan(&mut app, &plugins_dir);

    let invert = index_of(&app, "Invert 8-bit");
    inspect(&mut app, invert);
    assert_eq!(catalog(&app).plugins[invert].kind, PluginKind::PlugInFilter);
    assert!(app.plugin_prefill_block_reason(invert).is_none());
    let before = gray8_pixels(&app, id);

    app.run_catalog_plugin_immediately(invert)
        .expect("one click must start the verified filter");

    // The run is asynchronous: the UI thread only starts the worker and never
    // waits for the JVM. The prefill dialog must not be left open.
    assert!(app.is_plugin_filter_running());
    assert!(
        !app.dialogs.show_plugin_filter,
        "one-click run must dismiss the prefill dialog"
    );
    assert_eq!(app.dialogs.plugin_filter_target_id, None);

    let outcome = wait_for_filter(&mut app, Duration::from_secs(20)).expect("filter completes");
    assert_eq!(outcome.class_name, "fixtures.RectInvertFilter");
    let after = gray8_pixels(&app, id);
    assert_eq!(after.len(), before.len());
    for (index, (before, after)) in before.iter().zip(after.iter()).enumerate() {
        assert_eq!(*after, 255 - *before, "pixel {index} should be inverted");
    }
    assert!(app.is_dirty(id));
}

#[test]
fn gray16_blocks_catalog_run_with_a_concrete_reason() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    app.set_plugin_catalog_cache_path(Some(tmp.path().join("cache.json")));
    scan(&mut app, &plugins_dir);
    let invert = index_of(&app, "Invert 8-bit");
    inspect(&mut app, invert);

    let gray16 =
        ImageDocument::new("sixteen-bit", 8, 8, PixelPlane::Gray16(vec![0; 8 * 8])).unwrap();
    let id16 = app.session.insert_doc(gray16);
    app.set_active_window(id16).unwrap();

    let reason = app
        .plugin_prefill_block_reason(invert)
        .expect("16-bit must be blocked");
    assert!(reason.contains("not compatible"), "reason was {reason}");
    let error = app
        .run_catalog_plugin_immediately(invert)
        .expect_err("16-bit one-click run must be rejected");
    assert!(error.contains("not compatible"), "error was {error}");
    assert!(!app.is_plugin_filter_running());
    assert!(!app.dialogs.show_plugin_filter);
    assert_eq!(app.dialogs.plugin_filter_target_id, None);
}

#[test]
fn uninspected_incompatible_and_already_running_entries_are_rejected() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    app.set_plugin_catalog_cache_path(Some(tmp.path().join("cache.json")));
    open_ramp(&mut app, "reject-target", 8, 6);
    scan(&mut app, &plugins_dir);

    let uninspected = index_of(&app, "Dummy Filter");
    let error = app
        .run_catalog_plugin_immediately(uninspected)
        .expect_err("uninspected entry must be rejected");
    assert!(error.contains("Uninspected"), "error was {error}");
    assert!(!app.is_plugin_filter_running());
    assert!(!app.dialogs.show_plugin_filter);

    let incompatible = index_of(&app, "Simple Plugin");
    inspect(&mut app, incompatible);
    assert_eq!(catalog(&app).plugins[incompatible].kind, PluginKind::PlugIn);
    let error = app
        .run_catalog_plugin_immediately(incompatible)
        .expect_err("non-PlugInFilter entry must be rejected");
    assert!(error.contains("verified PlugInFilter"), "error was {error}");
    assert!(!app.is_plugin_filter_running());

    let invert = index_of(&app, "Invert 8-bit");
    inspect(&mut app, invert);
    app.run_catalog_plugin_immediately(invert)
        .expect("first run starts");
    assert!(app.is_plugin_filter_running());
    let error = app
        .run_catalog_plugin_immediately(invert)
        .expect_err("a second concurrent run must be rejected");
    assert!(
        error.contains("already executing") || error.contains("already running"),
        "error was {error}"
    );
    // Let the first run finish so the tempdir and JVM are cleaned up.
    wait_for_filter(&mut app, Duration::from_secs(20)).expect("first run completes");
}

#[test]
fn plugin_menu_entries_show_enabled_state_and_concrete_disabled_reasons() {
    let tmp = build_plugins_dir();
    let plugins_dir = tmp.path().join("plugins");
    let cache_path = tmp.path().join("cache").join("plugin_catalog.json");
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    app.set_plugin_catalog_cache_path(Some(cache_path.clone()));
    let id8 = open_ramp(&mut app, "menu-eight-bit", 8, 6);
    scan(&mut app, &plugins_dir);

    let invert = index_of(&app, "Invert 8-bit");
    inspect(&mut app, invert);
    let simple = index_of(&app, "Simple Plugin");
    inspect(&mut app, simple);

    let entries = app.plugin_menu_entries();
    assert_eq!(entries.len(), catalog(&app).plugins.len());
    let invert_entry = entries
        .iter()
        .find(|entry| entry.label == "Invert 8-bit")
        .expect("invert row");
    assert!(invert_entry.enabled);
    assert_eq!(invert_entry.reason, None);
    assert!(invert_entry.source.contains("Probe_Plugins.jar"));

    let uninspected_entry = entries
        .iter()
        .find(|entry| entry.label == "Dummy Filter")
        .expect("uninspected row");
    assert!(!uninspected_entry.enabled);
    assert!(
        uninspected_entry
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("Uninspected"))
    );

    let incompatible_entry = entries
        .iter()
        .find(|entry| entry.label == "Simple Plugin")
        .expect("incompatible row");
    assert!(!incompatible_entry.enabled);
    assert!(
        incompatible_entry
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("verified PlugInFilter"))
    );

    // The same row becomes disabled with the active image's preflight reason.
    let gray16 =
        ImageDocument::new("menu-sixteen-bit", 8, 8, PixelPlane::Gray16(vec![0; 8 * 8])).unwrap();
    let id16 = app.session.insert_doc(gray16);
    app.set_active_window(id16).unwrap();
    let entries = app.plugin_menu_entries();
    let invert_entry = entries
        .iter()
        .find(|entry| entry.label == "Invert 8-bit")
        .expect("invert row");
    assert!(!invert_entry.enabled);
    assert!(
        invert_entry
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("not compatible"))
    );

    // Restoring the 8-bit image re-enables the row and keeps the source path.
    app.set_active_window(id8).unwrap();
    let entries = app.plugin_menu_entries();
    let invert_entry = entries
        .iter()
        .find(|entry| entry.label == "Invert 8-bit")
        .expect("invert row");
    assert!(invert_entry.enabled);

    // An unresolved close/quit confirmation disables every row and rejects
    // the one-click path before it can touch document state.
    app.pending_close = Some(PendingClose {
        target: CloseTarget::Single(id8),
        queue: Vec::new(),
        current_prompt: Some(id8),
    });
    let entries = app.plugin_menu_entries();
    let invert_entry = entries
        .iter()
        .find(|entry| entry.label == "Invert 8-bit")
        .expect("invert row");
    assert!(!invert_entry.enabled);
    assert!(
        invert_entry
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("close/quit"))
    );
    let error = app
        .run_catalog_plugin_immediately(invert)
        .expect_err("close confirmation must block one-click runs");
    assert!(error.contains("close/quit"), "error was {error}");
    app.pending_close = None;

    // A cache reload preserves the source path and the enabled state.
    let mut restored = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    restored.set_plugin_catalog_cache_path(Some(cache_path));
    open_ramp(&mut restored, "menu-restored", 8, 6);
    assert!(restored.load_plugin_catalog_cache_at_startup());
    let restored_entries = restored.plugin_menu_entries();
    let restored_invert = restored_entries
        .iter()
        .find(|entry| entry.label == "Invert 8-bit")
        .expect("restored invert row");
    assert!(restored_invert.enabled);
    assert!(restored_invert.source.contains("Probe_Plugins.jar"));
    assert!(
        catalog(&restored)
            .classpath
            .iter()
            .any(|path| path.ends_with("fixtures.jar"))
    );
}
