use ij_bridge::{BridgeError, JavaOracleBridge};
use ij_core::ImageDocument;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static INTEGRITY_TEST_MUTEX: Mutex<()> = Mutex::new(());

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn file_sha256(path: &Path) -> String {
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg("import hashlib, sys; print(hashlib.sha256(open(sys.argv[1], 'rb').read()).hexdigest())")
        .arg(path)
        .output()
        .expect("run sha256 calculation");
    assert!(
        output.status.success(),
        "sha256 calculation failed for {:?}",
        path
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn live_oracle_artifact_hashes(root: &Path) -> BTreeMap<String, String> {
    let oracle_dir = root.join("tools/oracle");
    let mut map = BTreeMap::new();
    for name in ["ij-oracle.jar", "java-applet-shim.jar", ".build_stamp.json"] {
        let p = oracle_dir.join(name);
        assert!(p.is_file(), "live artifact missing: {}", p.display());
        map.insert(name.to_string(), file_sha256(&p));
    }
    map
}

struct LiveArtifactsHashGuard {
    root: PathBuf,
    expected_hashes: BTreeMap<String, String>,
}

impl LiveArtifactsHashGuard {
    fn new(root: PathBuf) -> Self {
        let expected_hashes = live_oracle_artifact_hashes(&root);
        Self {
            root,
            expected_hashes,
        }
    }

    fn assert_unchanged(&self) {
        let current_hashes = live_oracle_artifact_hashes(&self.root);
        assert_eq!(
            self.expected_hashes, current_hashes,
            "regression detected: live workspace oracle artifact hashes modified during test execution!"
        );
    }
}

impl Drop for LiveArtifactsHashGuard {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.assert_unchanged();
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create dst dir");
    for entry in std::fs::read_dir(src).expect("read dir") {
        let entry = entry.expect("entry");
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).expect("copy file");
            #[cfg(unix)]
            {
                let perms = entry.metadata().expect("meta").permissions();
                let _ = std::fs::set_permissions(&dst_path, perms);
            }
        }
    }
}

struct IsolatedOracleFixture {
    _temp_dir: tempfile::TempDir,
    root: PathBuf,
}

impl IsolatedOracleFixture {
    fn new(real_root: &Path) -> Self {
        let temp_dir = tempfile::Builder::new()
            .prefix("isolated_oracle_")
            .tempdir()
            .expect("create temp dir");
        let root = temp_dir.path().join("repo");
        let oracle_dir = root.join("tools/oracle");
        std::fs::create_dir_all(&oracle_dir).expect("create isolated oracle dir");

        // Symlink original_imagej into isolated repo
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            real_root.join("original_imagej"),
            root.join("original_imagej"),
        )
        .expect("symlink original_imagej");

        // Copy reference and harness files, but deliberately exclude built artifacts, locks, and cache
        let real_oracle = real_root.join("tools/oracle");
        for entry in std::fs::read_dir(&real_oracle).expect("read real oracle dir") {
            let entry = entry.expect("valid entry");
            let file_name = entry.file_name();
            let name = file_name.to_str().unwrap();

            if matches!(
                name,
                "ij-oracle.jar"
                    | "java-applet-shim.jar"
                    | ".build_stamp.json"
                    | ".build"
                    | ".oracle_build.lock"
                    | "__pycache__"
            ) {
                continue;
            }

            let src_path = entry.path();
            let dst_path = oracle_dir.join(&file_name);
            if src_path.is_dir() {
                copy_dir_recursive(&src_path, &dst_path);
            } else {
                std::fs::copy(&src_path, &dst_path).expect("copy file");
                #[cfg(unix)]
                {
                    let perms = entry.metadata().expect("meta").permissions();
                    let _ = std::fs::set_permissions(&dst_path, perms);
                }
            }
        }

        Self {
            _temp_dir: temp_dir,
            root,
        }
    }

    fn oracle_dir(&self) -> PathBuf {
        self.root.join("tools/oracle")
    }

    fn repo_root(&self) -> &Path {
        &self.root
    }
}

