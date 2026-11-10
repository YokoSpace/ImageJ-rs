//! Regression tests for the async PlugInFilter review findings:
//! cancellation must keep the UI in a cancelling state until the child JVM is
//! dead and reaped, and stale results must never reach the session.

use ij_app::{ImageJApp, PluginFilterPhase};
use ij_bridge::plugin::{PluginBridge, PluginBridgeError};
use ij_core::{PixelPlane, RectRoi, Roi};
use ij_session::{ImageId, PluginFilterOutcome, SessionError};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("locate repo root")
        .to_path_buf()
}

fn fixture_jar_path() -> PathBuf {
    repo_root().join("tools/plugin-host/fixtures/fixtures.jar")
}

fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).expect("write script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).expect("stat script").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod script");
    }
    path
}

fn bridge_with_runner(script: &Path) -> PluginBridge {
    PluginBridge::locate_from_repo_root(repo_root())
        .expect("locate bridge")
        .with_runner_script(script)
}

fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.is_file() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    path.is_file()
}

fn gray8(app: &ImageJApp, id: ImageId) -> Vec<u8> {
    match app.session.get_image(id).expect("image").plane() {
        PixelPlane::Gray8(pixels) => pixels.clone(),
        other => panic!("expected 8-bit grayscale, got {other:?}"),
    }
}

fn wait_for_completion(
    app: &mut ImageJApp,
    timeout: Duration,
) -> Option<Result<PluginFilterOutcome, SessionError>> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(result) = app.poll_plugin_filter() {
            return Some(result);
        }
        thread::sleep(Duration::from_millis(10));
    }
    None
}

#[test]
fn cancellation_stays_in_cancelling_state_until_the_process_exits() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pid_file = tmp.path().join("wrapper.pid");
    let script = write_script(
        tmp.path(),
        "slow_cancel_runner.sh",
        &format!("echo $$ > '{}'\nsleep 60", pid_file.display()),
    );

    let mut app = ImageJApp::new().with_plugin_bridge(bridge_with_runner(&script));
    let id = app.open_ramp_8bit("cancel-ui", 12, 10).unwrap();
    app.set_dirty(id, false);

    app.start_plugin_filter_task(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    )
    .expect("start task");
    assert!(wait_for_file(&pid_file, Duration::from_secs(5)));

    assert_eq!(app.plugin_filter_phase(), Some(PluginFilterPhase::Running));
    assert!(app.cancel_plugin_filter());
    assert_eq!(
        app.plugin_filter_phase(),
        Some(PluginFilterPhase::Cancelling),
        "cancel must not drop the worker immediately"
    );
    assert!(app.is_plugin_filter_running());
    assert!(!app.cancel_plugin_filter(), "a second cancel is a no-op");

    // Every close/second-run path must remain blocked while the JVM is alive.
    assert!(!app.close_window(id));
    assert!(!app.request_close_window(id));
    assert!(!app.request_close_all());
    assert!(!app.request_quit());
    assert!(matches!(
        app.start_plugin_filter_task(
            "fixtures.RectInvertFilter".to_string(),
            vec![fixture_jar_path()],
            String::new(),
        ),
        Err(SessionError::InvalidPluginFilterRequest(_))
    ));

    let cancel_started = Instant::now();
    let result = wait_for_completion(&mut app, Duration::from_secs(5))
        .expect("cancellation must complete once the process is reaped");
    assert!(
        cancel_started.elapsed() < Duration::from_secs(5),
        "cancellation did not return promptly"
    );
    match result {
        Err(SessionError::Plugin(PluginBridgeError::Cancelled)) => {}
        other => panic!("expected distinct Cancelled result, got {other:?}"),
    }
    assert!(!app.is_plugin_filter_running());
    assert_eq!(app.plugin_filter_phase(), None);
    assert!(
        app.status_message.to_lowercase().contains("cancel"),
        "status should distinguish cancellation: {}",
        app.status_message
    );

    // Only now is closing allowed.
    assert!(app.close_window(id));
}

#[cfg(unix)]
struct DetachedProcess {
    pid: i32,
}

