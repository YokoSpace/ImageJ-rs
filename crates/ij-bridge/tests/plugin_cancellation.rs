//! Cancellation tests for the isolated JVM plugin bridge.
//!
//! The bridge runs every plugin in its own process group. Cancellation must
//! terminate that whole group, reap the direct child before returning, and
//! report a distinct `Cancelled` error so callers never confuse it with a
//! timeout or an ordinary plugin failure.

use ij_bridge::plugin::{CancellationToken, PluginBridge, PluginBridgeError, PluginFilterRequest};
use ij_core::ImageDocument;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
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

fn bridge_with_runner(script: &Path) -> PluginBridge {
    PluginBridge::locate_from_repo_root(repo_root())
        .expect("locate bridge")
        .with_runner_script(script)
}

fn filter_request() -> PluginFilterRequest {
    PluginFilterRequest::new(
        "fixtures.RectInvertFilter",
        "",
        ImageDocument::new_ramp_8bit("cancel-fixture", 8, 8).expect("ramp"),
    )
    .with_classpath_entry(fixture_jar_path())
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

fn read_pid(path: &Path) -> i32 {
    // A shell redirection creates the file before the PID bytes are written.
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(pid) = std::fs::read_to_string(path)
            .unwrap_or_default()
            .trim()
            .parse()
        {
            return pid;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("PID file was not populated: {}", path.display())
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
fn wait_until_gone(pid: i32, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if !process_exists(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    !process_exists(pid)
}

#[test]
fn cancellation_kills_and_reaps_the_process_group_before_returning() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pid_file = tmp.path().join("wrapper.pid");
    let child_pid_file = tmp.path().join("sleep.pid");
    let script = write_script(
        tmp.path(),
        "slow_runner.sh",
        &format!(
            "echo $$ > '{}'\nsleep 60 &\necho $! > '{}'\nwait",
            pid_file.display(),
            child_pid_file.display()
        ),
    );

    let bridge = bridge_with_runner(&script);
    let request = filter_request();
    let token = CancellationToken::new();
    let worker_token = token.clone();

    let handle = thread::spawn(move || {
        let result = bridge.run_filter_with_cancel(&request, &worker_token);
        (result, Instant::now())
    });

    assert!(
        wait_for_file(&pid_file, Duration::from_secs(5)),
        "slow runner did not start"
    );
    assert!(
        wait_for_file(&child_pid_file, Duration::from_secs(5)),
        "slow runner did not start its child"
    );
    let wrapper_pid = read_pid(&pid_file);
    let child_pid = read_pid(&child_pid_file);
    #[cfg(unix)]
    {
        assert!(process_exists(wrapper_pid), "wrapper should be running");
        assert!(process_exists(child_pid), "child should be running");
    }

    let cancel_started = Instant::now();
    token.cancel();
    let (result, _) = handle.join().expect("worker thread");
    let elapsed = cancel_started.elapsed();

    match result {
        Err(PluginBridgeError::Cancelled) => {}
        other => panic!("expected Cancelled, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(5),
        "cancellation took too long: {elapsed:?}"
    );

    // The direct child was reaped and the whole process group was killed
    // before `run_filter_with_cancel` returned.
    #[cfg(unix)]
    {
        assert!(
            wait_until_gone(wrapper_pid, Duration::from_secs(2)),
            "wrapper process {wrapper_pid} was not reaped"
        );
        assert!(
            wait_until_gone(child_pid, Duration::from_secs(2)),
            "process-group child {child_pid} survived cancellation"
        );
    }
}

#[test]
fn a_pre_cancelled_token_never_spawns_the_host() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let marker = tmp.path().join("spawned");
    let script = write_script(
        tmp.path(),
        "should_not_run.sh",
        &format!("touch '{}'\nsleep 60", marker.display()),
    );
    let bridge = bridge_with_runner(&script);
    let request = filter_request();
    let token = CancellationToken::new();
    token.cancel();

    let result = bridge.run_filter_with_cancel(&request, &token);
    match result {
        Err(PluginBridgeError::Cancelled) => {}
        other => panic!("expected Cancelled, got {other:?}"),
    }
    assert!(
        !marker.exists(),
        "a pre-cancelled run must not spawn the plugin host"
    );
}

#[cfg(unix)]
struct DetachedHolder {
    pid: i32,
}

#[cfg(unix)]
impl Drop for DetachedHolder {
    fn drop(&mut self) {
        let _ = Command::new("/bin/kill")
            .arg("-9")
            .arg(self.pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = wait_until_gone(self.pid, Duration::from_secs(2));
    }
}

/// Starts a wrapper that spawns a detached grandchild in a new session. The
/// grandchild inherits the wrapper's stdout/stderr pipes but is outside the
/// wrapper's process group, so killing the group cannot close those pipes.
fn detached_holder_script(dir: &Path, sleep_secs: u32) -> (PathBuf, PathBuf) {
    let holder_pid = dir.join("holder.pid");
    let script = write_script(
        dir,
        "detached_holder_runner.sh",
        &format!(
            "python3 -c 'import os, time
os.setsid()
open(\"{holder}\", \"w\").write(str(os.getpid()))
print(\"detached-holder\", flush=True)
time.sleep({sleep_secs})
' &
sleep {sleep_secs}",
            holder = holder_pid.display(),
            sleep_secs = sleep_secs,
        ),
    );
    (script, holder_pid)
}

fn recv_result<T>(rx: &mpsc::Receiver<T>, timeout: Duration, label: &str) -> T {
    rx.recv_timeout(timeout)
        .unwrap_or_else(|err| panic!("{label}: {err}; the bridge is still blocked"))
}

#[test]
#[cfg(unix)]
fn cancellation_is_bounded_when_a_detached_child_holds_the_pipe() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (script, holder_pid_path) = detached_holder_script(tmp.path(), 120);
    let bridge = bridge_with_runner(&script);
    let request = filter_request();
    let token = CancellationToken::new();
    let worker_token = token.clone();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = bridge.run_filter_with_cancel(&request, &worker_token);
        let _ = tx.send(result);
    });

    assert!(
        wait_for_file(&holder_pid_path, Duration::from_secs(5)),
        "detached holder did not start"
    );
    let holder_pid = read_pid(&holder_pid_path);
    let _guard = DetachedHolder { pid: holder_pid };
    assert!(
        process_exists(holder_pid),
        "detached holder should still be running"
    );

    let cancel_started = Instant::now();
    token.cancel();
    let result = recv_result(
        &rx,
        Duration::from_secs(3),
        "cancellation did not return while a detached child held the pipe",
    );
    let elapsed = cancel_started.elapsed();

    match result {
        Err(PluginBridgeError::Cancelled) => {}
        other => panic!("expected Cancelled, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(3),
        "cancellation was not bounded: {elapsed:?}"
    );
    assert!(
        process_exists(holder_pid),
        "the detached holder must have survived the process-group kill, otherwise this test would not exercise the hanging-pipe case"
    );
}

#[test]
#[cfg(unix)]
fn timeout_is_bounded_when_a_detached_child_holds_the_pipe() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (script, holder_pid_path) = detached_holder_script(tmp.path(), 120);
    // The timeout starts before the helper process is scheduled. Give a busy
    // CI host time to start it so this test reaches the inherited-pipe case.
    let timeout = Duration::from_secs(10);
    let bridge = bridge_with_runner(&script).with_timeout(timeout);
    let request = filter_request();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = bridge.run_filter_with_timeout(&request, timeout);
        let _ = tx.send(result);
    });

    assert!(
        wait_for_file(&holder_pid_path, Duration::from_secs(9)),
        "detached holder did not start"
    );
    let holder_pid = read_pid(&holder_pid_path);
    let _guard = DetachedHolder { pid: holder_pid };

    let result = recv_result(
        &rx,
        timeout + Duration::from_secs(2),
        "timeout did not return while a detached child held the pipe",
    );
    match result {
        Err(PluginBridgeError::Timeout { .. }) => {}
        other => panic!("expected Timeout, got {other:?}"),
    }
    assert!(
        process_exists(holder_pid),
        "the detached holder must survive the process-group kill"
    );
}

#[test]
#[cfg(unix)]
fn diagnostics_are_preserved_when_a_detached_child_keeps_the_pipe_open() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let holder_pid_path = tmp.path().join("holder.pid");
    let script = write_script(
        tmp.path(),
        "diagnostic_runner.sh",
        &format!(
            "python3 -c 'import os, time
os.setsid()
open(\"{holder}\", \"w\").write(str(os.getpid()))
time.sleep(120)
' &
echo 'wrapped-stdout'
echo 'wrapped-stderr' >&2
exit 3",
            holder = holder_pid_path.display(),
        ),
    );
    let bridge = bridge_with_runner(&script);
    let request = filter_request();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = bridge.run_filter(&request);
        let _ = tx.send(result);
    });

    assert!(
        wait_for_file(&holder_pid_path, Duration::from_secs(5)),
        "detached holder did not start"
    );
    let _guard = DetachedHolder {
        pid: read_pid(&holder_pid_path),
    };

    let result = recv_result(
        &rx,
        Duration::from_secs(3),
        "process failure did not return while a detached child held the pipe",
    );
    match result {
        Err(PluginBridgeError::ProcessFailed { code, stderr }) => {
            assert_eq!(code, 3);
            assert!(
                stderr.contains("wrapped-stderr"),
                "wrapper stderr diagnostics were lost: {stderr:?}"
            );
        }
        other => panic!("expected ProcessFailed with diagnostics, got {other:?}"),
    }
}

