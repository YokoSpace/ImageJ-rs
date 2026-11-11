use super::*;
use serde::Serialize;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

impl ImageJApp {
    pub fn plugin_menu_generation(&self) -> u64 {
        self.plugin_catalog_generation
    }

    pub fn detect_repo_root() -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("IJ_REPO_ROOT") {
            let p = PathBuf::from(dir);
            if p.join("tools/plugin-host/run_host.sh").exists() {
                return Some(p);
            }
        }
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let p = PathBuf::from(manifest_dir);
            if let Some(parent) = p.parent().and_then(std::path::Path::parent) {
                if parent.join("tools/plugin-host/run_host.sh").exists() {
                    return Some(parent.to_path_buf());
                }
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            let mut curr: &std::path::Path = cwd.as_path();
            loop {
                if curr.join("tools/plugin-host/run_host.sh").exists() {
                    return Some(curr.to_path_buf());
                }
                match curr.parent() {
                    Some(p) => curr = p,
                    None => break,
                }
            }
        }
        None
    }

    pub fn try_detect_plugin_bridge(&mut self) -> bool {
        if self.session.plugin_bridge().is_some() {
            return true;
        }
        if let Some(repo_root) = Self::detect_repo_root() {
            if let Ok(bridge) = PluginBridge::locate_from_repo_root(repo_root) {
                self.session.set_plugin_bridge(bridge);
                return true;
            }
        }
        false
    }

    pub fn with_plugin_bridge(mut self, bridge: PluginBridge) -> Self {
        self.session.set_plugin_bridge(bridge);
        self
    }

    pub fn set_plugin_bridge(&mut self, bridge: PluginBridge) {
        self.session.set_plugin_bridge(bridge);
    }

    pub fn plugin_bridge(&self) -> Option<&PluginBridge> {
        self.session.plugin_bridge()
    }

    // -----------------------------------------------------------------
    // Installed Plugins browser (read-only discovery + explicit inspection)
    // -----------------------------------------------------------------

    /// Shows the independent Installed Plugins window.
    ///
    /// Opening is refused while a close/quit confirmation owns document state;
    /// the browser itself never mutates images and is not part of that prompt.
    pub fn open_plugins_browser(&mut self) -> bool {
        if self.pending_close.is_some() || self.quit_app {
            self.status_message =
                "Plugins browser blocked: resolve close confirmation first".to_string();
            return false;
        }
        self.plugins_browser.show = true;
        true
    }

    /// Hides the browser and discards any in-flight scan/inspection result.
    pub fn close_plugins_browser(&mut self) {
        let cancelled = self.cancel_plugin_catalog_job();
        self.plugins_browser.show = false;
        self.plugins_browser.selected = None;
        if cancelled {
            self.status_message = if self.is_plugin_catalog_busy() {
                "Plugins browser closed; waiting for Java inspection to stop".to_string()
            } else {
                "Plugins browser closed; late scan result was discarded".to_string()
            };
        }
    }

    pub fn is_plugin_catalog_busy(&self) -> bool {
        self.pending_plugin_catalog_job.is_some()
    }

    /// Cancels a scan immediately, or requests inspection cancellation while
    /// retaining the job until its child process has exited and been reaped.
    pub fn cancel_plugin_catalog_job(&mut self) -> bool {
        if let Some(PendingPluginCatalogJob::Inspect {
            index,
            cancel_token,
            ..
        }) = self.pending_plugin_catalog_job.as_ref()
        {
            if matches!(
                self.plugins_browser.inspect_phase,
                PluginInspectPhase::Cancelling { .. }
            ) {
                return false;
            }
            cancel_token.cancel();
            self.plugins_browser.inspect_phase = PluginInspectPhase::Cancelling { index: *index };
            self.plugins_browser.status =
                Some("Cancelling Java inspection; waiting for the child process to exit.".into());
            self.status_message = "Cancelling Java plugin inspection".into();
            return true;
        }
        let had_job = self.pending_plugin_catalog_job.take().is_some();
        if had_job {
            self.plugin_catalog_generation = self.plugin_catalog_generation.wrapping_add(1);
            self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
            self.plugins_browser.phase = if self.plugins_browser.catalog.is_some() {
                PluginCatalogPhase::Ready
            } else {
                PluginCatalogPhase::Idle
            };
            self.plugins_browser.status =
                Some("Catalog work cancelled; any late result will be discarded.".to_string());
            self.status_message =
                "Plugin scan/inspection cancelled; any late result will be discarded".to_string();
        }
        had_job
    }

    /// Starts a bounded, read-only scan in a worker thread.
    ///
    /// Scanning never starts a JVM and never reads class contents; every entry
    /// remains `PluginKind::Uninspected` until the user presses Inspect.
    pub fn start_plugin_directory_scan(
        &mut self,
        directory: impl Into<PathBuf>,
    ) -> Result<(), String> {
        if self.pending_close.is_some() || self.quit_app {
            return Err("Cannot scan while a close/quit confirmation is active.".to_string());
        }
        if self.is_plugin_catalog_busy() {
            return Err("A plugin scan or inspection is already running.".to_string());
        }
        // Consume the one-time startup load before replacing the catalog. A
        // later failed scan must not be followed by an old cache reappearing.
        self.load_plugin_catalog_cache_at_startup();
        let directory = directory.into();
        let display = directory.display().to_string();
        self.plugins_browser.directory_input = display.clone();
        self.plugin_catalog_generation = self.plugin_catalog_generation.wrapping_add(1);
        let generation = self.plugin_catalog_generation;
        self.plugins_browser.phase = PluginCatalogPhase::Scanning;
        self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
        self.plugins_browser.catalog = None;
        self.plugins_browser.source_block_reasons.clear();
        self.plugins_browser.selected = None;
        self.plugins_browser.scan_error = None;
        self.plugins_browser.status = Some(format!("Scanning {display}..."));
        self.status_message = format!("Scanning plugins directory: {display}");

        let options = bounded_plugin_scan_options();
        let (tx, rx) = channel();
        thread::spawn(move || {
            let result = PluginCatalog::scan_with_options(&directory, &options);
            let _ = tx.send(result);
        });
        self.pending_plugin_catalog_job = Some(PendingPluginCatalogJob::Scan {
            generation,
            receiver: rx,
            started: Instant::now(),
        });
        Ok(())
    }

    /// Scans the path currently typed into the browser.
    pub fn start_plugin_directory_scan_from_input(&mut self) -> Result<(), String> {
        let trimmed = self.plugins_browser.directory_input.trim();
        if trimmed.is_empty() {
            return Err("Enter a plugins directory path first.".to_string());
        }
        self.start_plugin_directory_scan(PathBuf::from(trimmed))
    }

    /// Explicitly inspects one catalog entry through the safe
    /// `Class.forName(name, false, loader)` bridge.
    ///
    /// Nothing happens implicitly: scanning leaves all entries Uninspected and
    /// only this user action starts the bounded plugin host.
    pub fn inspect_plugin_entry(&mut self, index: usize) -> Result<(), String> {
        if self.pending_close.is_some() || self.quit_app {
            return Err("Cannot inspect while a close/quit confirmation is active.".to_string());
        }
        if self.is_plugin_catalog_busy() {
            return Err("A plugin scan or inspection is already running.".to_string());
        }
        let (plugins_dir, classpath, plugin) = {
            let catalog =
                self.plugins_browser.catalog.as_ref().ok_or_else(|| {
                    "Scan a plugins directory before inspecting entries.".to_string()
                })?;
            if self.plugins_browser.phase != PluginCatalogPhase::Ready {
                return Err("The catalog is not ready for inspection.".to_string());
            }
            let plugin = catalog
                .plugins
                .get(index)
                .cloned()
                .ok_or_else(|| format!("No catalog entry at index {index}."))?;
            (
                catalog.plugins_dir.clone(),
                catalog.classpath.clone(),
                plugin,
            )
        };

        let bridge = match self.session.clone_plugin_bridge() {
            Some(bridge) => bridge,
            None => {
                if self.try_detect_plugin_bridge() {
                    self.session
                        .clone_plugin_bridge()
                        .expect("bridge just detected")
                } else {
                    return Err(
                        "Java plugin bridge is not configured; inspection needs the plugin host."
                            .to_string(),
                    );
                }
            }
        };

        // Inspect exactly this one entry by wrapping it in a minimal catalog.
        // `inspect_plugins_with_options` keeps the catalog's classpath, so the
        // bridge sees the same entries `PluginClassLoader` would.
        let mut probe = PluginCatalog {
            plugins_dir,
            classpath,
            plugins: vec![plugin],
            archives: Vec::new(),
            warnings: Vec::new(),
            warnings_truncated: false,
        };

        self.plugin_catalog_generation = self.plugin_catalog_generation.wrapping_add(1);
        let generation = self.plugin_catalog_generation;
        self.plugins_browser.inspect_phase = PluginInspectPhase::Inspecting { index };
        self.plugins_browser.status = Some(format!(
            "Inspecting catalog entry {index} with the bounded plugin host..."
        ));
        self.status_message = "Inspecting plugin class (read-only; no initializer/run)".to_string();

        let (tx, rx) = channel();
        let cancel_token = CancellationToken::new();
        let worker_cancel = cancel_token.clone();
        thread::spawn(move || {
            let summary = probe.inspect_plugins_with_options_and_cancel(
                &bridge,
                &InspectionOptions {
                    timeout: DEFAULT_PLUGIN_TIMEOUT,
                    reinspect: true,
                },
                Some(&worker_cancel),
            );
            let result = probe
                .plugins
                .into_iter()
                .next()
                .map(|plugin| (index, plugin, summary))
                .ok_or_else(|| "inspection produced no catalog entry".to_string());
            let _ = tx.send(result);
        });
        self.pending_plugin_catalog_job = Some(PendingPluginCatalogJob::Inspect {
            generation,
            index,
            receiver: rx,
            cancel_token,
            started: Instant::now(),
        });
        Ok(())
    }

    pub fn inspect_selected_plugin_entry(&mut self) -> Result<(), String> {
        let index = self
            .plugins_browser
            .selected
            .ok_or_else(|| "Select a catalog entry to inspect.".to_string())?;
        self.inspect_plugin_entry(index)
    }

    /// Polls the background catalog job once. Returns `true` when visible state
    /// changed (so the caller can request a repaint).
    pub fn poll_plugin_catalog_job(&mut self) -> bool {
        // The first UI frame performs the one-time startup load. Doing it here
        // (instead of in `new`) keeps unit tests that never render from reading
        // the developer's real config directory.
        if self.load_plugin_catalog_cache_at_startup() {
            return true;
        }
        let Some(job) = self.pending_plugin_catalog_job.take() else {
            return false;
        };
        match job {
            PendingPluginCatalogJob::Scan {
                generation,
                receiver,
                started,
            } => match receiver.try_recv() {
                Ok(Ok(catalog)) => {
                    if generation != self.plugin_catalog_generation {
                        return false;
                    }
                    let entries = catalog.plugins.len();
                    let warnings = catalog.warnings.len();
                    let truncated = catalog.warnings_truncated;
                    self.plugins_browser.catalog = Some(catalog);
                    self.plugins_browser.source_block_reasons = vec![None; entries];
                    self.plugins_browser.phase = PluginCatalogPhase::Ready;
                    self.plugins_browser.selected = None;
                    self.plugins_browser.scan_error = None;
                    self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
                    // Only a completed scan is persisted. Canceled, timed-out
                    // and failed jobs never reach this branch.
                    let cache_error = self.save_current_plugin_catalog_cache().err();
                    self.plugins_browser.status = Some(format!(
                        "Scan complete: {entries} entries, {warnings} warnings{}.{}",
                        if truncated {
                            " (warning limit reached)"
                        } else {
                            ""
                        },
                        cache_error
                            .map(|error| format!(" Cache save failed: {error}"))
                            .unwrap_or_default()
                    ));
                    self.status_message = format!(
                        "Plugin scan complete: {entries} entries, {warnings} warnings{}",
                        if truncated {
                            " (warning limit reached)"
                        } else {
                            ""
                        }
                    );
                    true
                }
                Ok(Err(error)) => {
                    if generation != self.plugin_catalog_generation {
                        return false;
                    }
                    self.plugins_browser.catalog = None;
                    self.plugins_browser.source_block_reasons.clear();
                    self.plugins_browser.phase = PluginCatalogPhase::Failed;
                    self.plugins_browser.selected = None;
                    self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
                    let message = error.to_string();
                    self.plugins_browser.scan_error = Some(message.clone());
                    self.plugins_browser.status = Some(format!("Scan failed: {message}"));
                    self.status_message = format!("Plugin scan failed: {message}");
                    true
                }
                Err(TryRecvError::Empty) => {
                    if started.elapsed() >= PLUGIN_SCAN_UI_BUDGET {
                        self.plugin_catalog_generation =
                            self.plugin_catalog_generation.wrapping_add(1);
                        self.plugins_browser.catalog = None;
                        self.plugins_browser.source_block_reasons.clear();
                        self.plugins_browser.phase = PluginCatalogPhase::Failed;
                        self.plugins_browser.selected = None;
                        self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
                        let message = format!(
                            "Scan exceeded the {:?} UI budget; the worker was abandoned and its late result will be discarded.",
                            PLUGIN_SCAN_UI_BUDGET
                        );
                        self.plugins_browser.scan_error = Some(message.clone());
                        self.plugins_browser.status = Some(message.clone());
                        self.status_message = message;
                        true
                    } else {
                        self.pending_plugin_catalog_job = Some(PendingPluginCatalogJob::Scan {
                            generation,
                            receiver,
                            started,
                        });
                        false
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    if generation != self.plugin_catalog_generation {
                        return false;
                    }
                    self.plugins_browser.catalog = None;
                    self.plugins_browser.source_block_reasons.clear();
                    self.plugins_browser.phase = PluginCatalogPhase::Failed;
                    self.plugins_browser.selected = None;
                    self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
                    let message =
                        "Catalog scan worker exited without reporting a result.".to_string();
                    self.plugins_browser.scan_error = Some(message.clone());
                    self.plugins_browser.status = Some(message.clone());
                    self.status_message = message;
                    true
                }
            },
            PendingPluginCatalogJob::Inspect {
                generation,
                index,
                receiver,
                cancel_token,
                started,
            } => match receiver.try_recv() {
                Ok(Ok((index, inspected, summary))) => {
                    if matches!(
                        self.plugins_browser.inspect_phase,
                        PluginInspectPhase::Cancelling { .. }
                    ) {
                        self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
                        self.plugins_browser.status = Some("Java inspection cancelled.".into());
                        self.status_message = "Java plugin inspection stopped".into();
                        return true;
                    }
                    if generation != self.plugin_catalog_generation {
                        return false;
                    }
                    let label = inspected.label.clone();
                    let kind = inspected.kind;
                    {
                        let Some(catalog) = self.plugins_browser.catalog.as_mut() else {
                            return false;
                        };
                        let Some(slot) = catalog.plugins.get_mut(index) else {
                            return false;
                        };
                        *slot = inspected;
                    }
                    if self.plugins_browser.selected.is_none() {
                        self.plugins_browser.selected = Some(index);
                    }
                    self.plugins_browser.inspect_phase = PluginInspectPhase::Done { index };
                    // Persist only a completed inspection. Cancellation,
                    // timeout and failure branches below never save.
                    let cache_error = self.save_current_plugin_catalog_cache().err();
                    self.plugins_browser.status = Some(format!(
                        "Inspected \"{label}\": {}. Inspected {}, supported {}, unsupported {}, internal {}, load-failed {}.{}",
                        plugin_kind_badge(kind),
                        summary.inspected,
                        summary.supported,
                        summary.unsupported,
                        summary.internal,
                        summary.load_failed,
                        cache_error
                            .map(|error| format!(" Cache save failed: {error}"))
                            .unwrap_or_default()
                    ));
                    self.status_message =
                        format!("Inspected \"{label}\": {}", plugin_kind_badge(kind));
                    true
                }
                Ok(Err(message)) => {
                    if matches!(
                        self.plugins_browser.inspect_phase,
                        PluginInspectPhase::Cancelling { .. }
                    ) {
                        self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
                        self.plugins_browser.status = Some("Java inspection cancelled.".into());
                        self.status_message = "Java plugin inspection stopped".into();
                        return true;
                    }
                    if generation != self.plugin_catalog_generation {
                        return false;
                    }
                    self.plugins_browser.inspect_phase = PluginInspectPhase::Failed {
                        index,
                        message: message.clone(),
                    };
                    self.plugins_browser.status = Some(format!("Inspection failed: {message}"));
                    self.status_message = format!("Plugin inspection failed: {message}");
                    true
                }
                Err(TryRecvError::Empty) => {
                    let timed_out = started.elapsed() >= PLUGIN_INSPECT_UI_BUDGET;
                    let newly_cancelling = timed_out
                        && !matches!(
                            self.plugins_browser.inspect_phase,
                            PluginInspectPhase::Cancelling { .. }
                        );
                    if newly_cancelling {
                        cancel_token.cancel();
                        self.plugins_browser.inspect_phase =
                            PluginInspectPhase::Cancelling { index };
                        self.plugins_browser.status = Some(
                            "Inspection exceeded the UI time budget; stopping its child process."
                                .into(),
                        );
                    }
                    self.pending_plugin_catalog_job = Some(PendingPluginCatalogJob::Inspect {
                        generation,
                        index,
                        receiver,
                        cancel_token,
                        started,
                    });
                    newly_cancelling
                }
                Err(TryRecvError::Disconnected) => {
                    if matches!(
                        self.plugins_browser.inspect_phase,
                        PluginInspectPhase::Cancelling { .. }
                    ) {
                        self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
                        self.plugins_browser.status = Some("Java inspection cancelled.".into());
                        self.status_message = "Java plugin inspection stopped".into();
                        return true;
                    }
                    if generation != self.plugin_catalog_generation {
                        return false;
                    }
                    let message =
                        "Plugin inspection worker exited without reporting a result.".to_string();
                    self.plugins_browser.inspect_phase = PluginInspectPhase::Failed {
                        index,
                        message: message.clone(),
                    };
                    self.plugins_browser.status = Some(message.clone());
                    self.status_message = message;
                    true
                }
            },
        }
    }

    /// Why the selected entry may not prefill the narrow dialog (computed
    /// against the image active *now*), or `None` when prefill is allowed.
    pub fn plugin_prefill_block_reason(&self, index: usize) -> Option<String> {
        // A close/quit confirmation owns document state. One-click runs must
        // not be offered while that modal is unresolved.
        if self.pending_close.is_some() || self.quit_app {
            return Some("Resolve the close/quit confirmation first.".to_string());
        }
        // A cached inspection is not proof that the source still exists. The
        // load step records this once so menu rendering does not stat every
        // classpath entry on every frame.
        if let Some(reason) = self
            .plugins_browser
            .source_block_reasons
            .get(index)
            .and_then(|reason| reason.clone())
        {
            return Some(reason);
        }
        let plugin = self
            .plugins_browser
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.plugins.get(index));
        let target = self.active_image_id();
        let preflight_error = target
            .and_then(|id| self.session.plugin_filter_preflight(id).err())
            .map(|error| error.to_string());
        plugin_prefill_block_reason(
            plugin,
            self.session.plugin_bridge().is_some(),
            self.is_plugin_filter_running(),
            target,
            preflight_error.as_deref(),
        )
    }

    /// Copies a verified `PluginKind::PlugInFilter` entry into the existing
    /// narrow PlugInFilter dialog and pins the currently active image.
    ///
    /// This never runs the plugin: it only fills the dialog, which re-checks
    /// preflight and requires the user to press Run Filter. Unsafe/Uninspected
    /// entries are rejected here as well as by the UI's disabled button.
    pub fn prefill_plugin_filter_from_catalog(&mut self, index: usize) -> Result<(), String> {
        if self.pending_close.is_some() || self.quit_app {
            return Err("Cannot open the PlugInFilter dialog during close/quit.".to_string());
        }
        if let Some(reason) = self.plugin_prefill_block_reason(index) {
            self.status_message = format!("Cannot prefill PlugInFilter dialog: {reason}");
            return Err(reason);
        }
        let (class_name, argument, classpath) = {
            let catalog = self
                .plugins_browser
                .catalog
                .as_ref()
                .ok_or_else(|| "Scan a plugins directory first.".to_string())?;
            let plugin = catalog
                .plugins
                .get(index)
                .ok_or_else(|| format!("No catalog entry at index {index}."))?;
            if !plugin.kind.is_supported_filter() {
                return Err(format!(
                    "Entry \"{}\" is {}; only verified PlugInFilter entries are supported.",
                    plugin.label,
                    plugin_kind_badge(plugin.kind)
                ));
            }
            let class_name = plugin
                .inspection
                .as_ref()
                .map(|inspection| inspection.class_name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| split_plugin_class_spec(&plugin.class_spec).0);
            let (_, argument) = split_plugin_class_spec(&plugin.class_spec);
            (class_name, argument, catalog.classpath.clone())
        };
        // Resolve the target once and use that same ID for the pin and the
        // dialog display. A document that disappeared during this frame is
        // rejected rather than silently retargeted to another window.
        let target_id = self
            .active_image_id()
            .ok_or_else(|| "No active image to pin as the PlugInFilter target.".to_string())?;
        if self.session.get_image(target_id).is_none() {
            return Err(format!(
                "Target image ID {} is no longer open; choose another image.",
                target_id.0
            ));
        }
        if let Err(error) = self.session.plugin_filter_preflight(target_id) {
            return Err(format!(
                "Target image ID {} failed preflight: {error}",
                target_id.0
            ));
        }

        self.dialogs.plugin_filter_class = class_name.clone();
        self.dialogs.plugin_filter_classpath = classpath;
        self.dialogs.plugin_filter_arg = argument.unwrap_or_default();
        self.dialogs.plugin_filter_target_id = Some(target_id);
        self.dialogs.show_plugin_filter = true;
        self.status_message = format!(
            "Prefilled narrow PlugInFilter dialog with {class_name} (target image ID {}); review and press Run Filter",
            target_id.0
        );
        Ok(())
    }

    /// Opens the PlugInFilter dialog pinned to the image that is active right
    /// now. Reopening after cancel/close must not reuse an old target.
    pub fn open_plugin_filter_dialog(&mut self) {
        self.dialogs.plugin_filter_target_id = self.active_image_id();
        self.dialogs.show_plugin_filter = true;
    }

    /// Closes or cancels the dialog and drops its pinned target so a later
    /// reopen pins the then-current image instead of a dangling one.
    pub fn dismiss_plugin_filter_dialog(&mut self) {
        self.dialogs.show_plugin_filter = false;
        self.dialogs.plugin_filter_target_id = None;
    }

    pub fn is_plugin_filter_running(&self) -> bool {
        self.pending_plugin_filter.is_some() || self.plugin_parameters.is_some()
    }

    pub fn is_plugin_filter_running_for(&self, id: ImageId) -> bool {
        self.plugin_parameters.as_ref().is_some_and(|p| p.target() == id)
            || self.pending_plugin_filter.as_ref().is_some_and(|p| p.target_image_id == id)
    }

    pub fn plugin_filter_phase(&self) -> Option<PluginFilterPhase> {
        self.pending_plugin_filter.as_ref().map(|p| p.phase)
    }

    pub fn is_plugin_filter_cancelling(&self) -> bool {
        self.plugin_filter_phase() == Some(PluginFilterPhase::Cancelling)
    }

    /// Requests cooperative cancellation. The pending entry is deliberately
    /// retained in the `Cancelling` phase until `poll_plugin_filter` observes
    /// that the child JVM has been killed and reaped. Returns `true` only for
    /// the first request.
    pub fn cancel_plugin_filter(&mut self) -> bool {
        if self.plugin_parameters.take().is_some() {
            self.status_message = "PlugInFilter parameters cancelled".into();
            return true;
        }
        let Some(pending) = self.pending_plugin_filter.as_mut() else {
            return false;
        };
        if pending.phase == PluginFilterPhase::Cancelling {
            return false;
        }
        pending.cancel_token.cancel();
        pending.phase = PluginFilterPhase::Cancelling;
        self.status_message = format!(
            "Cancelling PlugInFilter: {}... waiting for the child JVM to exit",
            pending.class_name
        );
        true
    }

    pub fn start_plugin_filter_task(
        &mut self,
        class_name: String,
        classpath: Vec<PathBuf>,
        arg: String,
    ) -> Result<(), SessionError> {
        let target_id = self.active_image_id().ok_or(SessionError::NoActiveImage)?;
        self.start_plugin_filter_task_for(target_id, class_name, classpath, arg)
    }

    /// Starts the run configured by the dialog using the target the dialog is
    /// currently displaying, not whatever image happens to be active.
    pub fn start_plugin_filter_from_dialog(&mut self) -> Result<(), SessionError> {
        let target_id = match self.dialogs.plugin_filter_target_id {
            Some(id) if self.session.get_image(id).is_some() => id,
            Some(id) => {
                let err = SessionError::InvalidPluginFilterRequest(format!(
                    "dialog target image ID {} is no longer open; reopen the dialog to choose a target",
                    id.0
                ));
                self.status_message = format!("{err}");
                self.dialogs.last_error = Some(format!("{err}"));
                return Err(err);
            }
            None => {
                let err = SessionError::InvalidPluginFilterRequest(
                    "no dialog target image is selected; reopen the dialog from an image window"
                        .to_string(),
                );
                self.status_message = format!("{err}");
                self.dialogs.last_error = Some(format!("{err}"));
                return Err(err);
            }
        };
        let class_name = self.dialogs.plugin_filter_class.clone();
        let classpath = self.dialogs.plugin_filter_classpath.clone();
        let arg = self.dialogs.plugin_filter_arg.clone();
        let result = self.start_plugin_filter_task_for(target_id, class_name, classpath, arg);
        if result.is_ok() {
            // The run consumes the pin; the dialog has been dismissed.
            self.dialogs.plugin_filter_target_id = None;
        }
        result
    }

    /// Starts a run on an explicit target. UI code must pass the image shown in
    /// the dialog so the displayed target and the execution target cannot
    /// diverge when the active window changes.
    pub fn start_plugin_filter_task_for(
        &mut self,
        target_id: ImageId,
        class_name: String,
        classpath: Vec<PathBuf>,
        arg: String,
    ) -> Result<(), SessionError> {
        if self.pending_close.is_some() || self.quit_app {
            let err = SessionError::InvalidPluginFilterRequest(
                "Cannot start PlugInFilter while close or quit confirmation is active".to_string(),
            );
            self.status_message = format!("{err}");
            return Err(err);
        }
        if self.is_plugin_filter_running() {
            let err = SessionError::InvalidPluginFilterRequest(
                "A PlugInFilter is already executing".to_string(),
            );
            self.status_message = format!("{err}");
            return Err(err);
        }

        if self.session.plugin_bridge().is_none() && !self.try_detect_plugin_bridge() {
            let err = SessionError::PluginBridgeUnavailable;
            self.status_message = "No plugin bridge is configured in this session".into();
            self.dialogs.last_error = Some(err.to_string());
            return Err(err);
        }

        let mut prepared =
            match self
                .session
                .prepare_plugin_filter(target_id, &class_name, &classpath, &arg)
            {
                Ok(prepared) => prepared,
                Err(err) => {
                    self.status_message = format!("{err}");
                    self.dialogs.last_error = Some(format!("{err}"));
                    return Err(err);
                }
            };

        if let Some(catalog) = &self.plugins_browser.catalog {
            if catalog.classpath == classpath {
                if let Some(entry) = catalog.plugins.iter().find(|entry| {
                    let (name, argument) = split_plugin_class_spec(&entry.class_spec);
                    name == class_name && argument.unwrap_or_default() == arg
                }) {
                    prepared.request.command = entry.label.clone();
                }
            }
        }
        self.start_prepared_plugin_filter(prepared)
    }

    pub(super) fn start_prepared_plugin_filter(
        &mut self,
        prepared: ij_session::PreparedPluginFilter,
    ) -> Result<(), SessionError> {
        if self.pending_close.is_some() || self.quit_app || self.pending_plugin_filter.is_some() {
            return Err(SessionError::InvalidPluginFilterRequest(
                "Resolve the active operation before running a filter".into(),
            ));
        }
        let bridge = self.session.clone_plugin_bridge()
            .ok_or(SessionError::PluginBridgeUnavailable)?;
        let ij_session::PreparedPluginFilter { ticket, request } = prepared;
        let target_id = ticket.target();
        let class_name = request.class_name.clone();
        let arg = request.arg.clone();
        let worker_request = request.clone();
        let cancel_token = CancellationToken::new();
        let worker_token = cancel_token.clone();
        let (tx, rx) = channel();

        thread::spawn(move || {
            let res = bridge
                .run_filter_with_cancel(&worker_request, &worker_token)
                .map_err(SessionError::Plugin);
            let _ = tx.send(res);
        });

        self.pending_plugin_filter = Some(PendingPluginFilter {
            target_image_id: target_id,
            class_name: class_name.clone(),
            arg,
            ticket,
            request,
            receiver: rx,
            cancel_token,
            phase: PluginFilterPhase::Running,
        });

        self.status_message = format!("Running PlugInFilter: {class_name}...");
        self.progress = Some(0.5);
        Ok(())
    }

    pub fn poll_plugin_filter(&mut self) -> Option<Result<PluginFilterOutcome, SessionError>> {
        let pending = self.pending_plugin_filter.as_ref()?;
        match pending.receiver.try_recv() {
            Ok(Ok(run)) => {
                let pending = self.pending_plugin_filter.take().expect("pending filter");
                self.progress = None;

                // A cancel may have been requested while the process was
                // finishing normally. The user's intent wins: discard output.
                if pending.phase == PluginFilterPhase::Cancelling {
                    return Some(Err(Self::plugin_filter_cancelled(&mut self.status_message)));
                }

                let target_id = pending.target_image_id;
                match self.session.apply_plugin_filter_run(pending.ticket, run) {
                    Ok(outcome) => {
                        self.texture_cache.remove(&target_id);
                        self.status_message =
                            format!("PlugInFilter {} completed successfully", outcome.class_name);
                        Some(Ok(outcome))
                    }
                    Err(err) => {
                        self.status_message =
                            format!("PlugInFilter {} error: {err}", pending.class_name);
                        self.dialogs.last_error = Some(format!("{err}"));
                        Some(Err(err))
                    }
                }
            }
            Ok(Err(err)) => {
                let pending = self.pending_plugin_filter.take().expect("pending filter");
                self.progress = None;
                let cancelled = pending.phase == PluginFilterPhase::Cancelling
                    || matches!(err, SessionError::Plugin(PluginBridgeError::Cancelled));
                if cancelled {
                    return Some(Err(Self::plugin_filter_cancelled(&mut self.status_message)));
                }
                if let SessionError::Plugin(PluginBridgeError::DialogRequired { schema }) = err {
                    self.plugin_parameters = Some(super::plugins_ui::PluginParameterDialog::new(
                        schema,
                        ij_session::PreparedPluginFilter {
                            ticket: pending.ticket,
                            request: pending.request,
                        },
                    ));
                    self.status_message = format!("Enter parameters for {}", pending.class_name);
                    self.dialogs.last_error = None;
                    return None;
                }
                self.status_message = format!("PlugInFilter {} failed: {err}", pending.class_name);
                self.dialogs.last_error = Some(format!("{err}"));
                Some(Err(err))
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                let pending = self.pending_plugin_filter.take().expect("pending filter");
                self.progress = None;
                if pending.phase == PluginFilterPhase::Cancelling {
                    return Some(Err(Self::plugin_filter_cancelled(&mut self.status_message)));
                }
                let err = SessionError::InvalidPluginFilterRequest(
                    "plugin worker exited without reporting a result".to_string(),
                );
                self.status_message = format!("PlugInFilter {} failed: {err}", pending.class_name);
                self.dialogs.last_error = Some(format!("{err}"));
                Some(Err(err))
            }
        }
    }

    fn plugin_filter_cancelled(status: &mut String) -> SessionError {
        *status = "PlugInFilter execution cancelled; image untouched".to_string();
        SessionError::Plugin(PluginBridgeError::Cancelled)
    }

    pub fn dispatch_plugin_filter_sync(
        &mut self,
        class_name: String,
        classpath: Vec<PathBuf>,
        arg: String,
    ) -> Result<PluginFilterOutcome, SessionError> {
        if let Some(err) = self.reject_while_closing("run PlugInFilter") {
            return Err(err);
        }
        if self.is_plugin_filter_running() {
            return Err(SessionError::InvalidPluginFilterRequest(
                "A PlugInFilter is already executing".to_string(),
            ));
        }

        let target_id = self.active_image_id().ok_or(SessionError::NoActiveImage)?;
        match self.session.dispatch(Command::RunPluginFilter {
            class_name,
            classpath,
            arg,
        }) {
            Ok(CommandOutcome::PluginFiltered(outcome)) => {
                self.texture_cache.remove(&target_id);
                self.status_message =
                    format!("PlugInFilter {} completed successfully", outcome.class_name);
                Ok(outcome)
            }
            Ok(other) => panic!("expected PluginFiltered outcome, got {other:?}"),
            Err(err) => {
                self.status_message = format!("PlugInFilter error: {err}");
                self.dialogs.last_error = Some(format!("{err}"));
                Err(err)
            }
        }
    }
}