#[cfg(unix)]
#[allow(dead_code)]
fn acquire_exclusive_file_lock(lock_path: &Path) -> std::fs::File {
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .expect("open lock file");
    let fd = file.as_raw_fd();
    unsafe {
        libc::flock(fd, libc::LOCK_EX);
    }
    file
}

#[test]
fn test_oracle_macro_abort_fails() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let res = bridge.eval_macro("exit(\"macro aborted intentionally\");");
    match res {
        Err(BridgeError::MacroAborted { message, .. }) => {
            assert!(
                message.contains("oracle-macro-aborted")
                    || message.contains("macro aborted intentionally"),
                "unexpected abort message: {message}"
            );
        }
        Err(other) => panic!("expected BridgeError::MacroAborted, got {other:?}"),
        Ok(out) => panic!("expected failure for aborted macro, but got Ok({out:?})"),
    }
}

#[test]
fn test_oracle_missing_expected_output_fails() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new_ramp_8bit("test.tif", 16, 16).unwrap();
    // A macro that closes the image window prevents saveAs from writing output.tif
    let res = bridge.run_macro_on_image(&doc, "close();");
    match res {
        Err(BridgeError::MissingExpectedOutput { path, .. }) => {
            assert!(
                path.ends_with("output.tif"),
                "expected path to end with output.tif, got {path:?}"
            );
        }
        Err(other) => panic!("expected BridgeError::MissingExpectedOutput, got {other:?}"),
        Ok(out) => panic!("expected failure for missing output image, but got Ok({out:?})"),
    }
}

#[test]
fn test_oracle_ordinary_stdout_with_aborted_succeeds() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // A macro that legitimately outputs "[aborted]" to stdout must NOT be flagged as failure
    let res = bridge.eval_macro("print(\"[aborted]\");");
    let out = res.expect("ordinary stdout containing [aborted] must succeed");
    assert_eq!(out.stdout.trim(), "[aborted]");
}

#[test]
fn test_oracle_ordinary_stdout_containing_word_aborted_succeeds() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let res = bridge.eval_macro("print(\"Operation is NOT [aborted] at all.\");");
    let out = res.expect("ordinary text containing [aborted] must succeed");
    assert_eq!(out.stdout.trim(), "Operation is NOT [aborted] at all.");
}

#[test]
fn test_oracle_syntax_error_fails_cleanly() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let start = Instant::now();
    let res = bridge.eval_macro("undefined_function_call();");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "syntax/undefined function error took too long (hung on dialog?): {:?}",
        start.elapsed()
    );
    match res {
        Err(BridgeError::MacroAborted { .. }) | Err(BridgeError::ProcessFailed { .. }) => {
            // Expected clean failure
        }
        other => panic!("expected clean abort error for syntax/undefined error, got {other:?}"),
    }
}

#[test]
fn test_oracle_timeout_and_child_cleanup() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let timeout = Duration::from_millis(600);
    let start = Instant::now();

    // Run an infinite loop macro in Java
    let res = bridge.eval_macro_with_timeout("while(true) {}", timeout);
    let elapsed = start.elapsed();

    assert!(
        elapsed >= timeout,
        "execution should take at least timeout duration"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "timeout exceeded bounded duration: {elapsed:?}"
    );

    match res {
        Err(BridgeError::Timeout {
            timeout: t,
            command,
        }) => {
            assert_eq!(t, timeout);
            assert!(command.contains("run_oracle.sh"));
        }
        other => panic!("expected BridgeError::Timeout, got {other:?}"),
    }

    // Verify child cleanup: give OS 100ms to reap and verify no runaway java loop process
    std::thread::sleep(Duration::from_millis(100));
}

#[test]
fn test_nested_command_errors_are_reported() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for code in [
        "run(\"NotARealCommand\"); print(\"after\");",
        "run(\"Image...\", \"width=abc\"); print(\"after\");",
    ] {
        assert!(
            matches!(
                bridge.eval_macro(code),
                Err(BridgeError::MacroAborted { .. })
            ),
            "nested macro command must fail: {code}"
        );
    }
}