#[cfg(unix)]
impl Drop for DetachedProcess {
    fn drop(&mut self) {
        let _ = Command::new("/bin/kill")
            .arg("-9")
            .arg(self.pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(unix)]
fn process_exists(pid: i32) -> bool {
    Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
fn read_pid(path: &Path) -> i32 {
    std::fs::read_to_string(path)
        .expect("read pid")
        .trim()
        .parse()
        .expect("parse pid")
}

#[test]
#[cfg(unix)]
fn cancellation_leaves_cancelling_state_even_when_a_detached_child_holds_the_pipe() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let holder_pid_file = tmp.path().join("holder.pid");
    let script = write_script(
        tmp.path(),
        "detached_cancel_runner.sh",
        &format!(
            "python3 -c 'import os, time
os.setsid()
open(\"{holder}\", \"w\").write(str(os.getpid()))
print(\"detached-holder\", flush=True)
time.sleep(120)
' &
sleep 120",
            holder = holder_pid_file.display(),
        ),
    );

    let mut app = ImageJApp::new().with_plugin_bridge(bridge_with_runner(&script));
    let id = app.open_ramp_8bit("detached-cancel-ui", 12, 10).unwrap();
    app.set_dirty(id, false);

    app.start_plugin_filter_task(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    )
    .expect("start task");
    assert!(wait_for_file(&holder_pid_file, Duration::from_secs(5)));
    let holder_pid = read_pid(&holder_pid_file);
    let _guard = DetachedProcess { pid: holder_pid };
    assert!(process_exists(holder_pid));

    assert!(app.cancel_plugin_filter());
    assert_eq!(
        app.plugin_filter_phase(),
        Some(PluginFilterPhase::Cancelling)
    );

    let result = wait_for_completion(&mut app, Duration::from_secs(3))
        .expect("the UI must leave Cancelling even while a detached child holds the pipe");
    assert!(matches!(
        result,
        Err(SessionError::Plugin(PluginBridgeError::Cancelled))
    ));
    assert_eq!(app.plugin_filter_phase(), None);
    assert!(!app.is_plugin_filter_running());
    assert!(
        app.status_message.to_lowercase().contains("cancel"),
        "status must describe cancellation: {}",
        app.status_message
    );
    assert!(
        process_exists(holder_pid),
        "the detached holder should still hold the pipe; the guard now cleans it up"
    );
}

#[test]
fn closed_or_cancelled_dialog_target_is_cleared_and_reopened_on_the_active_image() {
    let mut app = ImageJApp::new();
    let id1 = app.open_ramp_8bit("first-target", 8, 8).unwrap();
    let id2 = app.open_ramp_8bit("second-target", 8, 8).unwrap();
    app.set_active_window(id1).unwrap();

    app.open_plugin_filter_dialog();
    assert!(app.dialogs.show_plugin_filter);
    assert_eq!(app.dialogs.plugin_filter_target_id, Some(id1));

    app.dismiss_plugin_filter_dialog();
    assert!(!app.dialogs.show_plugin_filter);
    assert_eq!(
        app.dialogs.plugin_filter_target_id, None,
        "closing/cancelling the dialog must not keep a pinned target"
    );

    app.set_active_window(id2).unwrap();
    app.open_plugin_filter_dialog();
    assert_eq!(
        app.dialogs.plugin_filter_target_id,
        Some(id2),
        "reopening must pin the current active image, not the old closed target"
    );
}

#[test]
fn closing_the_pinned_target_clears_the_dialog_target() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("closing-target", 8, 8).unwrap();
    app.set_dirty(id, false);
    app.open_plugin_filter_dialog();
    assert_eq!(app.dialogs.plugin_filter_target_id, Some(id));

    assert!(app.close_window(id));
    assert_eq!(
        app.dialogs.plugin_filter_target_id, None,
        "a closed target must not remain pinned to a missing image"
    );
}

#[test]
fn a_stale_pinned_target_is_not_silently_retargeted_to_the_active_window() {
    let mut app = ImageJApp::new();
    let stale = app.open_ramp_8bit("stale-target", 8, 8).unwrap();
    let active = app.open_ramp_8bit("active-target", 8, 8).unwrap();
    app.set_dirty(stale, false);
    app.set_dirty(active, false);
    assert!(app.close_window(stale));
    app.set_active_window(active).unwrap();

    // Simulate a pin that outlived its document.
    app.dialogs.show_plugin_filter = true;
    app.dialogs.plugin_filter_target_id = Some(stale);
    app.dialogs.plugin_filter_class = "fixtures.RectInvertFilter".to_string();
    app.dialogs.plugin_filter_classpath = vec![fixture_jar_path()];

    match app.start_plugin_filter_from_dialog() {
        Err(SessionError::InvalidPluginFilterRequest(_)) => {}
        other => panic!("expected stale target rejection, got {other:?}"),
    }
    assert!(!app.is_plugin_filter_running());
    assert!(
        app.dialogs
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("no longer open")),
        "the stale target should be surfaced: {:?}",
        app.dialogs.last_error
    );
}

#[test]
fn a_dialog_without_a_target_does_not_fall_back_to_the_active_image() {
    let mut app = ImageJApp::new();
    let active = app.open_ramp_8bit("active-target", 8, 8).unwrap();
    app.set_dirty(active, false);
    app.dialogs.show_plugin_filter = true;
    app.dialogs.plugin_filter_target_id = None;
    app.dialogs.plugin_filter_class = "fixtures.RectInvertFilter".to_string();
    app.dialogs.plugin_filter_classpath = vec![fixture_jar_path()];

    match app.start_plugin_filter_from_dialog() {
        Err(SessionError::InvalidPluginFilterRequest(_)) => {}
        other => panic!("expected a missing-target rejection, got {other:?}"),
    }
    assert!(!app.is_plugin_filter_running());
}

#[test]
fn dialog_preflight_rejects_an_overflowing_roi_without_panicking() {
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("overflow-ui", 16, 12).unwrap();
    app.session.get_image_mut(id).unwrap().roi = Some(Roi::Rect(RectRoi {
        x: i32::MAX - 1,
        y: 0,
        width: 4,
        height: 1,
    }));
    app.set_active_window(id).unwrap();
    app.open_plugin_filter_dialog();
    assert_eq!(app.dialogs.plugin_filter_target_id, Some(id));

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        app.render_dialogs(ctx);
    });