/// One dynamic row under the `Plugins` menu.
///
/// This is a plain data snapshot so headless tests can assert exactly what the
/// menu would show without simulating OS-level menu clicks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginMenuEntry {
    pub index: usize,
    pub label: String,
    pub source: String,
    /// True only when `plugin_prefill_block_reason(index)` is `None`.
    pub enabled: bool,
    /// Concrete disabled-hover reason when `enabled` is false.
    pub reason: Option<String>,
}

/// Schema version of the JSON plugin-catalog cache.
///
/// Bump this whenever `PluginCatalog`'s persisted shape changes so an old file
/// is rejected as unsupported instead of being half-deserialized.
pub const PLUGIN_CATALOG_CACHE_SCHEMA_VERSION: u32 = 1;

/// Upper bound on a cache file read. The interactive scan options already
/// bound the catalog itself; this stops a corrupt or hostile file from being
/// read into memory before JSON parsing can reject it.
pub const PLUGIN_CATALOG_CACHE_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Explicit result of the one-time startup cache load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PluginCatalogCacheState {
    #[default]
    NotLoaded,
    /// No platform config path could be resolved.
    Disabled,
    Loaded {
        entries: usize,
    },
    Missing,
    Unreadable {
        message: String,
    },
    Corrupt {
        message: String,
    },
    TooLarge {
        size: u64,
        limit: u64,
    },
    UnsupportedSchema {
        found: u32,
        supported: u32,
    },
}