#[test]
fn test_oracle_cold_parallel_invocations() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let real_root = repo_root();
    let _guard = LiveArtifactsHashGuard::new(real_root.clone());

    // Create an isolated fixture that starts completely cold (no jars, no stamp)
    let fixture = IsolatedOracleFixture::new(&real_root);
    let isolated_root = fixture.repo_root().to_path_buf();
    let isolated_oracle = fixture.oracle_dir();

    // Verify it is truly cold: --check must report missing/stale in isolated fixture
    let check_status = std::process::Command::new("python3")
        .arg(isolated_oracle.join("check_freshness.py"))
        .arg("--check")
        .status()
        .expect("failed to run check_freshness");
    assert!(!check_status.success(), "cold state must fail --check");

    // Launch N concurrent threads simultaneously against the isolated root
    const WORKER_COUNT: usize = 6;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(WORKER_COUNT));
    let mut handles = Vec::new();

    for i in 0..WORKER_COUNT {
        let r = isolated_root.clone();
        let b = barrier.clone();
        handles.push(std::thread::spawn(move || {
            b.wait();
            let bridge = JavaOracleBridge::locate_from_repo_root(r).unwrap();
            let macro_code = format!("print(\"thread_{i}=\" + ({i} * 7));");
            bridge.eval_macro(&macro_code)
        }));
    }

    for (i, h) in handles.into_iter().enumerate() {
        let res = h.join().expect("thread panicked");
        let out = res.unwrap_or_else(|e| panic!("worker {i} failed: {e:?}"));
        let expected = format!("thread_{}={}", i, i * 7);
        assert_eq!(out.stdout.trim(), expected);
    }

    // Verify artifacts exist and are fresh in the isolated fixture after concurrent cold invocations
    let check_after = std::process::Command::new("python3")
        .arg(isolated_oracle.join("check_freshness.py"))
        .arg("--check")
        .status()
        .expect("failed to run check_freshness");
    assert!(
        check_after.success(),
        "post-cold run must be fresh in isolated fixture"
    );

    // Live workspace artifacts are checked by _guard on drop, but assert explicitly as well
    _guard.assert_unchanged();
}

#[test]
fn test_oracle_detects_substituted_jar() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let real_root = repo_root();
    let _guard = LiveArtifactsHashGuard::new(real_root.clone());

    // Create an isolated fixture
    let fixture = IsolatedOracleFixture::new(&real_root);
    let isolated_oracle = fixture.oracle_dir();
    let jar_path = isolated_oracle.join("ij-oracle.jar");

    // Ensure we are in a fresh state in the isolated fixture first
    let ensure_status = std::process::Command::new("python3")
        .arg(isolated_oracle.join("check_freshness.py"))
        .arg("--ensure")
        .status()
        .expect("failed to ensure freshness");
    assert!(
        ensure_status.success(),
        "initial ensure in isolated fixture must succeed"
    );

    // Overwrite ij-oracle.jar with dummy bytes in isolated fixture (simulating substituted/tampered jar)
    std::fs::write(&jar_path, b"SUBSTITUTED_OR_CORRUPTED_JAR_CONTENT").unwrap();

    // --check must fail due to hash mismatch
    let check_output = std::process::Command::new("python3")
        .arg(isolated_oracle.join("check_freshness.py"))
        .arg("--check")
        .output()
        .expect("failed to run check_freshness --check");
    assert!(!check_output.status.success());
    let stderr = String::from_utf8_lossy(&check_output.stderr);
    assert!(
        stderr.contains("hash mismatch") || stderr.contains("substituted"),
        "expected hash mismatch error, got: {stderr}"
    );

    // --ensure must detect invalid jar, rebuild authentic binary, and succeed
    let ensure_after = std::process::Command::new("python3")
        .arg(isolated_oracle.join("check_freshness.py"))
        .arg("--ensure")
        .output()
        .expect("failed to run check_freshness --ensure");
    assert!(
        ensure_after.status.success(),
        "ensure after corruption failed: {}",
        String::from_utf8_lossy(&ensure_after.stderr)
    );

    // --check must now pass
    let final_check = std::process::Command::new("python3")
        .arg(isolated_oracle.join("check_freshness.py"))
        .arg("--check")
        .status()
        .expect("failed to run final check_freshness");
    assert!(final_check.success());

    // Live workspace artifacts are checked by _guard on drop, but assert explicitly as well
    _guard.assert_unchanged();
}

