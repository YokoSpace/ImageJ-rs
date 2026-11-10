use ij_bridge::plugin::{PluginBridge, PluginBridgeError, PluginRunRequest, PluginRunStatus};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("locate repo root")
        .to_path_buf()
}

fn fixture_jar_path() -> PathBuf {
    repo_root().join("tools/plugin-host/fixtures/fixtures.jar")
}

fn fixture_classes_dir() -> PathBuf {
    repo_root().join("tools/plugin-host/fixtures/classes")
}

#[test]
fn test_plugin_success_from_class_directory() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let req = PluginRunRequest::new("fixtures.SimplePlugIn", "custom-arg-alpha-42")
        .with_classpath_entry(fixture_classes_dir());

    let output = bridge.run_plugin(&req).expect("invoke plugin");
    assert!(output.is_success(), "expected success, got {:?}", output);
    assert_eq!(output.status, PluginRunStatus::Success);
    assert_eq!(output.class_name, "fixtures.SimplePlugIn");
    assert_eq!(output.arg, "custom-arg-alpha-42");
    assert!(
        output
            .stdout
            .contains("SimplePlugIn executed with arg: custom-arg-alpha-42"),
        "stdout was: {}",
        output.stdout
    );
}

#[test]
fn test_plugin_success_from_jar() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let req = PluginRunRequest::new("fixtures.SimplePlugIn", "jar-execution-arg-999")
        .with_classpath_entry(fixture_jar_path());

    let output = bridge.run_plugin(&req).expect("invoke plugin from jar");
    assert!(output.is_success(), "expected success, got {:?}", output);
    assert_eq!(output.status, PluginRunStatus::Success);
    assert_eq!(output.class_name, "fixtures.SimplePlugIn");
    assert_eq!(output.arg, "jar-execution-arg-999");
    assert!(
        output
            .stdout
            .contains("SimplePlugIn executed with arg: jar-execution-arg-999"),
        "stdout was: {}",
        output.stdout
    );
}

#[test]
fn test_plugin_success_isolated_temp_file_side_effect() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let tmp = tempfile::tempdir().expect("create isolated tempdir");
    let marker_file = tmp.path().join("execution_marker.txt");

    let req = PluginRunRequest::new(
        "fixtures.SimplePlugIn",
        format!("write:{}", marker_file.display()),
    )
    .with_classpath_entry(fixture_jar_path())
    .with_work_dir(tmp.path());

    let output = bridge.run_plugin(&req).expect("invoke plugin");
    assert!(output.is_success(), "expected success, got {:?}", output);

    assert!(
        marker_file.exists(),
        "marker file should have been written by plugin"
    );
    let marker_content = std::fs::read_to_string(&marker_file).expect("read marker file");
    assert!(
        marker_content.contains(&format!("executed:write:{}", marker_file.display())),
        "marker content was: {}",
        marker_content
    );
}

#[test]
fn test_plugin_missing_class_reporting() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let req = PluginRunRequest::new("fixtures.NonExistentPluginClass", "any-arg")
        .with_classpath_entry(fixture_jar_path());

    let output = bridge
        .run_plugin(&req)
        .expect("invoke plugin should yield structured output");
    assert!(
        output.is_class_not_found(),
        "expected class not found, got {:?}",
        output
    );
    match &output.status {
        PluginRunStatus::ClassNotFound { message } => {
            assert!(
                message.contains("NonExistentPluginClass")
                    || message.contains("ClassNotFoundException"),
                "error message was: {message}"
            );
        }
        other => panic!("expected ClassNotFound, got {other:?}"),
    }
}

#[test]
fn test_plugin_thrown_exception_reporting() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let req = PluginRunRequest::new("fixtures.FailingPlugIn", "boom-crash-404")
        .with_classpath_entry(fixture_jar_path());

    let output = bridge
        .run_plugin(&req)
        .expect("invoke plugin should yield structured output");
    assert!(
        output.is_exception_thrown(),
        "expected exception thrown, got {:?}",
        output
    );
    match &output.status {
        PluginRunStatus::ExceptionThrown {
            exception_type,
            message,
            stack_trace,
        } => {
            assert!(
                exception_type.contains("IllegalStateException"),
                "expected IllegalStateException, got {exception_type}"
            );
            assert!(
                message.contains("boom-crash-404"),
                "expected message containing arg, got {message}"
            );
            assert!(
                stack_trace.contains("FailingPlugIn.java")
                    || stack_trace.contains("fixtures.FailingPlugIn"),
                "expected non-empty stack trace, got {stack_trace}"
            );
        }
        other => panic!("expected ExceptionThrown, got {other:?}"),
    }
}