/// Result of reading a cache file. `catalog` is `None` for every fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCatalogCacheLoad {
    pub state: PluginCatalogCacheState,
    pub catalog: Option<PluginCatalog>,
}

#[derive(Debug, Serialize)]
struct PluginCatalogCacheEnvelopeRef<'a> {
    schema_version: u32,
    catalog: &'a PluginCatalog,
}

/// Stable per-user cache location. Never inside the repository and never next
/// to `original_imagej/`.
pub fn default_plugin_catalog_cache_path() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("IJ_PLUGIN_CATALOG_CACHE") {
        let path = PathBuf::from(value);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    let base = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
        })
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(base.join("imagej-rs").join("plugin_catalog.json"))
}

/// Writes the complete catalog (entries, sources, classpath and inspection
/// results) as a versioned JSON envelope.
///
/// The file is written to a temporary sibling and renamed into place so a
/// crashed write cannot leave a half-valid cache.
pub fn save_plugin_catalog_cache(path: &Path, catalog: &PluginCatalog) -> Result<(), String> {
    let envelope = PluginCatalogCacheEnvelopeRef {
        schema_version: PLUGIN_CATALOG_CACHE_SCHEMA_VERSION,
        catalog,
    };
    let json = serde_json::to_vec_pretty(&envelope)
        .map_err(|error| format!("serialize plugin catalog cache: {error}"))?;
    if json.len() as u64 > PLUGIN_CATALOG_CACHE_MAX_BYTES {
        return Err(format!(
            "plugin catalog cache is {} bytes; the {} byte limit would make it unreadable",
            json.len(),
            PLUGIN_CATALOG_CACHE_MAX_BYTES
        ));
    }
    write_plugin_catalog_cache_atomically(path, &json)
        .map_err(|error| format!("write plugin catalog cache {}: {error}", path.display()))
}

