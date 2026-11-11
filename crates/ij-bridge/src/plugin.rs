//! Compatibility bridge for executing ImageJ 1.x plugins.
//!
//! # Lifecycle & Sandboxing Notice
//! The plugin host runner executes plugins in an isolated child JVM process, providing process-level
//! lifecycle management and terminating non-daemon background threads upon plugin return.
//! It makes **NO** claims of security sandboxing untrusted plugin code.

use ij_core::{Calibration, DisplayRange, ImageDocument, PixelPlane, Roi};
use ij_io::{load_tiff, save_tiff};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const DEFAULT_PLUGIN_TIMEOUT: Duration = Duration::from_secs(15);

/// Cooperative cancellation handle shared between the caller and the bridge's
/// child-process wait loop. Cancellation is sticky and may be requested before
/// the child process is spawned.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Error)]
pub enum PluginBridgeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TIFF exchange error: {0}")]
    Tiff(#[from] ij_io::IoError),
    #[error("Plugin host runner script or JAR not found at {0}")]
    HostNotFound(PathBuf),
    #[error("Plugin host build failed: {0}")]
    BuildFailed(String),
    #[error("Plugin execution timed out after {timeout:?}: {command}")]
    Timeout { timeout: Duration, command: String },
    #[error("Plugin execution was cancelled")]
    Cancelled,
    #[error("Plugin host process failed with exit code {code}: {stderr}")]
    ProcessFailed { code: i32, stderr: String },
    #[error(
        "Plugin host process exited abruptly without producing protocol result (exit code {code}): stderr={stderr:?}, stdout={stdout:?}"
    )]
    AbruptExit {
        code: i32,
        stdout: String,
        stderr: String,
    },
    #[error("Protocol identity mismatch: expected {expected}, got {actual}")]
    IdentityMismatch { expected: String, actual: String },
    #[error("Failed to parse plugin host JSON output: {error}\nraw: {raw}")]
    ProtocolError { error: String, raw: String },
    #[error("PlugInFilter input is outside the supported subset: {0}")]
    UnsupportedFilterInput(#[from] PluginFilterInputError),
    #[error("PlugInFilter host rejected the request: {message}")]
    FilterRejected { message: String },
    #[error("PlugInFilter host I/O failure: {message}")]
    FilterIo { message: String },
    #[error("PlugInFilter did not call run(ip); no pixel processing was performed: {message}")]
    FilterNotProcessed { message: String },
    #[error("PlugInFilter class not found: {message}")]
    FilterClassNotFound { message: String },
    #[error("PlugInFilter threw {exception_type}: {message}")]
    FilterException {
        exception_type: String,
        message: String,
    },
    #[error(
        "PlugInFilter host did not produce an output image at {path}\nstdout: {stdout}\nstderr: {stderr}"
    )]
    MissingFilterOutput {
        path: PathBuf,
        stdout: String,
        stderr: String,
    },
}

/// Explicit rejection reasons for the narrow PlugInFilter compatibility spike.
///
/// These are checked before any Java process is started, so an unsupported
/// document is never silently downgraded by the TIFF transfer.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PluginFilterInputError {
    #[error("image type is not supported; `{title}` is {detail}")]
    UnsupportedImageType { title: String, detail: String },
    #[error(
        "image stacks must be plain single-channel Z-stacks; `{title}` has {slices} plane(s) with C/Z/T={n_channels}/{n_slices}/{n_frames}"
    )]
    UnsupportedStack {
        title: String,
        slices: usize,
        n_channels: u32,
        n_slices: u32,
        n_frames: u32,
    },
    #[error(
        "current slice {current_slice}/position {position:?} is inconsistent with the {slices}-plane stack of `{title}`"
    )]
    UnsupportedStackPosition {
        title: String,
        current_slice: u32,
        position: [u32; 3],
        slices: usize,
    },
    #[error("only in-bounds rectangular ROIs are supported; {detail}")]
    UnsupportedRoi { detail: String },
    #[error("only the transferable default metadata is supported; {detail}")]
    UnsupportedMetadata { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum PluginRunStatus {
    Success,
    ClassNotFound {
        message: String,
    },
    ExceptionThrown {
        exception_type: String,
        message: String,
        #[serde(default)]
        stack_trace: String,
    },
    UnsupportedPluginType {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRunOutput {
    #[serde(flatten)]
    pub status: PluginRunStatus,
    #[serde(default)]
    pub run_id: Option<String>,
    pub class_name: String,
    pub arg: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
}

impl PluginRunOutput {
    pub fn is_success(&self) -> bool {
        matches!(self.status, PluginRunStatus::Success)
    }

    pub fn is_class_not_found(&self) -> bool {
        matches!(self.status, PluginRunStatus::ClassNotFound { .. })
    }

    pub fn is_exception_thrown(&self) -> bool {
        matches!(self.status, PluginRunStatus::ExceptionThrown { .. })
    }

    pub fn is_unsupported_plugin_type(&self) -> bool {
        matches!(self.status, PluginRunStatus::UnsupportedPluginType { .. })
    }
}

#[derive(Debug, Clone, Default)]
pub struct PluginRunRequest {
    pub plugin_classpath: Vec<PathBuf>,
    pub class_name: String,
    pub arg: String,
    pub work_dir: Option<PathBuf>,
}

impl PluginRunRequest {
    pub fn new(class_name: impl Into<String>, arg: impl Into<String>) -> Self {
        Self {
            plugin_classpath: Vec::new(),
            class_name: class_name.into(),
            arg: arg.into(),
            work_dir: None,
        }
    }

    pub fn with_classpath_entry(mut self, path: impl Into<PathBuf>) -> Self {
        self.plugin_classpath.push(path.into());
        self
    }

    pub fn with_classpath_entries<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        for p in paths {
            self.plugin_classpath.push(p.into());
        }
        self
    }

    pub fn with_work_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.work_dir = Some(dir.into());
        self
    }
}

/// Interface categories reported by read-only class inspection.
///
/// [`Self::Internal`] is assigned by the Rust bridge for `ij.*` class names;
/// the Java host never needs to load those classes. Every other category is
/// reported by the host after `Class.forName(name, false, loader)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginInspectionKind {
    #[serde(rename = "plugin_filter")]
    PlugInFilter,
    #[serde(rename = "plugin")]
    PlugIn,
    #[serde(rename = "extended_plugin_filter")]
    ExtendedPlugInFilter,
    #[serde(rename = "ambiguous")]
    Ambiguous,
    #[serde(rename = "not_a_plugin")]
    NotAPlugin,
    #[serde(rename = "internal")]
    Internal,
}