#[test]
fn test_plugin_hanging_bounded_process_and_cleanup() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let tmp = tempfile::tempdir().expect("create isolated tempdir");

    let req = PluginRunRequest::new("fixtures.HangingPlugIn", "sleep-arg")
        .with_classpath_entry(fixture_jar_path())
        .with_work_dir(tmp.path());

    // Short timeout of 400ms should cleanly kill the child JVM process
    let result = bridge.run_plugin_with_timeout(&req, Duration::from_millis(400));
    match result {
        Err(PluginBridgeError::Timeout { timeout, .. }) => {
            assert_eq!(timeout, Duration::from_millis(400));
        }
        other => panic!("expected PluginBridgeError::Timeout, got {other:?}"),
    }

    // Temp directory remains intact for caller or clean drops
    assert!(tmp.path().exists());
}

#[test]
fn test_plugin_unsupported_filter_reporting() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let req = PluginRunRequest::new("fixtures.DummyPlugInFilter", "arg")
        .with_classpath_entry(fixture_jar_path());

    let output = bridge
        .run_plugin(&req)
        .expect("invoke plugin should yield structured output");
    assert!(
        output.is_unsupported_plugin_type(),
        "expected unsupported plugin type, got {:?}",
        output
    );
    match &output.status {
        PluginRunStatus::UnsupportedPluginType { message } => {
            assert!(
                message.contains("PlugInFilter")
                    && message.contains("requires --input-image")
                    && message.contains("NO_IMAGE_REQUIRED")
                    && message.contains("GUI and stack compatibility are not supported"),
                "message was: {message}"
            );
        }
        other => panic!("expected UnsupportedPluginType, got {other:?}"),
    }
}

#[test]
fn test_plugin_halt_nonzero_cannot_succeed() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let tmp = tempfile::tempdir().expect("create isolated tempdir");

    let req = PluginRunRequest::new("fixtures.HaltNonZeroPlugIn", "halt-arg")
        .with_classpath_entry(fixture_jar_path())
        .with_work_dir(tmp.path());

    let result = bridge.run_plugin(&req);
    // A plugin halting nonzero MUST NOT be reported as success, even if result.json was written!
    match result {
        Err(PluginBridgeError::ProcessFailed { code, .. }) => {
            assert_eq!(code, 42, "expected exit code 42");
        }
        other => panic!("expected ProcessFailed with code 42, got: {other:?}"),
    }
}

#[test]
fn test_plugin_get_image_exit_without_result() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let tmp = tempfile::tempdir().expect("create isolated tempdir");

    let req = PluginRunRequest::new("fixtures.GetImageExitPlugIn", "any-arg")
        .with_classpath_entry(fixture_jar_path())
        .with_work_dir(tmp.path());

    let result = bridge.run_plugin(&req);
    // IJ.getImage() when no image is open calls System.exit(0) without producing a protocol result.
    // It must report an AbruptExit error containing captured stderr diagnostics.
    match result {
        Err(PluginBridgeError::AbruptExit {
            code,
            stderr,
            stdout,
        }) => {
            assert_eq!(code, 0, "expected exit code 0 from System.exit(0)");
            assert!(
                stderr.contains("There are no images open")
                    || stdout.contains("There are no images open"),
                "expected 'There are no images open' in captured output, stdout={stdout}, stderr={stderr}"
            );
        }
        other => panic!("expected AbruptExit with code 0 and captured diagnostics, got: {other:?}"),
    }
}

#[test]
fn test_plugin_background_thread_terminated() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let tmp = tempfile::tempdir().expect("create isolated tempdir");

    let req = PluginRunRequest::new("fixtures.BackgroundThreadPlugIn", "bg-test")
        .with_classpath_entry(fixture_jar_path())
        .with_work_dir(tmp.path());

    // Should complete cleanly and terminate background thread within 3 seconds, not hang
    let output = bridge
        .run_plugin_with_timeout(&req, Duration::from_secs(3))
        .expect("background thread plugin should terminate and return output");
    assert!(output.is_success(), "expected success, got {:?}", output);
    assert!(
        output
            .stdout
            .contains("BackgroundThreadPlugIn: run() completed"),
        "stdout was: {}",
        output.stdout
    );
}