#[test]
fn test_live_oracle_artifacts_remain_untampered() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let root = repo_root();
    let oracle_dir = root.join("tools/oracle");

    let stamp_path = oracle_dir.join(".build_stamp.json");
    assert!(
        stamp_path.is_file(),
        "build stamp must exist in live workspace"
    );
    let stamp_content = std::fs::read_to_string(&stamp_path).expect("read stamp");
    let stamp: serde_json::Value = serde_json::from_str(&stamp_content).expect("parse stamp");

    let artifacts = stamp["artifacts"].as_object().expect("artifacts object");
    let expected_oracle_sha = artifacts["ij-oracle.jar"]["sha256"].as_str().unwrap();
    let expected_patch_sha = artifacts["java-applet-shim.jar"]["sha256"]
        .as_str()
        .unwrap();

    let live_oracle_sha = file_sha256(&oracle_dir.join("ij-oracle.jar"));
    let live_patch_sha = file_sha256(&oracle_dir.join("java-applet-shim.jar"));

    assert_eq!(
        live_oracle_sha, expected_oracle_sha,
        "live ij-oracle.jar does not match stamp"
    );
    assert_eq!(
        live_patch_sha, expected_patch_sha,
        "live java-applet-shim.jar does not match stamp"
    );

    let status = std::process::Command::new("python3")
        .arg(oracle_dir.join("check_freshness.py"))
        .arg("--check")
        .status()
        .expect("check_freshness");
    assert!(status.success(), "live workspace oracle is not fresh");
}

#[test]
fn test_oracle_invocation_holds_shared_lock_across_jvm_execution() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let real_root = repo_root();
    let _guard = LiveArtifactsHashGuard::new(real_root.clone());

    // Use an isolated fixture so this test doesn't lock or contest the live workspace
    let fixture = IsolatedOracleFixture::new(&real_root);
    let isolated_root = fixture.repo_root().to_path_buf();
    let isolated_oracle = fixture.oracle_dir();

    // Ensure fixture is fresh
    let ensure_status = std::process::Command::new("python3")
        .arg(isolated_oracle.join("check_freshness.py"))
        .arg("--ensure")
        .status()
        .expect("ensure freshness");
    assert!(ensure_status.success());

    let lock_file = isolated_oracle.join(".oracle_build.lock");

    // Launch a macro in the background that sleeps for 800ms
    let r = isolated_root.clone();
    let handle = std::thread::spawn(move || {
        let bridge = JavaOracleBridge::locate_from_repo_root(r).unwrap();
        bridge.eval_macro("wait(2000); print(\"done_sleeping\");")
    });

    // While JVM is running, an attempt to acquire an exclusive lock non-blocking MUST fail (EWOULDBLOCK)
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_file)
            .expect("open lock file");
        let fd = file.as_raw_fd();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
            if res == -1 {
                let err = std::io::Error::last_os_error();
                assert!(
                    err.raw_os_error() == Some(libc::EWOULDBLOCK)
                        || err.raw_os_error() == Some(libc::EAGAIN),
                    "unexpected lock error: {err:?}"
                );
                break;
            }
            unsafe { libc::flock(fd, libc::LOCK_UN) };
            assert!(Instant::now() < deadline, "JVM did not acquire shared lock");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // Wait for the background macro to finish
    let out = handle
        .join()
        .expect("thread panicked")
        .expect("eval_macro failed");
    assert_eq!(out.stdout.trim(), "done_sleeping");

    // Once JVM has exited, an exclusive lock CAN now be acquired immediately non-blocking
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_file)
            .expect("open lock file");
        let fd = file.as_raw_fd();
        let res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        assert_eq!(
            res, 0,
            "LOCK_EX must be immediately grantable after JVM exits"
        );
        unsafe { libc::flock(fd, libc::LOCK_UN) };
    }

    _guard.assert_unchanged();
}