    assert_eq!(
        app.dialogs.plugin_filter_target_id,
        Some(id),
        "a rejected target must remain displayed, not silently retargeted"
    );
    assert!(
        app.session.plugin_filter_preflight(id).is_err(),
        "the shared bridge preflight must reject the overflowing ROI"
    );
}

#[test]
fn the_displayed_dialog_target_is_the_execution_target() {
    let mut app = ImageJApp::new()
        .with_plugin_bridge(PluginBridge::locate_from_repo_root(repo_root()).expect("bridge"));
    let id1 = app.open_ramp_8bit("dialog-target", 12, 10).unwrap();
    let id2 = app.open_ramp_8bit("other-target", 10, 8).unwrap();
    app.set_dirty(id1, false);
    app.set_dirty(id2, false);

    let before1 = gray8(&app, id1);
    let before2 = gray8(&app, id2);

    // Open the dialog while image 1 is active, then switch the active window
    // before pressing Run. The dialog still displays image 1.
    app.set_active_window(id1).unwrap();
    app.dialogs.show_plugin_filter = true;
    app.dialogs.plugin_filter_target_id = Some(id1);
    app.dialogs.plugin_filter_class = "fixtures.RectInvertFilter".to_string();
    app.dialogs.plugin_filter_classpath = vec![fixture_jar_path()];
    app.set_active_window(id2).unwrap();

    app.start_plugin_filter_from_dialog()
        .expect("start on displayed target");

    let result = wait_for_completion(&mut app, Duration::from_secs(15)).expect("completion");
    assert!(result.is_ok(), "filter should succeed: {result:?}");

    assert_ne!(
        gray8(&app, id1),
        before1,
        "displayed target must be processed"
    );
    assert_eq!(gray8(&app, id2), before2, "active window must be untouched");
    assert!(app.is_dirty(id1));
    assert!(!app.is_dirty(id2));
}

#[test]
fn a_document_change_during_execution_rejects_the_stale_result() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pid_file = tmp.path().join("wrapper.pid");
    let real_runner = repo_root().join("tools/plugin-host/run_host.sh");
    let script = write_script(
        tmp.path(),
        "slow_then_real_runner.sh",
        &format!(
            "echo $$ > '{}'\nsleep 1\nexec '{}' \"$@\"",
            pid_file.display(),
            real_runner.display()
        ),
    );

    let mut app = ImageJApp::new().with_plugin_bridge(bridge_with_runner(&script));
    let id = app.open_ramp_8bit("stale-ui", 14, 12).unwrap();
    app.set_dirty(id, false);
    let before_pixels = gray8(&app, id);

    app.start_plugin_filter_task(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    )
    .expect("start task");
    assert!(wait_for_file(&pid_file, Duration::from_secs(5)));

    // Mutate the same document while the child JVM is still working.
    app.session
        .dispatch(ij_session::Command::SetDisplayRange {
            min: 5.0,
            max: 200.0,
        })
        .unwrap();

    let result = wait_for_completion(&mut app, Duration::from_secs(15)).expect("completion");
    match result {
        Err(SessionError::StalePluginFilterResult { .. }) => {}
        other => panic!("expected stale rejection, got {other:?}"),
    }
    assert_eq!(
        gray8(&app, id),
        before_pixels,
        "stale output must not replace pixels"
    );
    assert_eq!(
        app.session.get_image(id).unwrap().display_range,
        ij_core::DisplayRange {
            min: 5.0,
            max: 200.0
        },
        "the concurrent edit must be preserved"
    );
    assert!(
        app.status_message.to_lowercase().contains("stale")
            || app
                .dialogs
                .last_error
                .as_deref()
                .is_some_and(|e| e.to_lowercase().contains("stale")),
        "stale rejection should be surfaced"
    );
}
