//! Tests for Java PlugInFilter UI exposure, asynchronous execution, cancellation,
//! close/dirty guards, selected image identity, failure handling, and state preservation.

use ij_app::{ImageJApp, PluginFilterPhase};
use ij_bridge::plugin::{PluginBridge, PluginBridgeError};
use ij_core::{ImageDocument, PixelPlane, ThresholdLevels};
use ij_session::{ImageId, SessionError};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("locate repo root")
        .to_path_buf()
}

fn fixture_jar_path() -> PathBuf {
    repo_root().join("tools/plugin-host/fixtures/fixtures.jar")
}

fn plugin_bridge() -> PluginBridge {
    PluginBridge::locate_from_repo_root(repo_root()).expect("locate plugin bridge")
}

fn setup_app_with_ramp(title: &str, width: u32, height: u32) -> (ImageJApp, ImageId) {
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    let id = app.open_ramp_8bit(title, width, height).expect("open ramp");
    app.set_dirty(id, false);
    (app, id)
}

fn gray8_pixels(app: &ImageJApp, id: ImageId) -> Vec<u8> {
    match app.session.get_image(id).expect("image").plane() {
        PixelPlane::Gray8(pixels) => pixels.clone(),
        other => panic!("expected 8-bit grayscale plane, got {other:?}"),
    }
}

/// Helper snapshot of state that must remain transactionally untouched on failure/cancellation.
#[derive(Debug, PartialEq)]
struct AppStateSnapshot {
    image: ImageDocument,
    dirty: bool,
    results: String,
    macro_recorder: Vec<String>,
}

fn capture_snapshot(app: &ImageJApp, id: ImageId) -> AppStateSnapshot {
    AppStateSnapshot {
        image: app.session.get_image(id).expect("doc").clone(),
        dirty: app.is_dirty(id),
        results: app.session.results.to_tsv_basic(),
        macro_recorder: app.session.macro_recorder.clone(),
    }
}

#[test]
fn test_plugin_filter_sync_success_and_state_preservation() {
    let (mut app, id) = setup_app_with_ramp("test-ramp", 32, 24);
    // Set a rectangular ROI: (x=4, y=4, w=10, h=8)
    app.dispatch_rect_roi(4, 4, 10, 8).unwrap();

    let initial_pixels = gray8_pixels(&app, id);
    let initial_doc = app.session.get_image(id).unwrap().clone();
    let initial_results = app.session.results.to_tsv_basic();
    let initial_macros = app.session.macro_recorder.len();

    // Execute through actual Session path via app.dispatch_plugin_filter_sync
    let outcome = app
        .dispatch_plugin_filter_sync(
            "fixtures.RectInvertFilter".to_string(),
            vec![fixture_jar_path()],
            String::new(),
        )
        .expect("supported rect invert filter should succeed");

    assert_eq!(outcome.class_name, "fixtures.RectInvertFilter");
    assert_eq!(outcome.run_invocations, 1);

    // Verify modified pixels: inside ROI inverted (255 - p), outside untouched
    let after_pixels = gray8_pixels(&app, id);
    for y in 0..24 {
        for x in 0..32 {
            let idx = (y * 32 + x) as usize;
            let orig = initial_pixels[idx];
            let after = after_pixels[idx];
            if x >= 4 && x < 14 && y >= 4 && y < 12 {
                assert_eq!(
                    after,
                    255 - orig,
                    "pixel at ({x},{y}) inside ROI should be inverted"
                );
            } else {
                assert_eq!(
                    after, orig,
                    "pixel at ({x},{y}) outside ROI should be unchanged"
                );
            }
        }
    }

    // Verify document identity, metadata, and state preservation
    let after_doc = app.session.get_image(id).unwrap();
    assert_eq!(after_doc.title, initial_doc.title);
    assert_eq!(after_doc.width, initial_doc.width);
    assert_eq!(after_doc.height, initial_doc.height);
    assert_eq!(after_doc.slices.len(), 1);
    assert_eq!(after_doc.calibration, initial_doc.calibration);
    assert_eq!(after_doc.display_range, initial_doc.display_range);
    assert_eq!(after_doc.roi, initial_doc.roi);
    assert_eq!(after_doc.threshold, initial_doc.threshold);

    // Document must now be marked dirty
    assert!(app.is_dirty(id), "modified document must be marked dirty");

    // Results table must be untouched by filter
    assert_eq!(app.session.results.to_tsv_basic(), initial_results);

    // Macro recorder recorded the unsupported replay note
    assert_eq!(
        &app.session.macro_recorder[initial_macros..],
        &[
            "// Java plugin filter fixtures.RectInvertFilter: macro replay is not supported"
                .to_string()
        ]
    );

    // Undo should restore pre-filter pixels
    app.undo_active().expect("undo should succeed");
    assert_eq!(
        gray8_pixels(&app, id),
        initial_pixels,
        "undo should restore exact pre-filter pixels"
    );
}