/// Reads a cache file. Missing, unreadable, corrupt and unsupported-schema
/// files all yield an empty fallback with an explicit state; none is an error
/// that can break startup.
pub fn load_plugin_catalog_cache(path: &Path) -> PluginCatalogCacheLoad {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return PluginCatalogCacheLoad {
                state: PluginCatalogCacheState::Missing,
                catalog: None,
            };
        }
        Err(error) => {
            return PluginCatalogCacheLoad {
                state: PluginCatalogCacheState::Unreadable {
                    message: error.to_string(),
                },
                catalog: None,
            };
        }
    };
    if metadata.len() > PLUGIN_CATALOG_CACHE_MAX_BYTES {
        return PluginCatalogCacheLoad {
            state: PluginCatalogCacheState::TooLarge {
                size: metadata.len(),
                limit: PLUGIN_CATALOG_CACHE_MAX_BYTES,
            },
            catalog: None,
        };
    }
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            return PluginCatalogCacheLoad {
                state: PluginCatalogCacheState::Unreadable {
                    message: error.to_string(),
                },
                catalog: None,
            };
        }
    };
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if let Err(error) = file
        .take(PLUGIN_CATALOG_CACHE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
    {
        return PluginCatalogCacheLoad {
            state: PluginCatalogCacheState::Unreadable {
                message: error.to_string(),
            },
            catalog: None,
        };
    }
    if bytes.len() as u64 > PLUGIN_CATALOG_CACHE_MAX_BYTES {
        return PluginCatalogCacheLoad {
            state: PluginCatalogCacheState::TooLarge {
                size: bytes.len() as u64,
                limit: PLUGIN_CATALOG_CACHE_MAX_BYTES,
            },
            catalog: None,
        };
    }
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            return PluginCatalogCacheLoad {
                state: PluginCatalogCacheState::Corrupt {
                    message: error.to_string(),
                },
                catalog: None,
            };
        }
    };
    let Some(found) = value.get("schema_version").and_then(|value| value.as_u64()) else {
        return PluginCatalogCacheLoad {
            state: PluginCatalogCacheState::Corrupt {
                message: "missing schema_version".to_string(),
            },
            catalog: None,
        };
    };
    if found != u64::from(PLUGIN_CATALOG_CACHE_SCHEMA_VERSION) {
        return PluginCatalogCacheLoad {
            state: PluginCatalogCacheState::UnsupportedSchema {
                found: u32::try_from(found).unwrap_or(u32::MAX),
                supported: PLUGIN_CATALOG_CACHE_SCHEMA_VERSION,
            },
            catalog: None,
        };
    }
    let Some(catalog_value) = value.get("catalog") else {
        return PluginCatalogCacheLoad {
            state: PluginCatalogCacheState::Corrupt {
                message: "missing catalog".to_string(),
            },
            catalog: None,
        };
    };
    let catalog: PluginCatalog = match serde_json::from_value(catalog_value.clone()) {
        Ok(catalog) => catalog,
        Err(error) => {
            return PluginCatalogCacheLoad {
                state: PluginCatalogCacheState::Corrupt {
                    message: error.to_string(),
                },
                catalog: None,
            };
        }
    };
    let bounds = bounded_plugin_scan_options();
    if catalog.plugins.len() > bounds.max_plugins || catalog.warnings.len() > bounds.max_warnings {
        return PluginCatalogCacheLoad {
            state: PluginCatalogCacheState::Corrupt {
                message: format!(
                    "cache exceeds scan bounds ({} entries, {} warnings; limits {} and {})",
                    catalog.plugins.len(),
                    catalog.warnings.len(),
                    bounds.max_plugins,
                    bounds.max_warnings
                ),
            },
            catalog: None,
        };
    }
    PluginCatalogCacheLoad {
        state: PluginCatalogCacheState::Loaded {
            entries: catalog.plugins.len(),
        },
        catalog: Some(catalog),
    }
}

