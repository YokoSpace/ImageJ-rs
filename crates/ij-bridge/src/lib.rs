use ij_core::ImageDocument;
use ij_io::{IoError, load_tiff, save_tiff};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use thiserror::Error;

pub const DEFAULT_ORACLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONCURRENT_ORACLE_PROCESSES: usize = 4;

static ORACLE_CONCURRENCY: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();

fn oracle_concurrency() -> &'static (Mutex<usize>, Condvar) {
    ORACLE_CONCURRENCY.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

struct OracleConcurrencyPermit;

impl Drop for OracleConcurrencyPermit {
    fn drop(&mut self) {
        let (lock, available) = oracle_concurrency();
        let mut active = lock.lock().unwrap();
        *active = active.saturating_sub(1);
        drop(active);
        available.notify_one();
    }
}

fn acquire_oracle_concurrency_permit() -> OracleConcurrencyPermit {
    let (lock, available) = oracle_concurrency();
    let mut active = lock.lock().unwrap();
    while *active >= MAX_CONCURRENT_ORACLE_PROCESSES {
        active = available.wait(active).unwrap();
    }
    *active += 1;
    OracleConcurrencyPermit
}

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TIFF exchange error: {0}")]
    Tiff(#[from] IoError),
    #[error("Java oracle script not found at {0}")]
    OracleNotFound(PathBuf),
    #[error("Java compatibility session timed out after {timeout:?}: {command}")]
    Timeout { timeout: Duration, command: String },
    #[error("Java oracle macro aborted: {message}\nstdout: {stdout}\nstderr: {stderr}")]
    MacroAborted {
        message: String,
        stdout: String,
        stderr: String,
    },
    #[error("Java compatibility session exited with status {status}: {stderr}")]
    ProcessFailed { status: i32, stderr: String },
    #[error(
        "Java oracle did not produce expected output image at {path}\nstdout: {stdout}\nstderr: {stderr}"
    )]
    MissingExpectedOutput {
        path: PathBuf,
        stdout: String,
        stderr: String,
    },
    #[error("Plugin bridge error: {0}")]
    Plugin(#[from] plugin::PluginBridgeError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OracleRunOutput {
    pub stdout: String,
    pub stderr: String,
    pub output_image: Option<ImageDocument>,
}

pub struct JavaOracleBridge {
    runner_script: PathBuf,
    timeout: Duration,
}

impl JavaOracleBridge {
    pub fn locate_from_repo_root(repo_root: impl AsRef<Path>) -> Result<Self, BridgeError> {
        let script = repo_root.as_ref().join("tools/oracle/run_oracle.sh");
        if !script.exists() {
            return Err(BridgeError::OracleNotFound(script));
        }
        Ok(Self {
            runner_script: script,
            timeout: DEFAULT_ORACLE_TIMEOUT,
        })
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

    pub fn eval_macro(&self, macro_code: &str) -> Result<OracleRunOutput, BridgeError> {
        self.eval_macro_with_timeout(macro_code, self.timeout)
    }

    pub fn eval_macro_with_timeout(
        &self,
        macro_code: &str,
        timeout: Duration,
    ) -> Result<OracleRunOutput, BridgeError> {
        let mut cmd = Command::new(&self.runner_script);
        cmd.arg("-eval").arg(macro_code).arg("-batch");
        self.run_command_with_timeout(&mut cmd, timeout)
    }

    fn run_command_with_timeout(
        &self,
        cmd: &mut Command,
        timeout: Duration,
    ) -> Result<OracleRunOutput, BridgeError> {
        let _permit = acquire_oracle_concurrency_permit();
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd.spawn()?;

        let mut stdout_pipe = child.stdout.take().expect("piped child stdout");
        let mut stderr_pipe = child.stderr.take().expect("piped child stderr");

        let stdout_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout_pipe.read_to_end(&mut buf);
            buf
        });