#[test]
fn test_plugin_filter_async_task_lifecycle_and_completion() {
    let (mut app, id) = setup_app_with_ramp("async-ramp", 24, 20);
    assert!(!app.is_plugin_filter_running());

    // Launch background task
    app.start_plugin_filter_task(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    )
    .expect("launch task");

    assert!(
        app.is_plugin_filter_running(),
        "task must be marked running"
    );
    assert!(
        app.is_plugin_filter_running_for(id),
        "task must target image id"
    );

    // Poll until completed (with timeout)
    let start = std::time::Instant::now();
    let mut finished = false;
    while start.elapsed() < Duration::from_secs(15) {
        if let Some(res) = app.poll_plugin_filter() {
            let outcome = res.expect("async filter should succeed");
            assert_eq!(outcome.class_name, "fixtures.RectInvertFilter");
            finished = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    assert!(finished, "async task did not finish within timeout");
    assert!(!app.is_plugin_filter_running());
    assert!(app.is_dirty(id));
}

#[test]
fn test_plugin_filter_cancellation_leaves_image_untouched() {
    let (mut app, id) = setup_app_with_ramp("cancel-ramp", 20, 20);
    let before = capture_snapshot(&app, id);

    app.start_plugin_filter_task(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    )
    .expect("launch task");

    assert!(app.is_plugin_filter_running());

    // Immediately cancel: the worker stays pending until the JVM is reaped.
    let cancelled = app.cancel_plugin_filter();
    assert!(cancelled);
    assert!(app.is_plugin_filter_running());
    assert_eq!(
        app.plugin_filter_phase(),
        Some(PluginFilterPhase::Cancelling)
    );

    // Wait for the worker to observe cancellation and exit.
    let start = std::time::Instant::now();
    let mut completion = None;
    while start.elapsed() < Duration::from_secs(15) {
        if let Some(result) = app.poll_plugin_filter() {
            completion = Some(result);
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let completion = completion.expect("cancellation did not finish");
    assert!(matches!(
        completion,
        Err(SessionError::Plugin(PluginBridgeError::Cancelled))
    ));
    assert!(!app.is_plugin_filter_running());
    assert_eq!(app.plugin_filter_phase(), None);

    // State is 100% untouched
    let after = capture_snapshot(&app, id);
    assert_eq!(
        after, before,
        "cancellation must leave image and session completely untouched"
    );
}

#[test]
fn test_plugin_filter_close_and_quit_guards() {
    let (mut app, id) = setup_app_with_ramp("guard-ramp", 20, 20);

    app.start_plugin_filter_task(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    )
    .expect("launch task");

    assert!(app.is_plugin_filter_running_for(id));

    // 1. Direct close_window is blocked
    assert!(
        !app.close_window(id),
        "close_window must be blocked while running"
    );
    assert!(app.session.get_image(id).is_some());

    // 2. request_close_window is blocked
    assert!(
        !app.request_close_window(id),
        "request_close_window must be blocked"
    );
    assert!(app.session.get_image(id).is_some());

    // 3. request_close_all is blocked
    assert!(
        !app.request_close_all(),
        "request_close_all must be blocked"
    );
    assert!(app.session.get_image(id).is_some());

    // 4. request_quit is blocked
    assert!(!app.request_quit(), "request_quit must be blocked");
    assert!(!app.quit_app);

    // Cancel task; closing stays blocked until the child JVM has exited.
    app.cancel_plugin_filter();
    assert!(
        !app.close_window(id),
        "close must stay blocked while cancellation is in flight"
    );
    let start = std::time::Instant::now();
    while app.is_plugin_filter_running() && start.elapsed() < Duration::from_secs(15) {
        app.poll_plugin_filter();
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!app.is_plugin_filter_running());
    assert!(
        app.close_window(id),
        "close_window should succeed once cancelled"
    );
    assert!(app.session.get_image(id).is_none());
}

#[test]
fn test_selected_image_identity_maintained_across_active_window_switches() {
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());
    let id1 = app.open_ramp_8bit("image-1", 16, 16).unwrap();
    let id2 = app.open_ramp_8bit("image-2", 24, 24).unwrap();
    app.set_dirty(id1, false);
    app.set_dirty(id2, false);

    // Activate image 1 and start plugin filter on it
    app.set_active_window(id1).unwrap();
    let initial1 = gray8_pixels(&app, id1);
    let initial2 = gray8_pixels(&app, id2);

    app.start_plugin_filter_task(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    )
    .unwrap();

    assert!(app.is_plugin_filter_running_for(id1));
    assert!(!app.is_plugin_filter_running_for(id2));

    // While task is running, user switches active image to Image 2!
    app.set_active_window(id2).unwrap();
    assert_eq!(app.active_image_id(), Some(id2));

    // Wait for completion
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        if app.poll_plugin_filter().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    // Image 1 (the target) must be modified and marked dirty
    assert_ne!(
        gray8_pixels(&app, id1),
        initial1,
        "image 1 should be processed"
    );
    assert!(app.is_dirty(id1));

    // Image 2 (which became active) must be completely UNTOUCHED and CLEAN
    assert_eq!(
        gray8_pixels(&app, id2),
        initial2,
        "image 2 must remain completely untouched"
    );
    assert!(!app.is_dirty(id2), "image 2 must remain clean");
}

#[test]
fn test_failure_handling_and_state_preservation() {
    let (mut app, id) = setup_app_with_ramp("failing-ramp", 24, 24);
    // Seed some initial state: inverted once, measured, dirty
    app.invert_active().unwrap();
    app.measure_active().unwrap();
    let before = capture_snapshot(&app, id);

    // 1. Filter throwing exception
    let err = app.dispatch_plugin_filter_sync(
        "fixtures.FailingFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    );
    match err {
        Err(SessionError::Plugin(PluginBridgeError::FilterException { message, .. })) => {
            assert!(message.contains("fixture-failure-after-mutation"));
        }
        other => panic!("expected FilterException, got {other:?}"),
    }
    assert_eq!(
        capture_snapshot(&app, id),
        before,
        "exception must preserve state"
    );

    // 2. Class not found
    let err_missing = app.dispatch_plugin_filter_sync(
        "fixtures.NonExistent".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    );
    match err_missing {
        Err(SessionError::Plugin(PluginBridgeError::FilterClassNotFound { .. })) => {}
        other => panic!("expected FilterClassNotFound, got {other:?}"),
    }
    assert_eq!(
        capture_snapshot(&app, id),
        before,
        "class not found must preserve state"
    );

    // 3. Output metadata alteration rejected
    let err_meta = app.dispatch_plugin_filter_sync(
        "fixtures.OutputDisplayRangeFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    );
    match err_meta {
        Err(SessionError::Plugin(PluginBridgeError::FilterRejected { .. })) => {}
        other => panic!("expected FilterRejected, got {other:?}"),
    }
    assert_eq!(
        capture_snapshot(&app, id),
        before,
        "metadata alteration rejection must preserve state"
    );
}

#[test]
fn test_unsupported_document_inputs_rejected_without_modifications() {
    let mut app = ImageJApp::new().with_plugin_bridge(plugin_bridge());

    // 1. Hyperstack (a plain Z-stack is transferable since P3.10; a
    //    C/T hyperstack dimension is not).
    let stack_doc = ImageDocument::new_hyperstack(
        "hyper",
        16,
        16,
        vec![
            PixelPlane::Gray8(vec![0; 256]),
            PixelPlane::Gray8(vec![0; 256]),
        ],
        2,
        1,
        1,
    )
    .unwrap();
    let id_stack = app.session.insert_doc(stack_doc);
    let before_stack = capture_snapshot(&app, id_stack);
    app.set_active_window(id_stack).unwrap();

    let res_stack = app.dispatch_plugin_filter_sync(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    );
    assert!(matches!(
        res_stack,
        Err(SessionError::Plugin(
            PluginBridgeError::UnsupportedFilterInput(_)
        ))
    ));
    assert_eq!(capture_snapshot(&app, id_stack), before_stack);

    // 2. 16-bit grayscale offered to an 8-bit-only filter: since P3.10 the
    //    image itself transfers, so the rejection now comes from the Java
    //    host's setup() flag preflight, transactionally unchanged.
    let gray16_doc = ImageDocument::new("g16", 16, 16, PixelPlane::Gray16(vec![0; 256])).unwrap();
    let id_16 = app.session.insert_doc(gray16_doc);
    let before_16 = capture_snapshot(&app, id_16);
    app.set_active_window(id_16).unwrap();

    let res_16 = app.dispatch_plugin_filter_sync(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    );
    match res_16 {
        Err(SessionError::Plugin(PluginBridgeError::FilterRejected { message })) => {
            assert!(
                message.contains("8-bit grayscale"),
                "rejection should name the required image type: {message}"
            );
        }
        other => panic!("expected FilterRejected for the depth mismatch, got {other:?}"),
    }
    assert_eq!(capture_snapshot(&app, id_16), before_16);

    // 3. Oval ROI
    let (mut app_oval, id_oval) = setup_app_with_ramp("oval-doc", 20, 20);
    app_oval.dispatch_oval_roi(2, 2, 8, 8).unwrap();
    let before_oval = capture_snapshot(&app_oval, id_oval);

    let res_oval = app_oval.dispatch_plugin_filter_sync(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    );
    assert!(matches!(
        res_oval,
        Err(SessionError::Plugin(
            PluginBridgeError::UnsupportedFilterInput(_)
        ))
    ));
    assert_eq!(capture_snapshot(&app_oval, id_oval), before_oval);

    // 4. Active threshold
    let (mut app_thresh, id_thresh) = setup_app_with_ramp("thresh-doc", 20, 20);
    app_thresh.session.active_doc_mut().unwrap().threshold = Some(ThresholdLevels {
        lower: 50.0,
        upper: 150.0,
    });
    let before_thresh = capture_snapshot(&app_thresh, id_thresh);

    let res_thresh = app_thresh.dispatch_plugin_filter_sync(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    );
    assert!(matches!(
        res_thresh,
        Err(SessionError::UnsupportedPluginFilterState(_))
    ));
    assert_eq!(capture_snapshot(&app_thresh, id_thresh), before_thresh);

    // 5. Empty class name
    let (mut app_empty, id_empty) = setup_app_with_ramp("empty-doc", 20, 20);
    let before_empty = capture_snapshot(&app_empty, id_empty);

    let res_empty = app_empty.dispatch_plugin_filter_sync(
        "   ".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    );
    assert!(matches!(
        res_empty,
        Err(SessionError::InvalidPluginFilterRequest(_))
    ));
    assert_eq!(capture_snapshot(&app_empty, id_empty), before_empty);

    // 6. No bridge configured
    let mut app_nobridge = ImageJApp::new();
    let id_nobridge = app_nobridge.open_ramp_8bit("nobridge", 20, 20).unwrap();
    let before_nobridge = capture_snapshot(&app_nobridge, id_nobridge);

    let res_nobridge = app_nobridge.dispatch_plugin_filter_sync(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    );
    assert!(matches!(
        res_nobridge,
        Err(SessionError::PluginBridgeUnavailable)
    ));
    assert_eq!(
        capture_snapshot(&app_nobridge, id_nobridge),
        before_nobridge
    );
}

#[test]
fn test_ui_dialog_state_management() {
    let mut app = ImageJApp::new();
    assert!(!app.dialogs.show_plugin_filter);
    assert!(app.dialogs.plugin_filter_classpath.is_empty());

    // Set dialog fields
    app.dialogs.show_plugin_filter = true;
    app.dialogs.plugin_filter_class = "fixtures.RectInvertFilter".to_string();
    app.dialogs.plugin_filter_arg = "test-arg".to_string();
    app.dialogs.plugin_filter_classpath.push(fixture_jar_path());

    assert_eq!(app.dialogs.plugin_filter_class, "fixtures.RectInvertFilter");
    assert_eq!(app.dialogs.plugin_filter_arg, "test-arg");
    assert_eq!(app.dialogs.plugin_filter_classpath.len(), 1);

    // Auto-detect bridge
    assert!(app.try_detect_plugin_bridge());
    assert!(app.plugin_bridge().is_some());
}