#[test]
fn test_plugin_relative_classpath_and_work_dir() {
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let relative_cp = PathBuf::from("tools/plugin-host/fixtures/fixtures.jar");
    let tmp = tempfile::tempdir().expect("create tempdir");

    let req = PluginRunRequest::new("fixtures.SimplePlugIn", "relative-arg")
        .with_classpath_entry(relative_cp)
        .with_work_dir(tmp.path());

    let output = bridge
        .run_plugin(&req)
        .expect("plugin with relative classpath should succeed");
    assert!(output.is_success(), "expected success, got {:?}", output);
}

#[test]
fn test_plugin_concurrent_runs_sharing_work_dir() {
    let tmp = tempfile::tempdir().expect("create shared tempdir");
    let shared_dir = tmp.path().to_path_buf();

    let mut handles = Vec::new();
    for i in 0..4 {
        let bridge_ref = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
        let dir = shared_dir.clone();
        let handle = std::thread::spawn(move || {
            let arg = format!("concurrent-run-{i}");
            let req = PluginRunRequest::new("fixtures.SimplePlugIn", &arg)
                .with_classpath_entry(fixture_jar_path())
                .with_work_dir(&dir);

            let out = bridge_ref
                .run_plugin(&req)
                .expect("concurrent run should succeed");
            assert!(out.is_success());
            assert_eq!(out.arg, arg);
            assert!(
                out.stdout
                    .contains(&format!("SimplePlugIn executed with arg: {arg}"))
            );
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("thread join");
    }
}

#[test]
fn test_concurrent_host_builds_are_serialized() {
    let script = repo_root().join("tools/plugin-host/build_host.sh");
    let mut handles = Vec::new();
    for _ in 0..3 {
        let script = script.clone();
        handles.push(std::thread::spawn(move || {
            Command::new(&script)
                .current_dir(repo_root())
                .output()
                .expect("spawn build_host.sh")
        }));
    }
    for (index, handle) in handles.into_iter().enumerate() {
        let output = handle.join().expect("build thread join");
        assert!(
            output.status.success(),
            "concurrent build {index} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // The published host must still execute a fixture correctly.
    let bridge = PluginBridge::locate_from_repo_root(repo_root()).expect("locate bridge");
    let req = PluginRunRequest::new("fixtures.SimplePlugIn", "after-concurrent-build")
        .with_classpath_entry(fixture_jar_path());
    let output = bridge
        .run_plugin(&req)
        .expect("plugin after concurrent builds");
    assert!(output.is_success(), "expected success, got {:?}", output);
    assert!(output.stdout.contains("after-concurrent-build"));
}

#[test]
fn test_plugin_identity_mismatch_detected() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let fake_runner = tmp.path().join("fake_runner.sh");

    // Fake runner that writes a spoofed response with mismatched class name
    let script = r#"#!/bin/sh
for arg in "$@"; do
    if [ "$arg" = "--result-file" ]; then
        next_is_file=1
    elif [ "$next_is_file" = "1" ]; then
        echo '{"status":"success","class_name":"ForgedClass","arg":"spoofed","stdout":"","stderr":""}' > "$arg"
        exit 0
    fi
done
exit 0
"#;
    std::fs::write(&fake_runner, script).expect("write fake runner");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_runner, std::fs::Permissions::from_mode(0o755))
            .expect("set exec");
    }

    let bridge = PluginBridge::locate_from_repo_root(repo_root())
        .expect("locate bridge")
        .with_runner_script(&fake_runner);

    let req = PluginRunRequest::new("fixtures.ExpectedClass", "expected-arg");
    let result = bridge.run_plugin(&req);

    match result {
        Err(PluginBridgeError::IdentityMismatch { expected, actual }) => {
            assert!(
                expected.contains("fixtures.ExpectedClass"),
                "expected: {expected}"
            );
            assert!(actual.contains("ForgedClass"), "actual: {actual}");
        }
        other => panic!("expected IdentityMismatch, got: {other:?}"),
    }
}