        let stderr_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut buf);
            buf
        });

        let start = Instant::now();
        let status = loop {
            match child.try_wait()? {
                Some(status) => break status,
                None => {
                    if start.elapsed() >= timeout {
                        // Kill child and all descendants in process group
                        let _ = child.kill();
                        #[cfg(unix)]
                        {
                            let pid = child.id() as i32;
                            unsafe {
                                libc::kill(-pid, libc::SIGKILL);
                                libc::kill(pid, libc::SIGKILL);
                            }
                        }
                        let _ = child.wait();
                        let _ = stdout_thread.join();
                        let _ = stderr_thread.join();
                        return Err(BridgeError::Timeout {
                            timeout,
                            command: format!("{cmd:?}"),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };

        let stdout_bytes = stdout_thread.join().unwrap_or_default();
        let stderr_bytes = stderr_thread.join().unwrap_or_default();

        let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

        if !status.success() {
            if stderr.contains("[oracle-macro-aborted]")
                || stderr.contains("[aborted]")
                || stdout.contains("[aborted]")
            {
                let message = if stderr.contains("[oracle-macro-aborted]") {
                    stderr.trim().to_string()
                } else if !stderr.trim().is_empty() {
                    stderr.trim().to_string()
                } else {
                    stdout.trim().to_string()
                };
                return Err(BridgeError::MacroAborted {
                    message,
                    stdout,
                    stderr,
                });
            }
            return Err(BridgeError::ProcessFailed {
                status: status.code().unwrap_or(-1),
                stderr,
            });
        }

        Ok(OracleRunOutput {
            stdout,
            stderr,
            output_image: None,
        })
    }

    pub fn run_macro_on_image(
        &self,
        doc: &ImageDocument,
        macro_body: &str,
    ) -> Result<OracleRunOutput, BridgeError> {
        self.run_macro_on_image_with_timeout(doc, macro_body, self.timeout)
    }

    pub fn run_macro_on_image_with_timeout(
        &self,
        doc: &ImageDocument,
        macro_body: &str,
        timeout: Duration,
    ) -> Result<OracleRunOutput, BridgeError> {
        let tmp_dir = tempfile::Builder::new().prefix("ij_bridge_").tempdir()?;
        let in_path = tmp_dir.path().join("input.tif");
        let out_path = tmp_dir.path().join("output.tif");

        save_tiff(&in_path, doc)?;
        let wrapped_macro = format!(
            "open(\"{}\");\n{}\nsaveAs(\"Tiff\", \"{}\");",
            in_path.display(),
            macro_body,
            out_path.display()
        );

        let mut run_out = self.eval_macro_with_timeout(&wrapped_macro, timeout)?;

        if !out_path.exists() {
            return Err(BridgeError::MissingExpectedOutput {
                path: out_path,
                stdout: run_out.stdout,
                stderr: run_out.stderr,
            });
        }

        let output_image = Some(load_tiff(&out_path)?);
        run_out.output_image = output_image;
        Ok(run_out)
    }
}

pub mod plugin;
pub mod plugin_discovery;
pub use plugin::{
    DEFAULT_PLUGIN_TIMEOUT, PluginBridge, PluginBridgeError, PluginInspectionKind,
    PluginInspectionRequest, PluginInspectionResult, PluginInspectionStatus, PluginRunOutput,
    PluginRunRequest, PluginRunStatus,
};
pub use plugin_discovery::{
    ArchiveReport, ConfigOrigin, DiscoveredPlugin, DiscoveryWarning, InspectionOptions,
    InspectionSummary, PluginCatalog, PluginDiscoveryError, PluginInspection,
    PluginInspectionFailure, PluginKind, PluginSource, ScanOptions,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    #[cfg(unix)]
    fn write_counting_runner(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let state = dir.join("state");
        std::fs::create_dir(&state).unwrap();
        let script = dir.join("counting-runner.sh");
        let state_text = state.display().to_string().replace('\'', "'\\''");
        let script_text = r#"#!/bin/sh
state='__STATE__'
touch "$state/start-$$"
tries=0
while [ ! -f "$state/release" ] && [ "$tries" -lt 1500 ]; do
    tries=$((tries + 1))
    sleep 0.01
done
exit 0
"#
        .replace("__STATE__", &state_text);
        std::fs::write(&script, script_text).unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        script
    }

    #[cfg(unix)]
    #[test]
    fn oracle_processes_are_bounded_to_four_concurrent_children() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_counting_runner(temp.path());
        let bridge = Arc::new(JavaOracleBridge {
            runner_script: script,
            timeout: Duration::from_secs(10),
        });
        let workers = 8;
        let barrier = Arc::new(Barrier::new(workers));
        let entered = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..workers {
            let bridge = Arc::clone(&bridge);
            let barrier = Arc::clone(&barrier);
            let entered = Arc::clone(&entered);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                entered.fetch_add(1, Ordering::SeqCst);
                bridge.eval_macro("ignored")
            }));
        }

        while entered.load(Ordering::SeqCst) < workers {
            std::thread::yield_now();
        }
        let state = temp.path().join("state");
        let count_started = || {
            std::fs::read_dir(&state)
                .unwrap()
                .filter(|entry| {
                    entry
                        .as_ref()
                        .is_ok_and(|entry| entry.file_name().to_string_lossy().starts_with("start-"))
                })
                .count()
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while count_started() < 4 {
            if Instant::now() >= deadline {
                std::fs::write(state.join("release"), "").unwrap();
                panic!("timed out waiting for the bounded batch");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_secs(1));
        let peak = count_started();
        std::fs::write(state.join("release"), "").unwrap();

        for handle in handles {
            handle.join().unwrap().unwrap();
        }
        assert_eq!(peak, 4, "oracle child process peak");
    }
}