fn write_plugin_catalog_cache_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "plugin catalog cache path has no file name",
        )
    })?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        nonce
    ));
    let write_result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            // Windows cannot replace an existing file with rename. Fall back to
            // remove+rename there; Unix keeps the atomic replace above.
            #[cfg(windows)]
            {
                if path.exists() {
                    let _ = fs::remove_file(path);
                    if fs::rename(&temporary, path).is_ok() {
                        return Ok(());
                    }
                }
            }
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

fn plugin_catalog_cache_state_message(state: &PluginCatalogCacheState) -> Option<&str> {
    match state {
        PluginCatalogCacheState::Unreadable { message }
        | PluginCatalogCacheState::Corrupt { message } => Some(message.as_str()),
        PluginCatalogCacheState::TooLarge { .. } => Some("cache file is too large"),
        PluginCatalogCacheState::UnsupportedSchema { .. } => {
            Some("unsupported cache schema version")
        }
        _ => None,
    }
}

impl ImageJApp {
    /// Replaces the cache location and consumes the one-time load state.
    pub fn set_plugin_catalog_cache_path(&mut self, path: Option<PathBuf>) {
        self.plugin_catalog_cache_path = path;
        self.plugin_catalog_cache_state = PluginCatalogCacheState::NotLoaded;
    }