/// Structured outcome of a read-only inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum PluginInspectionStatus {
    /// The class loaded without initialization and its interfaces are known.
    Inspected {
        kind: PluginInspectionKind,
        #[serde(default)]
        message: String,
    },
    /// `Class.forName` reported that the class itself is absent.
    ClassNotFound { message: String },
    /// Loading or linking failed for another reason (missing dependency,
    /// malformed bytecode, security manager, ...).
    LoadFailed {
        exception_type: String,
        message: String,
    },
    /// The bridge rejected the name before starting a host process. This
    /// prevents arbitrary text (flags, shell-like syntax, path separators,
    /// `macro:` prefixes) from being interpreted as a command.
    InvalidClassName { message: String },
}

/// Result of inspecting one class with a bounded host process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInspectionResult {
    #[serde(flatten)]
    pub status: PluginInspectionStatus,
    /// Class name actually looked up. Invalid input echoes the rejected text
    /// verbatim so callers can report it without it ever becoming a command.
    pub class_name: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
}

impl PluginInspectionResult {
    /// Interface category when the class was loaded successfully.
    pub fn kind(&self) -> Option<PluginInspectionKind> {
        match self.status {
            PluginInspectionStatus::Inspected { kind, .. } => Some(kind),
            _ => None,
        }
    }

    /// True only for a plain `ij.plugin.filter.PlugInFilter`.
    pub fn is_supported_filter(&self) -> bool {
        self.kind() == Some(PluginInspectionKind::PlugInFilter)
    }
}

/// Request for read-only class inspection.
///
/// Inspection uses the same bounded child-process lifecycle as
/// [`PluginBridge::run_plugin`], but the host loads the class with
/// `Class.forName(name, false, loader)` and only calls `isAssignableFrom`.
/// No constructor, static initializer, `setup`, `run`, or menu command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInspectionRequest {
    pub plugin_classpath: Vec<PathBuf>,
    pub class_name: String,
    pub work_dir: Option<PathBuf>,
}

impl PluginInspectionRequest {
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            plugin_classpath: Vec::new(),
            class_name: class_name.into(),
            work_dir: None,
        }
    }

    pub fn with_classpath_entry(mut self, path: impl Into<PathBuf>) -> Self {
        self.plugin_classpath.push(path.into());
        self
    }

    pub fn with_classpath_entries<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        for p in paths {
            self.plugin_classpath.push(p.into());
        }
        self
    }

    pub fn with_work_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.work_dir = Some(dir.into());
        self
    }
}

/// Maximum accepted class name length. Real binary names are far shorter; the
/// cap keeps malformed configuration text from producing enormous arguments.
const MAX_INSPECTION_CLASS_NAME_LEN: usize = 1024;

/// Accepts only Java binary names (`a.b.C$D`). This is deliberately narrower
/// than every name `Class.forName` accepts so configuration text cannot smuggle
/// flags, paths, `macro:` commands, or interface-argument syntax into the host.
fn validate_inspection_class_name(name: &str) -> Result<&str, String> {
    if name.is_empty() {
        return Err("class name is empty".to_string());
    }
    if name.len() > MAX_INSPECTION_CLASS_NAME_LEN {
        return Err(format!(
            "class name is longer than {MAX_INSPECTION_CLASS_NAME_LEN} characters"
        ));
    }
    let mut segment_start = true;
    for ch in name.chars() {
        if ch == '.' {
            if segment_start {
                return Err("class name contains an empty segment".to_string());
            }
            segment_start = true;
            continue;
        }
        let valid = if segment_start {
            ch.is_alphabetic() || ch == '_' || ch == '$'
        } else {
            ch.is_alphanumeric() || ch == '_' || ch == '$'
        };
        if !valid {
            return Err(format!("class name contains unsupported character {ch:?}"));
        }
        segment_start = false;
    }
    if segment_start {
        return Err("class name ends with a separator".to_string());
    }
    Ok(name)
}

/// Request for the narrow PlugInFilter path.
///
/// The image is transferred through a temporary TIFF and the returned image is
/// read back from a second TIFF. Supported inputs are 8-bit/16-bit/32-bit
/// grayscale and RGB images (single-plane or plain single-channel Z-stacks),
/// an optional in-bounds rectangular ROI, and transferable default
/// calibration/display-range metadata. Whether the plugin's `setup()` flags
/// cover the image type is checked by the Java host, mirroring
/// `PlugInFilterRunner.checkImagePlus`.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginFilterRequest {
    pub plugin_classpath: Vec<PathBuf>,
    pub class_name: String,
    pub arg: String,
    /// ImageJ's `commandName`; only affects labels/status text in ImageJ 1.54u.
    pub command: String,
    pub image: ImageDocument,
    pub work_dir: Option<PathBuf>,
}

impl PluginFilterRequest {
    pub fn new(
        class_name: impl Into<String>,
        arg: impl Into<String>,
        image: ImageDocument,
    ) -> Self {
        Self {
            plugin_classpath: Vec::new(),
            class_name: class_name.into(),
            arg: arg.into(),
            command: String::new(),
            image,
            work_dir: None,
        }
    }

    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = command.into();
        self
    }

    pub fn with_classpath_entry(mut self, path: impl Into<PathBuf>) -> Self {
        self.plugin_classpath.push(path.into());
        self
    }

    pub fn with_classpath_entries<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        for p in paths {
            self.plugin_classpath.push(p.into());
        }
        self
    }

    pub fn with_work_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.work_dir = Some(dir.into());
        self
    }
}

