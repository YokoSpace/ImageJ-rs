//! Resource-bound regression tests for the Java plugin bridge.
//!
//! These tests use small deterministic host substitutes. They never rely on
//! allocating the unbounded output they emulate: the noisy host writes stream
//! data in fixed-size chunks, and oversized protocol files are written in
//! fixed-size chunks.

use ij_bridge::plugin::{
    PluginBridge, PluginBridgeError, PluginFilterRequest, PluginInspectionRequest, PluginRunRequest,
};
use ij_core::ImageDocument;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const HOST_OUTPUT_CAP_BYTES: usize = 1024 * 1024;
const PROTOCOL_CAP_BYTES: usize = 4 * 1024 * 1024;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("locate repo root")
        .to_path_buf()
}

fn bridge_with_runner(script: &Path) -> PluginBridge {
    PluginBridge::locate_from_repo_root(repo_root())
        .expect("locate bridge")
        .with_runner_script(script)
}

fn write_python_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/usr/bin/env python3\n{body}\n"))
        .expect("write python script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)
            .expect("stat python script")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod python script");
    }
    path
}

fn filter_request(class_name: &str) -> PluginFilterRequest {
    PluginFilterRequest::new(
        class_name,
        "",
        ImageDocument::new_ramp_8bit("output-bound-fixture", 8, 8).expect("ramp"),
    )
}

fn expect_protocol_too_large(result: Result<impl std::fmt::Debug, PluginBridgeError>) {
    let err = result.expect_err("oversized protocol result must be rejected");
    match err {
        PluginBridgeError::ProtocolResultTooLarge { size, max, .. } => {
            assert_eq!(max, PROTOCOL_CAP_BYTES as u64);
            assert!(
                size > max,
                "reported size should explain the overflow: size={size}, max={max}"
            );
        }
        other => panic!("expected ProtocolResultTooLarge, got {other:?}"),
    }
}

#[test]
fn noisy_host_output_is_bounded_and_reports_truncation_on_both_streams() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let script = write_python_script(
        tmp.path(),
        "noisy_runner.py",
        r#"
import sys

chunk_out = b"o" * 65536
chunk_err = b"e" * 65536
for _ in range(24):
    sys.stdout.buffer.write(chunk_out)
    sys.stderr.buffer.write(chunk_err)
sys.stdout.buffer.flush()
sys.stderr.buffer.flush()
"#,
    );
    let bridge = bridge_with_runner(&script);
    let request = PluginRunRequest::new("example.NoisyPlugin", "");

    let started = Instant::now();
    let result = bridge.run_plugin_with_timeout(&request, Duration::from_secs(5));
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "noisy host was not drained promptly: {elapsed:?}"
    );
    match result {
        Err(PluginBridgeError::AbruptExit { stdout, stderr, .. }) => {
            assert!(
                stdout.len() <= HOST_OUTPUT_CAP_BYTES + 256,
                "retained stdout is not bounded: {} bytes",
                stdout.len()
            );
            assert!(
                stderr.len() <= HOST_OUTPUT_CAP_BYTES + 256,
                "retained stderr is not bounded: {} bytes",
                stderr.len()
            );
            assert!(
                stdout.contains("truncated"),
                "stdout truncation was not reported"
            );
            assert!(
                stderr.contains("truncated"),
                "stderr truncation was not reported"
            );
        }
        other => panic!("expected AbruptExit diagnostics for noisy host, got {other:?}"),
    }
}

#[test]
fn valid_protocol_result_below_the_limit_still_succeeds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let script = write_python_script(
        tmp.path(),
        "valid_result_runner.py",
        r#"
import json
import sys


def option(name):
    return sys.argv[sys.argv.index(name) + 1]

result = {
    "status": "success",
    "run_id": option("--run-id"),
    "class_name": option("--class"),
    "arg": option("--arg"),
    "stdout": "host-ok",
    "stderr": "",
}
with open(option("--result-file"), "w", encoding="utf-8") as f:
    json.dump(result, f)
"#,
    );
    let bridge = bridge_with_runner(&script);
    let request = PluginRunRequest::new("example.ValidPlugin", "value");

    let output = bridge
        .run_plugin_with_timeout(&request, Duration::from_secs(5))
        .expect("valid result below the protocol cap should succeed");
    assert!(output.is_success());
    assert_eq!(output.stdout, "host-ok");
}

#[test]
fn oversized_protocol_result_is_rejected_for_run_inspect_and_filter() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let script = write_python_script(
        tmp.path(),
        "oversized_result_runner.py",
        &format!(
            r#"
import sys

path = sys.argv[sys.argv.index("--result-file") + 1]
remaining = {} + 1
chunk = b"x" * 65536
with open(path, "wb") as f:
    while remaining:
        part = chunk[:min(len(chunk), remaining)]
        f.write(part)
        remaining -= len(part)
"#,
            PROTOCOL_CAP_BYTES
        ),
    );
    let bridge = bridge_with_runner(&script);

    expect_protocol_too_large(bridge.run_plugin_with_timeout(
        &PluginRunRequest::new("example.OversizedPlugin", ""),
        Duration::from_secs(5),
    ));
    expect_protocol_too_large(bridge.inspect_plugin_with_timeout(
        &PluginInspectionRequest::new("example.OversizedPlugin"),
        Duration::from_secs(5),
    ));
    expect_protocol_too_large(bridge.run_filter_with_timeout(
        &filter_request("example.OversizedPlugin"),
        Duration::from_secs(5),
    ));
}