    /// Loads the cache exactly once. Broken files leave an empty catalog and an
    /// explicit state; they never abort startup.
    pub fn load_plugin_catalog_cache_at_startup(&mut self) -> bool {
        if self.plugin_catalog_cache_state != PluginCatalogCacheState::NotLoaded {
            return false;
        }
        let Some(path) = self.plugin_catalog_cache_path.clone() else {
            self.plugin_catalog_cache_state = PluginCatalogCacheState::Disabled;
            return false;
        };
        if self.is_plugin_catalog_busy() {
            return false;
        }
        let load = load_plugin_catalog_cache(&path);
        self.plugin_catalog_cache_state = load.state.clone();
        let Some(catalog) = load.catalog else {
            if let Some(message) =
                plugin_catalog_cache_state_message(&self.plugin_catalog_cache_state)
            {
                self.plugins_browser.status = Some(format!(
                    "Ignored plugin catalog cache {}: {message}",
                    path.display()
                ));
            }
            return false;
        };
        if self.plugins_browser.catalog.is_some() {
            return false;
        }
        let entries = catalog.plugins.len();
        self.plugins_browser.directory_input = catalog.plugins_dir.display().to_string();
        let classpath_reason = crate::dialogs::plugin_classpath_missing_reason(&catalog.classpath);
        self.plugins_browser.source_block_reasons = catalog
            .plugins
            .iter()
            .map(|plugin| {
                crate::dialogs::plugin_source_file_missing_reason(plugin)
                    .or_else(|| classpath_reason.clone())
            })
            .collect();
        self.plugin_catalog_generation = self.plugin_catalog_generation.wrapping_add(1);
        self.plugins_browser.catalog = Some(catalog);
        self.plugins_browser.phase = PluginCatalogPhase::Ready;
        self.plugins_browser.selected = None;
        self.plugins_browser.scan_error = None;
        self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
        self.plugins_browser.status = Some(format!(
            "Loaded {entries} cached plugin entries from {}.",
            path.display()
        ));
        true
    }