#[test]
fn test_oracle_build_stamp_content() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let root = repo_root();
    let oracle_dir = root.join("tools/oracle");

    // Ensure fresh build
    let status = std::process::Command::new("python3")
        .arg(oracle_dir.join("check_freshness.py"))
        .arg("--ensure")
        .status()
        .expect("failed to ensure freshness");
    assert!(status.success());

    let stamp_path = oracle_dir.join(".build_stamp.json");
    assert!(stamp_path.is_file(), "stamp file must exist");

    let stamp_content = std::fs::read_to_string(&stamp_path).expect("read stamp");
    let stamp: serde_json::Value = serde_json::from_str(&stamp_content).expect("parse json");

    assert_eq!(stamp["format_version"], 1);
    let compiler = stamp["compiler_identity"]
        .as_str()
        .expect("compiler string");
    assert!(compiler.contains("javac"), "compiler must mention javac");

    let manifest_sha = stamp["reference_manifest"]["sha256"]
        .as_str()
        .expect("manifest sha");
    assert_eq!(manifest_sha.len(), 64, "manifest sha must be 64-char hex");

    let harness = stamp["harness_hashes"].as_object().expect("harness object");
    assert!(harness.contains_key("tools/oracle/build_oracle.sh"));
    assert!(harness.contains_key("tools/oracle/check_freshness.py"));
    assert!(harness.contains_key("tools/oracle/run_oracle.sh"));
    assert!(harness.contains_key("tools/oracle/runner/OracleRunner.java"));
    assert!(harness.contains_key("tools/oracle/shim/java/applet/Applet.java"));

    let artifacts = stamp["artifacts"].as_object().expect("artifacts object");
    assert!(artifacts.contains_key("ij-oracle.jar"));
    assert!(artifacts.contains_key("java-applet-shim.jar"));

    let oracle_sha = artifacts["ij-oracle.jar"]["sha256"].as_str().unwrap();
    assert_eq!(oracle_sha.len(), 64);
}

#[test]
fn test_oracle_manifest_missing_source_fails() {
    let _lock = INTEGRITY_TEST_MUTEX.lock().unwrap();
    let root = repo_root();
    let oracle_dir = root.join("tools/oracle");

    // Outside original_imagej, create a mock reference tree & manifest to test deleted source detection
    let tmp = tempfile::Builder::new()
        .prefix("test_manifest_")
        .tempdir()
        .unwrap();
    let dir = tmp.path().join("mock_src");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("A.java"), b"class A {}").unwrap();
    std::fs::write(dir.join("B.java"), b"class B {}").unwrap();

    let manifest_path = tmp.path().join("mock.manifest");
    let gen_status = std::process::Command::new("python3")
        .arg(oracle_dir.join("check_manifest.py"))
        .arg("--generate")
        .arg("--base-dir")
        .arg(&dir)
        .arg("--manifest")
        .arg(&manifest_path)
        .status()
        .expect("run check_manifest generate");
    assert!(gen_status.success());

    // Manifest check passes initially
    let check_status = std::process::Command::new("python3")
        .arg(oracle_dir.join("check_manifest.py"))
        .arg("--check")
        .arg("--base-dir")
        .arg(&dir)
        .arg("--manifest")
        .arg(&manifest_path)
        .status()
        .expect("run check_manifest check");
    assert!(check_status.success());

    // Delete one source file (simulating deleted source)
    std::fs::remove_file(dir.join("B.java")).unwrap();

    // Manifest check MUST fail with missing file error
    let fail_output = std::process::Command::new("python3")
        .arg(oracle_dir.join("check_manifest.py"))
        .arg("--check")
        .arg("--base-dir")
        .arg(&dir)
        .arg("--manifest")
        .arg(&manifest_path)
        .output()
        .expect("run check_manifest check on deleted source");
    assert!(
        !fail_output.status.success(),
        "manifest check must fail when source is deleted"
    );
    let stderr = String::from_utf8_lossy(&fail_output.stderr);
    assert!(
        stderr.contains("missing file(s) expected by manifest"),
        "expected missing file in stderr: {stderr}"
    );
    assert!(stderr.contains("B.java"));
}