/// Successful result of the narrow PlugInFilter path.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginFilterRun {
    pub image: ImageDocument,
    pub class_name: String,
    pub arg: String,
    pub command: String,
    /// Flags returned by the filter's `setup`, as observed by the host.
    pub filter_flags: i32,
    /// Number of times ImageJ called `run(ip)`. A successful result is never
    /// returned with zero (that is reported as `FilterNotProcessed`).
    pub run_invocations: u32,
    /// Whether ImageJ called `setup("final", imp)` for FINAL_PROCESSING.
    pub final_setup_called: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
enum PluginFilterHostStatus {
    Success,
    ClassNotFound {
        message: String,
    },
    ExceptionThrown {
        exception_type: String,
        message: String,
        #[serde(default)]
        stack_trace: String,
    },
    UnsupportedPluginType {
        message: String,
    },
    UnsupportedFilter {
        message: String,
    },
    NotProcessed {
        message: String,
    },
    IoError {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PluginFilterHostOutput {
    #[serde(flatten)]
    status: PluginFilterHostStatus,
    #[serde(default)]
    run_id: Option<String>,
    class_name: String,
    arg: String,
    #[serde(default)]
    filter_flags: i32,
    #[serde(default)]
    image_width: u32,
    #[serde(default)]
    image_height: u32,
    #[serde(default)]
    run_invocations: u32,
    #[serde(default)]
    final_setup_called: bool,
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PluginInspectionHostOutput {
    #[serde(flatten)]
    status: PluginInspectionStatus,
    class_name: String,
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
}

fn next_run_id() -> String {
    static RUN_COUNTER: AtomicU64 = AtomicU64::new(1);
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

struct HostProcessOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

/// Kills the whole child process group on Unix. The direct child is put in its
/// own process group with `process_group(0)` before exec, so `-pid` also
/// reaches helper processes spawned by a wrapper script.
///
/// On non-Unix platforms only the direct child is killed. A detached
/// descendant can survive there and keep the inherited pipes open, which is
/// why output collection is bounded instead of joining the reader threads.
fn terminate_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
            libc::kill(pid, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

/// How long a completed (or killed) host gets to flush and close its pipes.
/// A detached descendant can inherit the pipes and outlive the process group,
/// so joining the reader threads unconditionally can block cancellation and
/// timeout forever. After this grace period we keep whatever was read and let
/// the reader threads drain/exit on their own when the last writer closes.
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_millis(300);

struct OutputCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
    finished: std::sync::mpsc::Receiver<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OutputCapture {
    fn spawn<R>(mut pipe: R) -> Self
    where
        R: Read + Send + 'static,
    {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&buffer);
        let (finished_tx, finished) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => sink
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .extend_from_slice(&chunk[..n]),
                    Err(ref err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let _ = finished_tx.send(());
        });
        Self {
            buffer,
            finished,
            thread: Some(thread),
        }
    }

    /// Waits until `deadline` for EOF, then returns everything read so far.
    /// The reader thread is deliberately abandoned on timeout: it stays
    /// blocked on the inherited pipe until the detached descendant exits and
    /// must not keep the caller (or the UI's Cancelling state) waiting.
    fn finish(mut self, deadline: Instant) -> Vec<u8> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if self.finished.recv_timeout(remaining).is_ok() {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
        self.buffer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

fn collect_output(stdout: OutputCapture, stderr: OutputCapture) -> (Vec<u8>, Vec<u8>) {
    // Give each stream its own grace window: one stream may be held open by a
    // detached descendant while the other already has the diagnostics we need.
    let stdout = stdout.finish(Instant::now() + OUTPUT_DRAIN_GRACE);
    let stderr = stderr.finish(Instant::now() + OUTPUT_DRAIN_GRACE);
    (stdout, stderr)
}

fn run_host_process(
    cmd: &mut Command,
    timeout: Duration,
    cancel: Option<&CancellationToken>,
) -> Result<HostProcessOutput, PluginBridgeError> {
    if cancel.is_some_and(CancellationToken::is_cancelled) {
        return Err(PluginBridgeError::Cancelled);
    }

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd.spawn()?;

    let stdout_pipe = child.stdout.take().expect("piped child stdout");
    let stderr_pipe = child.stderr.take().expect("piped child stderr");
    let stdout_capture = OutputCapture::spawn(stdout_pipe);
    let stderr_capture = OutputCapture::spawn(stderr_pipe);

    let start = Instant::now();
    let status = loop {
        // The cancellation check runs before `try_wait` so a token that was
        // cancelled while the child was starting still terminates it.
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            terminate_process_group(&mut child);
            let _ = child.wait();
            let _ = collect_output(stdout_capture, stderr_capture);
            return Err(PluginBridgeError::Cancelled);
        }

        match child.try_wait()? {
            Some(status) => break status,
            None => {
                if start.elapsed() >= timeout {
                    terminate_process_group(&mut child);
                    let _ = child.wait();
                    let _ = collect_output(stdout_capture, stderr_capture);
                    return Err(PluginBridgeError::Timeout {
                        timeout,
                        command: format!("{cmd:?}"),
                    });
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };

    let (stdout_bytes, stderr_bytes) = collect_output(stdout_capture, stderr_capture);

    Ok(HostProcessOutput {
        status,
        stdout: String::from_utf8_lossy(&stdout_bytes).to_string(),
        stderr: String::from_utf8_lossy(&stderr_bytes).to_string(),
    })
}

fn directory_has_newer_file(dir: &Path, threshold: SystemTime) -> std::io::Result<bool> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            if directory_has_newer_file(&entry.path(), threshold)? {
                return Ok(true);
            }
        } else if metadata.modified()? > threshold {
            return Ok(true);
        }
    }
    Ok(false)
}

static HOST_BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub struct PluginBridge {
    repo_root: PathBuf,
    runner_script: PathBuf,
    timeout: Duration,
}

impl PluginBridge {
    pub fn locate_from_repo_root(repo_root: impl AsRef<Path>) -> Result<Self, PluginBridgeError> {
        let repo = repo_root.as_ref().to_path_buf();
        let host_dir = repo.join("tools/plugin-host");
        let runner = host_dir.join("run_host.sh");
        let host_jar = host_dir.join("plugin-host.jar");
        let fixtures_jar = host_dir.join("fixtures/fixtures.jar");

        if !runner.exists() {
            return Err(PluginBridgeError::HostNotFound(runner));
        }

        let is_stale = || -> bool {
            let host_mtime = host_jar.metadata().and_then(|m| m.modified()).ok();
            let fixtures_mtime = fixtures_jar.metadata().and_then(|m| m.modified()).ok();
            match (host_mtime, fixtures_mtime) {
                (Some(hm), Some(fm)) => {
                    let host_src = host_dir.join("src");
                    if directory_has_newer_file(&host_src, hm).unwrap_or(true) {
                        return true;
                    }
                    if let Ok(entries) = std::fs::read_dir(host_dir.join("fixtures/src/fixtures")) {
                        for entry in entries.flatten() {
                            if let Ok(src_m) = entry.metadata().and_then(|m| m.modified()) {
                                if src_m > fm {
                                    return true;
                                }
                            }
                        }
                    }
                    false
                }
                _ => true,
            }
        };

        // If plugin-host.jar or fixtures.jar is missing or stale, trigger
        // build_host.sh. The process-global mutex avoids redundant concurrent
        // builds from tests in this process; build_host.sh also takes an
        // inter-process lock and publishes jars with atomic renames.
        if !host_jar.exists() || !fixtures_jar.exists() || is_stale() {
            let _build_guard = HOST_BUILD_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !host_jar.exists() || !fixtures_jar.exists() || is_stale() {
                let build_script = host_dir.join("build_host.sh");
                let build_output = Command::new(&build_script).current_dir(&repo).output()?;
                if !build_output.status.success() {
                    return Err(PluginBridgeError::BuildFailed(
                        String::from_utf8_lossy(&build_output.stderr).to_string(),
                    ));
                }
            }
        }

        Ok(Self {
            repo_root: repo,
            runner_script: runner,
            timeout: DEFAULT_PLUGIN_TIMEOUT,
        })
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn with_runner_script(mut self, runner: impl Into<PathBuf>) -> Self {
        self.runner_script = runner.into();
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    fn joined_plugin_classpath(
        &self,
        entries: &[PathBuf],
    ) -> Result<Option<std::ffi::OsString>, PluginBridgeError> {
        if entries.is_empty() {
            return Ok(None);
        }
        // Normalize classpath entries to absolute paths so relative paths
        // resolve reliably regardless of the child process working directory.
        let current = std::env::current_dir().unwrap_or_else(|_| self.repo_root.clone());
        let abs_entries: Vec<PathBuf> = entries
            .iter()
            .map(|p| {
                if p.is_relative() {
                    if current.join(p).exists() {
                        current.join(p)
                    } else if self.repo_root.join(p).exists() {
                        self.repo_root.join(p)
                    } else {
                        current.join(p)
                    }
                } else {
                    p.clone()
                }
            })
            .collect();
        let joined = std::env::join_paths(&abs_entries)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        Ok(Some(joined))
    }

    pub fn run_plugin(
        &self,
        request: &PluginRunRequest,
    ) -> Result<PluginRunOutput, PluginBridgeError> {
        self.run_plugin_with_timeout(request, self.timeout)
    }

    pub fn run_plugin_with_timeout(
        &self,
        request: &PluginRunRequest,
        timeout: Duration,
    ) -> Result<PluginRunOutput, PluginBridgeError> {
        self.run_plugin_with_timeout_and_cancel(request, timeout, None)
    }

    /// Runs a no-image plugin with cooperative cancellation. Cancellation
    /// terminates and reaps the child process group before returning
    /// `PluginBridgeError::Cancelled`.
    pub fn run_plugin_with_cancel(
        &self,
        request: &PluginRunRequest,
        cancel: &CancellationToken,
    ) -> Result<PluginRunOutput, PluginBridgeError> {
        self.run_plugin_with_timeout_and_cancel(request, self.timeout, Some(cancel))
    }

    pub fn run_plugin_with_timeout_and_cancel(
        &self,
        request: &PluginRunRequest,
        timeout: Duration,
        cancel: Option<&CancellationToken>,
    ) -> Result<PluginRunOutput, PluginBridgeError> {
        // Pre-cancellation wins before temp directories, classpath resolution
        // or a child process are touched.
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            return Err(PluginBridgeError::Cancelled);
        }

        let run_id = next_run_id();

        // Prepare isolated per-run protocol directory separate from caller work_dir
        let protocol_dir = tempfile::Builder::new()
            .prefix("ij_plugin_protocol_")
            .tempdir()?;
        let result_file = protocol_dir.path().join("result.json");

        // Prepare caller working directory (or isolated temporary directory if not provided)
        let _guard_work_dir;
        let work_dir = match &request.work_dir {
            Some(dir) => {
                if dir.is_relative() {
                    std::env::current_dir()?.join(dir)
                } else {
                    dir.to_path_buf()
                }
            }
            None => {
                _guard_work_dir = tempfile::Builder::new()
                    .prefix("ij_plugin_work_")
                    .tempdir()?;
                _guard_work_dir.path().to_path_buf()
            }
        };

        if !work_dir.exists() {
            std::fs::create_dir_all(&work_dir)?;
        }

        let mut cmd = Command::new(&self.runner_script);
        cmd.current_dir(&work_dir);

        if let Some(classpath) = self.joined_plugin_classpath(&request.plugin_classpath)? {
            cmd.arg("--plugin-cp").arg(classpath);
        }

        cmd.arg("--class").arg(&request.class_name);
        cmd.arg("--arg").arg(&request.arg);
        cmd.arg("--run-id").arg(&run_id);
        cmd.arg("--result-file").arg(&result_file);

        let process = run_host_process(&mut cmd, timeout, cancel)?;
        let code = process.status.code().unwrap_or(-1);

        // 1. Non-zero exit code cannot be reported as success, even if a result file was written
        if !process.status.success() {
            let diag = if !process.stderr.trim().is_empty() {
                process.stderr
            } else if !process.stdout.trim().is_empty() {
                process.stdout
            } else {
                format!("process exited with code {code}")
            };
            return Err(PluginBridgeError::ProcessFailed { code, stderr: diag });
        }
        let stdout = process.stdout;
        let stderr = process.stderr;

        // 2. Read protocol result file or fallback to stdout
        let output = if result_file.is_file() {
            let raw_json = std::fs::read_to_string(&result_file)?;
            serde_json::from_str::<PluginRunOutput>(&raw_json).map_err(|e| {
                PluginBridgeError::ProtocolError {
                    error: e.to_string(),
                    raw: raw_json,
                }
            })?
        } else if let Ok(output) = serde_json::from_str::<PluginRunOutput>(&stdout) {
            output
        } else {
            // Process exited with code 0 without producing protocol result (e.g. IJ.getImage() -> System.exit(0))
            return Err(PluginBridgeError::AbruptExit {
                code,
                stdout,
                stderr,
            });
        };

        // 3. Validate request / response identity
        if output.class_name != request.class_name {
            return Err(PluginBridgeError::IdentityMismatch {
                expected: format!("class_name={}", request.class_name),
                actual: format!("class_name={}", output.class_name),
            });
        }
        if output.arg != request.arg {
            return Err(PluginBridgeError::IdentityMismatch {
                expected: format!("arg={}", request.arg),
                actual: format!("arg={}", output.arg),
            });
        }
        if let Some(resp_run_id) = &output.run_id {
            if !resp_run_id.is_empty() && resp_run_id != &run_id {
                return Err(PluginBridgeError::IdentityMismatch {
                    expected: format!("run_id={run_id}"),
                    actual: format!("run_id={resp_run_id}"),
                });
            }
        }

        Ok(output)
    }

    /// Loads and classifies one plugin class without initializing or running it.
    pub fn inspect_plugin(
        &self,
        request: &PluginInspectionRequest,
    ) -> Result<PluginInspectionResult, PluginBridgeError> {
        self.inspect_plugin_with_timeout(request, self.timeout)
    }

    pub fn inspect_plugin_with_timeout(
        &self,
        request: &PluginInspectionRequest,
        timeout: Duration,
    ) -> Result<PluginInspectionResult, PluginBridgeError> {
        self.inspect_plugin_with_timeout_and_cancel(request, timeout, None)
    }

    /// Read-only inspection with cooperative cancellation. Cancellation kills
    /// and reaps the whole child process group before returning `Cancelled`.
    pub fn inspect_plugin_with_cancel(
        &self,
        request: &PluginInspectionRequest,
        cancel: &CancellationToken,
    ) -> Result<PluginInspectionResult, PluginBridgeError> {
        self.inspect_plugin_with_timeout_and_cancel(request, self.timeout, Some(cancel))
    }

    pub fn inspect_plugin_with_timeout_and_cancel(
        &self,
        request: &PluginInspectionRequest,
        timeout: Duration,
        cancel: Option<&CancellationToken>,
    ) -> Result<PluginInspectionResult, PluginBridgeError> {
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            return Err(PluginBridgeError::Cancelled);
        }

        let class_name = match validate_inspection_class_name(&request.class_name) {
            Ok(name) => name.to_string(),
            Err(message) => {
                return Ok(PluginInspectionResult {
                    status: PluginInspectionStatus::InvalidClassName { message },
                    class_name: request.class_name.clone(),
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
        };

        // `IJ.runPlugIn` routes `ij.*` to the built-in path; these are never
        // user plugin candidates and must not be loaded by the plugin loader.
        if class_name.starts_with("ij.") {
            return Ok(PluginInspectionResult {
                status: PluginInspectionStatus::Inspected {
                    kind: PluginInspectionKind::Internal,
                    message: "internal ImageJ class (ij.*); not a user plugin candidate"
                        .to_string(),
                },
                class_name,
                stdout: String::new(),
                stderr: String::new(),
            });
        }

        let run_id = next_run_id();

        let protocol_dir = tempfile::Builder::new()
            .prefix("ij_inspect_protocol_")
            .tempdir()?;
        let result_file = protocol_dir.path().join("result.json");

        let _guard_work_dir;
        let work_dir = match &request.work_dir {
            Some(dir) => {
                if dir.is_relative() {
                    std::env::current_dir()?.join(dir)
                } else {
                    dir.to_path_buf()
                }
            }
            None => {
                _guard_work_dir = tempfile::Builder::new()
                    .prefix("ij_inspect_work_")
                    .tempdir()?;
                _guard_work_dir.path().to_path_buf()
            }
        };
        if !work_dir.exists() {
            std::fs::create_dir_all(&work_dir)?;
        }

        let mut cmd = Command::new(&self.runner_script);
        cmd.current_dir(&work_dir);
        if let Some(classpath) = self.joined_plugin_classpath(&request.plugin_classpath)? {
            cmd.arg("--plugin-cp").arg(classpath);
        }
        cmd.arg("--inspect");
        cmd.arg("--class").arg(&class_name);
        cmd.arg("--run-id").arg(&run_id);
        cmd.arg("--result-file").arg(&result_file);

        let process = run_host_process(&mut cmd, timeout, cancel)?;
        let code = process.status.code().unwrap_or(-1);
        if !process.status.success() {
            let diag = if !process.stderr.trim().is_empty() {
                process.stderr
            } else if !process.stdout.trim().is_empty() {
                process.stdout
            } else {
                format!("process exited with code {code}")
            };
            return Err(PluginBridgeError::ProcessFailed { code, stderr: diag });
        }

        let output = if result_file.is_file() {
            let raw_json = std::fs::read_to_string(&result_file)?;
            serde_json::from_str::<PluginInspectionHostOutput>(&raw_json).map_err(|e| {
                PluginBridgeError::ProtocolError {
                    error: e.to_string(),
                    raw: raw_json,
                }
            })?
        } else if let Ok(output) =
            serde_json::from_str::<PluginInspectionHostOutput>(&process.stdout)
        {
            output
        } else {
            return Err(PluginBridgeError::AbruptExit {
                code,
                stdout: process.stdout,
                stderr: process.stderr,
            });
        };

        if output.class_name != class_name {
            return Err(PluginBridgeError::IdentityMismatch {
                expected: format!("class_name={class_name}"),
                actual: format!("class_name={}", output.class_name),
            });
        }

        Ok(PluginInspectionResult {
            status: output.status,
            class_name: output.class_name,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    /// Run a plain `PlugInFilter` in the narrow supported subset and return the
    /// processed image. Unsupported documents are rejected before any child
    /// JVM starts; a filter exception never yields a partial output image.
    pub fn run_filter(
        &self,
        request: &PluginFilterRequest,
    ) -> Result<PluginFilterRun, PluginBridgeError> {
        self.run_filter_with_timeout(request, self.timeout)
    }

    pub fn run_filter_with_timeout(
        &self,
        request: &PluginFilterRequest,
        timeout: Duration,
    ) -> Result<PluginFilterRun, PluginBridgeError> {
        self.run_filter_with_timeout_and_cancel(request, timeout, None)
    }

    /// Runs a `PlugInFilter` with cooperative cancellation. The token is
    /// checked before spawn and while waiting; cancellation kills and reaps the
    /// entire child process group before returning `Cancelled`.
    pub fn run_filter_with_cancel(
        &self,
        request: &PluginFilterRequest,
        cancel: &CancellationToken,
    ) -> Result<PluginFilterRun, PluginBridgeError> {
        self.run_filter_with_timeout_and_cancel(request, self.timeout, Some(cancel))
    }

    pub fn run_filter_with_timeout_and_cancel(
        &self,
        request: &PluginFilterRequest,
        timeout: Duration,
        cancel: Option<&CancellationToken>,
    ) -> Result<PluginFilterRun, PluginBridgeError> {
        // Check before validation and especially before the input TIFF write:
        // a pre-cancelled request must never do document I/O.
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            return Err(PluginBridgeError::Cancelled);
        }

        validate_filter_request(&request.image)?;

        let run_id = next_run_id();

        // The protocol directory is separate from the caller's work_dir, the
        // same isolation boundary used by the no-image PlugIn path.
        let protocol_dir = tempfile::Builder::new()
            .prefix("ij_filter_protocol_")
            .tempdir()?;
        let input_path = protocol_dir.path().join("input.tif");
        let output_path = protocol_dir.path().join("output.tif");
        let result_file = protocol_dir.path().join("result.json");

        // TIFF does not carry the ROI (the loader intentionally ignores ROI
        // metadata), so the rectangle is transferred explicitly via --roi.
        let mut transfer = request.image.clone();
        transfer.roi = None;
        transfer.snapshot = None;
        save_tiff(&input_path, &transfer)?;

        let _guard_work_dir;
        let work_dir = match &request.work_dir {
            Some(dir) => {
                if dir.is_relative() {
                    std::env::current_dir()?.join(dir)
                } else {
                    dir.to_path_buf()
                }
            }
            None => {
                _guard_work_dir = tempfile::Builder::new()
                    .prefix("ij_filter_work_")
                    .tempdir()?;
                _guard_work_dir.path().to_path_buf()
            }
        };
        if !work_dir.exists() {
            std::fs::create_dir_all(&work_dir)?;
        }

        let mut cmd = Command::new(&self.runner_script);
        cmd.current_dir(&work_dir);
        if let Some(classpath) = self.joined_plugin_classpath(&request.plugin_classpath)? {
            cmd.arg("--plugin-cp").arg(classpath);
        }
        cmd.arg("--class").arg(&request.class_name);
        cmd.arg("--arg").arg(&request.arg);
        cmd.arg("--command").arg(&request.command);
        if let Some(Roi::Rect(rect)) = &request.image.roi {
            cmd.arg("--roi").arg(format!(
                "{},{},{},{}",
                rect.x, rect.y, rect.width, rect.height
            ));
        }
        // The TIFF does not carry the current slice (FileSaver never writes
        // it), so ImagePlus's position is transferred explicitly. A filter
        // without DOES_STACKS processes exactly this slice.
        cmd.arg("--current-slice").arg(request.image.current_slice.to_string());
        cmd.arg("--input-image").arg(&input_path);
        cmd.arg("--output-image").arg(&output_path);
        cmd.arg("--run-id").arg(&run_id);
        cmd.arg("--result-file").arg(&result_file);

        let process = run_host_process(&mut cmd, timeout, cancel)?;
        let code = process.status.code().unwrap_or(-1);
        if !process.status.success() {
            let diag = if !process.stderr.trim().is_empty() {
                process.stderr
            } else if !process.stdout.trim().is_empty() {
                process.stdout
            } else {
                format!("process exited with code {code}")
            };
            return Err(PluginBridgeError::ProcessFailed { code, stderr: diag });
        }

        let output = if result_file.is_file() {
            let raw_json = std::fs::read_to_string(&result_file)?;
            serde_json::from_str::<PluginFilterHostOutput>(&raw_json).map_err(|e| {
                PluginBridgeError::ProtocolError {
                    error: e.to_string(),
                    raw: raw_json,
                }
            })?
        } else if let Ok(output) = serde_json::from_str::<PluginFilterHostOutput>(&process.stdout) {
            output
        } else {
            return Err(PluginBridgeError::AbruptExit {
                code,
                stdout: process.stdout,
                stderr: process.stderr,
            });
        };

        if output.class_name != request.class_name {
            return Err(PluginBridgeError::IdentityMismatch {
                expected: format!("class_name={}", request.class_name),
                actual: format!("class_name={}", output.class_name),
            });
        }
        if output.arg != request.arg {
            return Err(PluginBridgeError::IdentityMismatch {
                expected: format!("arg={}", request.arg),
                actual: format!("arg={}", output.arg),
            });
        }
        if let Some(resp_run_id) = &output.run_id {
            if !resp_run_id.is_empty() && resp_run_id != &run_id {
                return Err(PluginBridgeError::IdentityMismatch {
                    expected: format!("run_id={run_id}"),
                    actual: format!("run_id={resp_run_id}"),
                });
            }
        }

        match output.status {
            PluginFilterHostStatus::Success => {}
            PluginFilterHostStatus::ClassNotFound { message } => {
                return Err(PluginBridgeError::FilterClassNotFound { message });
            }
            PluginFilterHostStatus::ExceptionThrown {
                exception_type,
                message,
                ..
            } => {
                return Err(PluginBridgeError::FilterException {
                    exception_type,
                    message,
                });
            }
            PluginFilterHostStatus::UnsupportedPluginType { message }
            | PluginFilterHostStatus::UnsupportedFilter { message } => {
                return Err(PluginBridgeError::FilterRejected { message });
            }
            PluginFilterHostStatus::NotProcessed { message } => {
                return Err(PluginBridgeError::FilterNotProcessed { message });
            }
            PluginFilterHostStatus::IoError { message } => {
                return Err(PluginBridgeError::FilterIo { message });
            }
        }

        // Defensively enforce the protocol invariant even if a custom host
        // reports success without recording a run(ip) call.
        if output.run_invocations == 0 {
            return Err(PluginBridgeError::FilterNotProcessed {
                message: "host reported success but recorded zero run(ip) invocations".to_string(),
            });
        }

        if !output_path.is_file() {
            return Err(PluginBridgeError::MissingFilterOutput {
                path: output_path,
                stdout: process.stdout,
                stderr: process.stderr,
            });
        }

        let mut image = load_tiff(&output_path)?;
        let expected_kind = plane_kind(&request.image.slices[0]);
        let shapes_match = image.slices.len() == request.image.slices.len()
            && image.width == request.image.width
            && image.height == request.image.height
            && image.slices.iter().all(|plane| plane_kind(plane) == expected_kind);
        if !shapes_match {
            return Err(PluginBridgeError::FilterRejected {
                message: format!(
                    "filter output was not the same pixel type and stack shape as the input \
                     ({} planes of the input's type at {}x{})",
                    expected_kind_label(expected_kind),
                    request.image.width, request.image.height
                ),
            });
        }
        if image.calibration != request.image.calibration
            || image.display_range != request.image.display_range
        {
            return Err(PluginBridgeError::FilterRejected {
                message: "filter output metadata differs from the supported input metadata"
                    .to_string(),
            });
        }

        // Document state the Java run does not own is restored from the
        // request: TIFF does not carry title/selection/position, and the
        // current slice of the loaded stack is always 1.
        image.title = request.image.title.clone();
        image.roi = request.image.roi.clone();
        image.n_channels = request.image.n_channels;
        image.n_slices = request.image.n_slices;
        image.n_frames = request.image.n_frames;
        image.current_slice = request.image.current_slice;
        image.position = request.image.position;
        image.slice_labels = request.image.slice_labels.clone();
        image.threshold = request.image.threshold.clone();
        image.snapshot = None;
        image.snapshot_dimensions = None;
        image.snapshot_display_range = None;

        Ok(PluginFilterRun {
            image,
            class_name: output.class_name,
            arg: output.arg,
            command: request.command.clone(),
            filter_flags: output.filter_flags,
            run_invocations: output.run_invocations,
            final_setup_called: output.final_setup_called,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// Pixel format classification shared by the preflight and output checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterPlaneKind {
    Gray8,
    Gray16,
    Gray32,
    Rgb,
    Color256,
}

pub fn plane_kind(plane: &PixelPlane) -> FilterPlaneKind {
    match plane {
        PixelPlane::Gray8(_) => FilterPlaneKind::Gray8,
        PixelPlane::Color256 { .. } => FilterPlaneKind::Color256,
        PixelPlane::Gray16(_) => FilterPlaneKind::Gray16,
        PixelPlane::Gray32(_) => FilterPlaneKind::Gray32,
        PixelPlane::ColorRgb(_) => FilterPlaneKind::Rgb,
    }
}

/// Single preflight validator for the narrow `PlugInFilter` subset.
///
/// Both the bridge (immediately before TIFF transfer) and callers that only
/// want to know whether an image is compatible (the session and its UI) must
/// use this function so compatibility decisions cannot drift. The image type
/// itself (8/16/32-bit gray, RGB) is only checked for transferability here;
/// whether the plugin's `setup()` flags cover the image type is the Java
/// host's preflight, exactly like `PlugInFilterRunner.checkImagePlus`
/// (PlugInFilterRunner.java 379-427). In particular, ROI bounds are computed
/// in `i64` so `x + width` cannot overflow `i32`.
pub fn validate_filter_request(doc: &ImageDocument) -> Result<(), PluginFilterInputError> {
    // Only a plain single-channel Z-stack (or a single plane) can be
    // transferred: the multi-page TIFF opens as a 1 x N x 1 ImagePlus stack
    // (FileOpener.setCalibration, FileOpener.java 468-482). C or T
    // hyperstack dimensions cannot round-trip through it.
    let planes = doc.slices.len();
    if doc.n_channels != 1 || doc.n_frames != 1 || doc.n_slices as usize != planes {
        return Err(PluginFilterInputError::UnsupportedStack {
            title: doc.title.clone(),
            slices: planes,
            n_channels: doc.n_channels,
            n_slices: doc.n_slices,
            n_frames: doc.n_frames,
        });
    }
    if doc.current_slice < 1
        || doc.current_slice as usize > planes
        || doc.position != [1, doc.current_slice, 1]
    {
        return Err(PluginFilterInputError::UnsupportedStackPosition {
            title: doc.title.clone(),
            current_slice: doc.current_slice,
            position: doc.position,
            slices: planes,
        });
    }

    let first_kind = plane_kind(&doc.slices[0]);
    if first_kind == FilterPlaneKind::Color256 {
        return Err(PluginFilterInputError::UnsupportedImageType {
            title: doc.title.clone(),
            detail: pixel_plane_description(&doc.slices[0]).to_string(),
        });
    }
    if doc.slices.iter().any(|plane| plane_kind(plane) != first_kind) {
        return Err(PluginFilterInputError::UnsupportedImageType {
            title: doc.title.clone(),
            detail: "the stack mixes different pixel types".to_string(),
        });
    }

    if doc.calibration != Calibration::default() {
        return Err(PluginFilterInputError::UnsupportedMetadata {
            detail: format!(
                "calibration metadata ({} x {} {}, invert_y={}) cannot be preserved",
                doc.calibration.pixel_width,
                doc.calibration.pixel_height,
                doc.calibration.unit,
                doc.calibration.invert_y
            ),
        });
    }
    match first_kind {
        // FileSaver cannot store a non-default display range for RGB images
        // (FileSaver.java 775-784), and the 8-bit LUT/display contract keeps
        // its previous 0..255 requirement.
        FilterPlaneKind::Gray8 | FilterPlaneKind::Rgb => {
            let default_range = DisplayRange {
                min: 0.0,
                max: 255.0,
            };
            if doc.display_range != default_range {
                return Err(PluginFilterInputError::UnsupportedMetadata {
                    detail: format!(
                        "display range {}..{} is not the default 0..255 supported for 8-bit and RGB images",
                        doc.display_range.min, doc.display_range.max
                    ),
                });
            }
        }
        // 16/32-bit display ranges are written into the ImageDescription
        // (FileSaver.java 775-784), but FileOpener applies them as floats
        // (FileOpener.java 459-463), so only f32-representable bounds can
        // round-trip exactly.
        FilterPlaneKind::Gray16 | FilterPlaneKind::Gray32 => {
            let range = doc.display_range;
            if !range.min.is_finite()
                || !range.max.is_finite()
                || f64::from(range.min as f32) != range.min
                || f64::from(range.max as f32) != range.max
            {
                return Err(PluginFilterInputError::UnsupportedMetadata {
                    detail: format!(
                        "display range {}..{} is not finite and f32-representable; ImageJ \
                         stores 16/32-bit display ranges as floats and the round trip would \
                         silently change them",
                        range.min, range.max
                    ),
                });
            }
        }
        FilterPlaneKind::Color256 => unreachable!("rejected above"),
    }
    if let Some(levels) = &doc.threshold {
        return Err(PluginFilterInputError::UnsupportedMetadata {
            detail: format!(
                "active threshold {}..{} cannot be preserved by the narrow PlugInFilter subset",
                levels.lower, levels.upper
            ),
        });
    }

    match &doc.roi {
        None => {}
        Some(Roi::Rect(rect)) => {
            if rect.width <= 0 || rect.height <= 0 {
                return Err(PluginFilterInputError::UnsupportedRoi {
                    detail: format!(
                        "rectangle {}x{} at ({}, {}) has a non-positive dimension",
                        rect.width, rect.height, rect.x, rect.y
                    ),
                });
            }
            let right = i64::from(rect.x) + i64::from(rect.width);
            let bottom = i64::from(rect.y) + i64::from(rect.height);
            if rect.x < 0
                || rect.y < 0
                || right > i64::from(doc.width)
                || bottom > i64::from(doc.height)
            {
                return Err(PluginFilterInputError::UnsupportedRoi {
                    detail: format!(
                        "rectangle {}x{} at ({}, {}) is not fully inside the {}x{} image",
                        rect.width, rect.height, rect.x, rect.y, doc.width, doc.height
                    ),
                });
            }
        }
        Some(Roi::Oval(_)) => {
            return Err(PluginFilterInputError::UnsupportedRoi {
                detail: "oval selections are not supported".to_string(),
            });
        }
        Some(Roi::Polygon(_)) => {
            return Err(PluginFilterInputError::UnsupportedRoi {
                detail: "polygon selections are not supported".to_string(),
            });
        }
        Some(Roi::Line(_)) => {
            return Err(PluginFilterInputError::UnsupportedRoi {
                detail: "line selections are not supported".to_string(),
            });
        }
        Some(Roi::Point(_)) => {
            return Err(PluginFilterInputError::UnsupportedRoi {
                detail: "point selections are not supported".to_string(),
            });
        }
        Some(Roi::Angle(_)) => {
            return Err(PluginFilterInputError::UnsupportedRoi {
                detail: "angle selections are not supported".to_string(),
            });
        }
    }

    Ok(())
}

fn pixel_plane_description(plane: &PixelPlane) -> &'static str {
    match plane {
        PixelPlane::Gray8(_) => "8-bit grayscale",
        PixelPlane::Color256 { .. } => "8-bit indexed color (LUT)",
        PixelPlane::Gray16(_) => "16-bit grayscale",
        PixelPlane::Gray32(_) => "32-bit float grayscale",
        PixelPlane::ColorRgb(_) => "24-bit RGB color",
    }
}

fn expected_kind_label(kind: FilterPlaneKind) -> &'static str {
    match kind {
        FilterPlaneKind::Gray8 => "8-bit grayscale",
        FilterPlaneKind::Gray16 => "16-bit grayscale",
        FilterPlaneKind::Gray32 => "32-bit float grayscale",
        FilterPlaneKind::Rgb => "RGB color",
        FilterPlaneKind::Color256 => "8-bit indexed color (LUT)",
    }
}