    /// Starts a verified catalog entry with one click.
    ///
    /// This deliberately reuses the existing prefill + start path so the
    /// narrow dialog's preflight, target pinning, background worker,
    /// cancellation and stale-output validation are unchanged. The prefill
    /// dialog is dismissed immediately; no extra confirmation is shown.
    pub fn run_catalog_plugin_immediately(&mut self, index: usize) -> Result<(), String> {
        if self.pending_close.is_some() || self.quit_app {
            let reason =
                "Cannot run a plugin while a close/quit confirmation is active.".to_string();
            self.status_message = reason.clone();
            return Err(reason);
        }
        if self.is_plugin_filter_running() {
            let reason = "A Java PlugInFilter is already executing.".to_string();
            self.status_message = reason.clone();
            return Err(reason);
        }
        // Resolve the target once. The prefill step pins this exact image and
        // the start step consumes the pin, so a window switch cannot retarget
        // the run.
        let target_id = self.active_image_id().ok_or_else(|| {
            "Open and activate an image first; the plugin run pins that image as the target."
                .to_string()
        })?;
        if let Some(reason) = self.plugin_prefill_block_reason(index) {
            self.status_message = format!("Cannot run plugin: {reason}");
            return Err(reason);
        }
        if let Err(error) = self.prefill_plugin_filter_from_catalog(index) {
            self.dismiss_plugin_filter_dialog();
            return Err(error);
        }
        if self.dialogs.plugin_filter_target_id != Some(target_id) {
            self.dismiss_plugin_filter_dialog();
            let reason = "The active image changed while starting the plugin; no run was started."
                .to_string();
            self.status_message = reason.clone();
            return Err(reason);
        }
        let result = self.start_plugin_filter_from_dialog();
        // The one-click path never leaves the prefill dialog open, including
        // when the session rejects the request.
        self.dismiss_plugin_filter_dialog();
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let reason = error.to_string();
                self.status_message = format!("Cannot run plugin: {reason}");
                Err(reason)
            }
        }
    }

    fn save_current_plugin_catalog_cache(&mut self) -> Result<(), String> {
        let Some(path) = self.plugin_catalog_cache_path.clone() else {
            return Ok(());
        };
        let Some(catalog) = self.plugins_browser.catalog.as_ref() else {
            return Ok(());
        };
        save_plugin_catalog_cache(&path, catalog)?;
        self.plugin_catalog_cache_state = PluginCatalogCacheState::Loaded {
            entries: catalog.plugins.len(),
        };
        Ok(())
    }

    /// Snapshot of the dynamic `Plugins` menu rows.
    pub fn plugin_menu_entries(&self) -> Vec<PluginMenuEntry> {
        let Some(catalog) = self.plugins_browser.catalog.as_ref() else {
            return Vec::new();
        };
        catalog
            .plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| {
                let reason = self.plugin_prefill_block_reason(index);
                PluginMenuEntry {
                    index,
                    label: plugin.label.clone(),
                    source: plugin.source.to_string(),
                    enabled: reason.is_none(),
                    reason,
                }
            })
            .collect()
    }
}