#[test]
fn a_pre_cancelled_token_skips_document_validation_and_tiff_transfer() {
    let mut image = ImageDocument::new_ramp_8bit("pre-cancel", 8, 8).expect("ramp");
    image.set_oval_roi(1, 1, 4, 4).expect("oval");
    let request = PluginFilterRequest::new("fixtures.RectInvertFilter", "", image)
        .with_classpath_entry(fixture_jar_path());
    let bridge = bridge_with_runner(&repo_root().join("tools/plugin-host/run_host.sh"));
    let token = CancellationToken::new();
    token.cancel();

    match bridge.run_filter_with_cancel(&request, &token) {
        Err(PluginBridgeError::Cancelled) => {}
        other => panic!("pre-cancellation must win over input validation/TIFF I/O, got {other:?}"),
    }
}

#[test]
fn cancellation_is_not_reported_as_a_timeout() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pid_file = tmp.path().join("wrapper.pid");
    let script = write_script(
        tmp.path(),
        "slow_timeout_runner.sh",
        &format!("echo $$ > '{}'\nsleep 60", pid_file.display()),
    );
    let bridge = bridge_with_runner(&script).with_timeout(Duration::from_secs(30));
    let request = filter_request();
    let token = CancellationToken::new();
    let worker_token = token.clone();

    let handle = thread::spawn(move || bridge.run_filter_with_cancel(&request, &worker_token));
    assert!(
        wait_for_file(&pid_file, Duration::from_secs(5)),
        "slow runner did not start"
    );
    token.cancel();
    let result = handle.join().expect("worker thread");

    match result {
        Err(PluginBridgeError::Cancelled) => {}
        Err(PluginBridgeError::Timeout { .. }) => panic!("cancel must not be reported as timeout"),
        other => panic!("expected Cancelled, got {other:?}"),
    }
}
