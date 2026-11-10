use crate::dialogs::{
    DialogState, PluginBrowserState, PluginCatalogPhase, PluginInspectPhase, plugin_kind_badge,
    plugin_kind_support_note, plugin_label_counts, plugin_prefill_block_reason, plugin_row_text,
    split_plugin_class_spec,
};
use crate::image_view::ImageView;
use crate::pixel_render::plane_to_rgba;
use crate::stack_process::{
    PendingStackProcess, StackOperation, StackProcessChoice, StackProcessOutcome,
};
use crate::tool::Tool;
use egui::{Color32, ColorImage, CornerRadius, Pos2, Rect, Sense, Stroke, TextureOptions, Vec2};
use ij_bridge::plugin::{
    CancellationToken, DEFAULT_PLUGIN_TIMEOUT, PluginBridge, PluginBridgeError, PluginFilterRun,
};
use ij_bridge::plugin_discovery::{
    DiscoveredPlugin, InspectionOptions, InspectionSummary, PluginCatalog, PluginDiscoveryError,
    ScanOptions,
};
use ij_core::{MeasurementSummary, PixelPlane, Roi};
use ij_session::{
    Command, CommandOutcome, ImageId, PluginFilterOutcome, PluginFilterTicket, Session,
    SessionError,
};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    Save,
    Discard,
    Cancel,
}

/// Default file name for a TIFF Save dialog.
///
/// ImageJ proposes the image title and only supplies a `.tif` extension when
/// the title does not already have one, so an image opened as `photo.tif` is
/// never offered as `photo.tif.tif` (original_imagej/ij/io/FileSaver.java
/// 86-94 and SaveDialog.java).
pub fn proposed_tiff_file_name(title: &str) -> String {
    let has_tiff_extension = std::path::Path::new(title)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("tif") || ext.eq_ignore_ascii_case("tiff"));
    if has_tiff_extension {
        title.to_string()
    } else {
        format!("{title}.tif")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseTarget {
    Single(ImageId),
    All,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingClose {
    pub target: CloseTarget,
    pub queue: Vec<ImageId>,
    pub current_prompt: Option<ImageId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginFilterPhase {
    Running,
    /// The user requested cancellation. The pending entry is retained until
    /// the child JVM has been killed and reaped, so close/re-run stay blocked.
    Cancelling,
}

pub struct PendingPluginFilter {
    pub target_image_id: ImageId,
    pub class_name: String,
    pub arg: String,
    pub ticket: PluginFilterTicket,
    pub receiver: Receiver<Result<PluginFilterRun, SessionError>>,
    pub cancel_token: CancellationToken,
    pub phase: PluginFilterPhase,
}

/// Upper bound on how long the UI waits for one background catalog scan.
///
/// `PluginCatalog::scan_with_options` is read-only and bounded by
/// [`bounded_plugin_scan_options`], but a slow filesystem can still stall it.
/// After this budget the UI abandons the worker (the thread cannot be killed),
/// rejects its late result by generation and reports the timeout.
pub const PLUGIN_SCAN_UI_BUDGET: Duration = Duration::from_secs(20);

/// Upper bound on how long the UI waits for one explicit entry inspection.
///
/// One entry gets one `PluginBridge::inspect_plugin_with_timeout` call bounded
/// by [`DEFAULT_PLUGIN_TIMEOUT`], plus process start/exit overhead.
pub const PLUGIN_INSPECT_UI_BUDGET: Duration = Duration::from_secs(25);

/// Bounds used for interactive scans. Smaller than the library defaults so a
/// mistaken giant folder cannot occupy the worker (or the UI's patience) for
/// an unbounded time.
pub fn bounded_plugin_scan_options() -> ScanOptions {
    ScanOptions {
        max_archive_bytes: 64 * 1024 * 1024,
        max_config_bytes: 256 * 1024,
        max_archive_entries: 20_000,
        max_plugins: 2_000,
        max_warnings: 500,
    }
}

/// Background work owned by the Plugins browser. Only one job may exist at a
/// time; starting a new scan invalidates a running inspection by generation.
pub enum PendingPluginCatalogJob {
    Scan {
        generation: u64,
        receiver: Receiver<Result<PluginCatalog, PluginDiscoveryError>>,
        started: Instant,
    },
    Inspect {
        generation: u64,
        index: usize,
        receiver: Receiver<Result<(usize, DiscoveredPlugin, InspectionSummary), String>>,
        cancel_token: CancellationToken,
        started: Instant,
    },
}

pub struct ImageJApp {
    pub session: Session,
    pub active_tool: Tool,
    pub status_message: String,
    pub progress: Option<f32>,
    pub image_views: BTreeMap<ImageId, ImageView>,
    pub show_results_window: bool,
    pub dialogs: DialogState,
    pub pending_close: Option<PendingClose>,
    pub pending_stack_process: Option<PendingStackProcess>,
    pub pending_plugin_filter: Option<PendingPluginFilter>,
    /// UI model for Plugins > Installed Plugins.
    pub plugins_browser: PluginBrowserState,
    /// At most one background catalog scan/inspection.
    pub pending_plugin_catalog_job: Option<PendingPluginCatalogJob>,
    /// Invalidates late worker results after cancel, timeout or a newer scan.
    plugin_catalog_generation: u64,
    pub quit_app: bool,
    texture_cache: HashMap<ImageId, (u64, egui::TextureHandle)>,
    pub texture_upload_count: u64,
    pub texture_cache_hit_count: u64,
}

impl Default for ImageJApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageJApp {
    pub fn new() -> Self {
        Self {
            session: Session::new(),
            active_tool: Tool::Rectangle,
            status_message: "ImageJ 1.54u ready".to_string(),
            progress: None,
            image_views: BTreeMap::new(),
            show_results_window: false,
            dialogs: DialogState::new(),
            pending_close: None,
            pending_stack_process: None,
            pending_plugin_filter: None,
            plugins_browser: PluginBrowserState::default(),
            pending_plugin_catalog_job: None,
            plugin_catalog_generation: 0,
            quit_app: false,
            texture_cache: HashMap::new(),
            texture_upload_count: 0,
            texture_cache_hit_count: 0,
        }
    }

    pub fn open_window_count(&self) -> usize {
        self.image_views.len()
    }

    pub fn active_image_id(&self) -> Option<ImageId> {
        self.session.active_image_id()
    }

    pub fn set_active_window(&mut self, id: ImageId) -> Result<(), SessionError> {
        self.session.set_active_image(id)
    }

    /// Activates the specified image in the session and requests viewport focus
    /// and un-minimization for its window.
    pub fn activate_image_window(
        &mut self,
        id: ImageId,
        ctx: &egui::Context,
    ) -> Result<(), SessionError> {
        self.set_active_window(id)?;
        let viewport_id = egui::ViewportId::from_hash_of(&("ij_image_window", id.0));
        ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Focus);
        Ok(())
    }

    pub fn close_window(&mut self, id: ImageId) -> bool {
        if self.is_plugin_filter_running_for(id) {
            self.status_message =
                "Cannot close window while Java PlugInFilter is running; cancel it first".into();
            return false;
        }
        if self
            .pending_stack_process
            .as_ref()
            .is_some_and(|pending| pending.image_id == id)
        {
            self.pending_stack_process = None;
        }
        self.image_views.remove(&id);
        if self.dialogs.plugin_filter_target_id == Some(id) {
            self.dialogs.plugin_filter_target_id = None;
        }
        self.texture_cache.remove(&id);
        self.session.close_image(id)
    }

    pub fn close_all_windows(&mut self) {
        if self.is_plugin_filter_running() {
            self.status_message =
                "Cannot close all windows while Java PlugInFilter is running; cancel it first"
                    .into();
            return;
        }
        self.pending_stack_process = None;
        self.image_views.clear();
        self.dialogs.plugin_filter_target_id = None;
        self.texture_cache.clear();
        self.session.close_all_images();
        self.status_message = "All windows closed".to_string();
    }

    pub fn texture_upload_count(&self) -> u64 {
        self.texture_upload_count
    }

    pub fn texture_cache_hit_count(&self) -> u64 {
        self.texture_cache_hit_count
    }

    pub fn is_texture_cached(&self, id: ImageId) -> bool {
        self.texture_cache.contains_key(&id)
    }

    pub fn cached_texture_revision(&self, id: ImageId) -> Option<u64> {
        self.texture_cache.get(&id).map(|(rev, _)| *rev)
    }

    pub fn cached_texture_id(&self, id: ImageId) -> Option<egui::TextureId> {
        self.texture_cache.get(&id).map(|(_, handle)| handle.id())
    }

    pub fn reset_texture_counters(&mut self) {
        self.texture_upload_count = 0;
        self.texture_cache_hit_count = 0;
    }

    pub fn prune_texture_cache(&mut self) {
        self.texture_cache
            .retain(|id, _| self.session.get_image(*id).is_some());
    }

    pub fn is_dirty(&self, id: ImageId) -> bool {
        self.session.is_dirty(id)
    }

    pub fn set_dirty(&mut self, id: ImageId, dirty: bool) {
        self.session.set_dirty(id, dirty);
    }

    pub fn any_dirty(&self) -> bool {
        self.session.any_dirty()
    }

    /// True while a modal close/quit confirmation is unresolved.
    pub fn close_prompt_active(&self) -> bool {
        self.pending_close.is_some()
    }

    /// Returns an error and updates the status bar when a document-changing
    /// action is attempted while the close/quit modal is open. The modal is the
    /// single source of truth for which documents are being resolved, so
    /// creating, opening or mutating documents underneath it would either lose
    /// the new work or invalidate the prompt's snapshot.
    fn reject_while_closing(&mut self, action: &str) -> Option<SessionError> {
        if self.is_plugin_filter_running() {
            let reason = "Java PlugInFilter is currently executing";
            self.status_message = format!("{action} blocked: {reason}");
            return Some(SessionError::InvalidPluginFilterRequest(format!(
                "{action} is unavailable: {reason}"
            )));
        }
        if self.pending_close.is_none() && !self.quit_app {
            return None;
        }
        let reason = if self.pending_close.is_some() {
            "resolve close confirmation first"
        } else {
            "application is exiting"
        };
        self.status_message = format!("{action} blocked: {reason}");
        Some(SessionError::UnknownCommand(format!(
            "{action} is unavailable: {reason}"
        )))
    }

    /// Requests closing a single window.
    /// If dirty, enters pending close confirmation guard and returns false (retaining window).
    /// If not dirty, closes immediately and returns true.
    /// While any prompt is open this is a deterministic no-op: it never
    /// replaces or downgrades the active prompt.
    pub fn request_close_window(&mut self, id: ImageId) -> bool {
        if self.is_plugin_filter_running_for(id) {
            self.status_message =
                "Close request ignored: Java PlugInFilter is executing on this image".into();
            return false;
        }
        if self.pending_close.is_some() {
            self.status_message =
                "Close request ignored: close confirmation is already open".into();
            return false;
        }
        if !self.is_dirty(id) {
            self.close_window(id);
            true
        } else {
            self.pending_close = Some(PendingClose {
                target: CloseTarget::Single(id),
                queue: vec![id],
                current_prompt: Some(id),
            });
            false
        }
    }

    /// Requests closing all windows.
    /// Clean windows are retained until the sequence is resolved, so Cancel
    /// preserves the entire document set. Dirty windows are prompted in order.
    /// Returns true if all windows were closed immediately with no pending confirmation.
    pub fn request_close_all(&mut self) -> bool {
        if self.is_plugin_filter_running() {
            self.status_message =
                "Close All ignored: Java PlugInFilter is currently executing".into();
            return false;
        }
        if self.pending_close.is_some() {
            self.status_message = "Close All ignored: close confirmation is already open".into();
            return false;
        }
        let dirty_queue = self.dirty_image_ids();
        if dirty_queue.is_empty() {
            self.close_all_windows();
            true
        } else {
            let first = dirty_queue[0];
            self.pending_close = Some(PendingClose {
                target: CloseTarget::All,
                queue: dirty_queue,
                current_prompt: Some(first),
            });
            false
        }
    }

    /// Requests quitting the application.
    /// Clean windows are retained until the sequence is resolved, so Cancel
    /// preserves every document. Dirty windows are prompted in order.
    /// Returns true if app can quit immediately.
    pub fn request_quit(&mut self) -> bool {
        if self.is_plugin_filter_running() {
            self.status_message =
                "Quit request ignored: Java PlugInFilter is currently executing; cancel it first"
                    .into();
            return false;
        }
        if self.pending_close.is_some() {
            // A repeated quit (Cmd-Q again, dock quit, root close) while the
            // modal is open must not rescan or rebuild the queue.
            self.status_message = "Quit request ignored: close confirmation is already open".into();
            return false;
        }
        if matches!(
            self.pending_plugin_catalog_job,
            Some(PendingPluginCatalogJob::Inspect { .. })
        ) {
            self.cancel_plugin_catalog_job();
            self.status_message =
                "Quit deferred: waiting for Java plugin inspection to stop; retry when it finishes"
                    .into();
            return false;
        }
        self.cancel_plugin_catalog_job();
        let dirty_queue = self.dirty_image_ids();
        if dirty_queue.is_empty() {
            self.close_all_windows();
            self.quit_app = true;
            crate::macos_bridge::set_quit_authorized(true);
            crate::macos_bridge::set_dirty_state(false);
            true
        } else {
            let first = dirty_queue[0];
            self.pending_close = Some(PendingClose {
                target: CloseTarget::Quit,
                queue: dirty_queue,
                current_prompt: Some(first),
            });
            crate::macos_bridge::set_quit_authorized(false);
            crate::macos_bridge::set_dirty_state(true);
            false
        }
    }

    /// Deterministic list of dirty documents (session order).
    fn dirty_image_ids(&self) -> Vec<ImageId> {
        self.session
            .image_ids()
            .into_iter()
            .filter(|&id| self.is_dirty(id))
            .collect()
    }

    /// Advances a close sequence after the prompted document was resolved.
    ///
    /// For Close All/Quit the whole session is re-checked before closing:
    /// a document created or dirtied after the prompt started must receive its
    /// own prompt rather than being discarded by the final close. A Single
    /// window close never touches other documents.
    fn advance_close_sequence(&mut self, mut pending: PendingClose) -> bool {
        if matches!(pending.target, CloseTarget::Single(_)) {
            self.pending_close = None;
            return true;
        }

        let dirty_remaining = self.dirty_image_ids();
        pending.queue = dirty_remaining.clone();
        if let Some(&next) = dirty_remaining.first() {
            pending.current_prompt = Some(next);
            self.pending_close = Some(pending);
            return false;
        }

        self.pending_close = None;
        self.close_all_windows();
        if pending.target == CloseTarget::Quit {
            self.quit_app = true;
            crate::macos_bridge::set_quit_authorized(true);
            crate::macos_bridge::set_dirty_state(false);
        }
        true
    }

    /// Cancels any pending close sequence. All pending and prompted documents are retained.
    pub fn confirm_pending_close_cancel(&mut self) {
        self.pending_close = None;
        self.status_message = "Close canceled".to_string();
        crate::macos_bridge::set_quit_authorized(false);
    }

    /// Discards unsaved changes for the currently prompted document and advances queue.
    /// Returns true if the full close sequence has finished.
    pub fn confirm_pending_close_discard(&mut self) -> bool {
        let Some(mut pending) = self.pending_close.take() else {
            return true;
        };
        let Some(curr_id) = pending.current_prompt else {
            return true;
        };

        if self.session.get_image(curr_id).is_some() {
            self.close_window(curr_id);
        }
        pending.queue.retain(|&id| id != curr_id);

        self.advance_close_sequence(pending)
    }

    /// Saves the currently prompted document to the given path.
    /// If path is None (user cancelled picker) or save fails, document is RETAINED and close is aborted.
    /// Returns Ok(true) if close sequence completed, Ok(false) if more pending or cancelled, or Err on write failure.
    pub fn confirm_pending_close_save(
        &mut self,
        path: Option<PathBuf>,
    ) -> Result<bool, SessionError> {
        let Some(pending) = self.pending_close.clone() else {
            return Ok(true);
        };
        let Some(curr_id) = pending.current_prompt else {
            return Ok(true);
        };

        let Some(path) = path else {
            // User cancelled the file picker dialog: retain document and the
            // still-unresolved prompt so closing remains guarded.
            self.status_message = "Save cancelled - document retained".to_string();
            crate::macos_bridge::set_quit_authorized(false);
            return Ok(false);
        };

        if let Err(e) = self.session.save_image(curr_id, &path) {
            // Failed save: retain document and prompt, abort close, surface error!
            self.status_message = format!("Save failed: {e} - document retained");
            self.dialogs.last_error = Some(format!("Failed to save \"{}\": {e}", path.display()));
            crate::macos_bridge::set_quit_authorized(false);
            return Err(e);
        }

        self.pending_close = None;
        self.status_message = format!("Saved \"{}\"", path.display());
        self.close_window(curr_id);
        let mut pending = pending;
        pending.queue.retain(|&id| id != curr_id);

        Ok(self.advance_close_sequence(pending))
    }

    /// Resolves the current close prompt at the document's remembered TIFF
    /// path, matching `FileSaver.save()` (original_imagej/ij/io/FileSaver.java
    /// 42-69). Returns `None` when the image was never opened from or saved to
    /// a file, so the caller must show a Save As picker instead.
    pub fn save_pending_close_to_remembered_path(&mut self) -> Option<Result<bool, SessionError>> {
        let id = self.pending_close.as_ref()?.current_prompt?;
        let path = self.session.image_path(id)?.to_path_buf();
        Some(self.confirm_pending_close_save(Some(path)))
    }

    pub fn open_ramp_8bit(
        &mut self,
        title: &str,
        width: u32,
        height: u32,
    ) -> Result<ImageId, SessionError> {
        if let Some(err) = self.reject_while_closing("New") {
            return Err(err);
        }
        let outcome = match self.session.dispatch(Command::NewRamp8Bit {
            title: title.to_string(),
            width,
            height,
        }) {
            Ok(o) => o,
            Err(e) => {
                self.status_message = format!("Create ramp failed: {e}");
                self.dialogs.last_error = Some(format!("Create ramp failed: {e}"));
                return Err(e);
            }
        };
        if let CommandOutcome::ImageOpened(id) = outcome {
            self.image_views.insert(id, ImageView::new(width, height));
            self.status_message = format!("Created ramp image \"{title}\" ({width}x{height})");
            Ok(id)
        } else {
            let err = SessionError::NoActiveImage;
            self.status_message = format!("Create ramp failed: {err}");
            self.dialogs.last_error = Some(format!("Create ramp failed: {err}"));
            Err(err)
        }
    }

    pub fn open_tiff_file(&mut self, path: PathBuf) -> Result<ImageId, SessionError> {
        if let Some(err) = self.reject_while_closing("Open") {
            return Err(err);
        }
        let outcome = match self
            .session
            .dispatch(Command::OpenTiff { path: path.clone() })
        {
            Ok(o) => o,
            Err(e) => {
                self.status_message = format!("Open failed: {e}");
                self.dialogs.last_error =
                    Some(format!("Could not open \"{}\": {e}", path.display()));
                return Err(e);
            }
        };
        if let CommandOutcome::ImageOpened(id) = outcome {
            if let Some(doc) = self.session.get_image(id) {
                self.image_views
                    .insert(id, ImageView::new(doc.width, doc.height));
                self.status_message = format!("Opened \"{}\"", path.display());
            }
            Ok(id)
        } else {
            let err = SessionError::NoActiveImage;
            self.status_message = format!("Open failed: {err}");
            self.dialogs.last_error = Some(format!("Could not open \"{}\": {err}", path.display()));
            Err(err)
        }
    }

    pub fn save_active_tiff(&mut self, path: PathBuf) -> Result<(), SessionError> {
        match self
            .session
            .dispatch(Command::SaveTiff { path: path.clone() })
        {
            Ok(_) => {
                self.status_message = format!("Saved \"{}\"", path.display());
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Save failed: {e}");
                self.dialogs.last_error =
                    Some(format!("Failed to save \"{}\": {e}", path.display()));
                Err(e)
            }
        }
    }

    pub fn save_results_tsv(&mut self, path: PathBuf) -> Result<(), SessionError> {
        match self
            .session
            .dispatch(Command::SaveResultsTsv { path: path.clone() })
        {
            Ok(_) => {
                self.status_message = format!("Saved results to \"{}\"", path.display());
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Save results failed: {e}");
                self.dialogs.last_error = Some(format!("Failed to save results: {e}"));
                Err(e)
            }
        }
    }

    pub fn dispatch_rect_roi(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("Rect ROI") {
            return Err(err);
        }
        match self.session.dispatch(Command::MakeRectRoi {
            x,
            y,
            width,
            height,
        }) {
            Ok(_) => {
                self.status_message = format!("Rect ROI: ({x}, {y}, {width}, {height})");
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Rect ROI failed: {e}");
                self.dialogs.last_error = Some(format!("Rect ROI failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn dispatch_oval_roi(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("Oval ROI") {
            return Err(err);
        }
        match self.session.dispatch(Command::MakeOvalRoi {
            x,
            y,
            width,
            height,
        }) {
            Ok(_) => {
                self.status_message = format!("Oval ROI: ({x}, {y}, {width}, {height})");
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Oval ROI failed: {e}");
                self.dialogs.last_error = Some(format!("Oval ROI failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn dispatch_polygon_roi(&mut self, points: Vec<(i32, i32)>) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("Polygon ROI") {
            return Err(err);
        }
        match self.session.dispatch(Command::MakePolygonRoi { points }) {
            Ok(_) => {
                self.status_message = "Polygon ROI created".to_string();
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Polygon ROI failed: {e}");
                self.dialogs.last_error = Some(format!("Polygon ROI failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn dispatch_line_roi(
        &mut self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
    ) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("Line ROI") {
            return Err(err);
        }
        match self
            .session
            .dispatch(Command::MakeLineRoi { x1, y1, x2, y2 })
        {
            Ok(_) => {
                self.status_message = format!("Line ROI: ({x1}, {y1}) -> ({x2}, {y2})");
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Line ROI failed: {e}");
                self.dialogs.last_error = Some(format!("Line ROI failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn clear_roi(&mut self) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("Clear selection") {
            return Err(err);
        }
        match self.session.dispatch(Command::ClearRoi) {
            Ok(_) => {
                self.status_message = "Selection cleared".to_string();
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Clear selection failed: {e}");
                self.dialogs.last_error = Some(format!("Clear selection failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn measure_active(&mut self) -> Result<MeasurementSummary, SessionError> {
        let outcome = match self.session.dispatch(Command::Measure) {
            Ok(o) => o,
            Err(e) => {
                self.status_message = format!("Measure failed: {e}");
                self.dialogs.last_error = Some(format!("Measure failed: {e}"));
                return Err(e);
            }
        };
        if let CommandOutcome::Measured(summary) = outcome {
            self.show_results_window = true;
            self.status_message = format!(
                "Measured: Area={:.1}, Mean={:.2}, Min={:.1}, Max={:.1}",
                summary.area, summary.mean, summary.min, summary.max
            );
            Ok(summary)
        } else {
            let err = SessionError::NoActiveImage;
            self.status_message = format!("Measure failed: {err}");
            self.dialogs.last_error = Some(format!("Measure failed: {err}"));
            Err(err)
        }
    }

    pub fn invert_active(&mut self) -> Result<(), SessionError> {
        self.begin_stack_aware(StackOperation::Invert)
    }

    /// Starts a filter or, for a multi-slice image, opens the ImageJ
    /// "Process Stack?" decision. A pending question deliberately performs no
    /// pixel work, so Cancel cannot dirty the document.
    fn begin_stack_aware(&mut self, operation: StackOperation) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing(operation.failure_label()) {
            return Err(err);
        }
        let Some(image_id) = self.active_image_id() else {
            return self.apply_current_slice(operation);
        };
        let stack_size = self
            .session
            .get_image(image_id)
            .map(|doc| doc.stack_size())
            .unwrap_or(1);
        if stack_size > 1 {
            self.pending_stack_process = Some(PendingStackProcess {
                image_id,
                operation,
                stack_size,
            });
            self.status_message = format!("Process all {stack_size} images?");
            return Ok(());
        }
        self.apply_current_slice(operation)
    }

    fn apply_current_slice(&mut self, operation: StackOperation) -> Result<(), SessionError> {
        match self.session.dispatch(operation.current_slice_command()) {
            Ok(_) => {
                self.status_message = operation.success_message();
                Ok(())
            }
            Err(e) => {
                let label = operation.failure_label();
                self.status_message = format!("{label} failed: {e}");
                self.dialogs.last_error = Some(format!("{label} failed: {e}"));
                Err(e)
            }
        }
    }

    /// Answers the pending "Process Stack?" question. `All` is only accepted
    /// when a verified whole-stack implementation exists; otherwise the
    /// pending question stays open and the limitation is reported instead of
    /// silently processing the current slice.
    pub fn resolve_stack_process(
        &mut self,
        choice: StackProcessChoice,
    ) -> Result<StackProcessOutcome, SessionError> {
        let Some(pending) = self.pending_stack_process.clone() else {
            return Ok(StackProcessOutcome::NothingPending);
        };

        if self.active_image_id() != Some(pending.image_id) {
            self.pending_stack_process = None;
            self.status_message = "Process stack canceled: active image changed".to_string();
            return Ok(StackProcessOutcome::Stale);
        }

        match choice {
            StackProcessChoice::Cancel => {
                self.pending_stack_process = None;
                self.status_message = "Process stack canceled".to_string();
                Ok(StackProcessOutcome::Cancelled)
            }
            StackProcessChoice::Current => {
                if let Some(err) = self.reject_while_closing("Process stack") {
                    return Err(err);
                }
                self.pending_stack_process = None;
                self.apply_current_slice(pending.operation)?;
                Ok(StackProcessOutcome::Applied)
            }
            StackProcessChoice::All => {
                if let Some(err) = self.reject_while_closing("Process stack") {
                    return Err(err);
                }
                let Some(command) = pending.operation.whole_stack_command() else {
                    self.status_message = pending
                        .operation
                        .whole_stack_limitation()
                        .unwrap_or_else(|| "Whole-stack processing is not implemented".into());
                    return Ok(StackProcessOutcome::UnsupportedWholeStack);
                };
                self.pending_stack_process = None;
                match self.session.dispatch(command) {
                    Ok(_) => {
                        self.status_message = pending
                            .operation
                            .whole_stack_success_message(pending.stack_size);
                        Ok(StackProcessOutcome::Applied)
                    }
                    Err(e) => {
                        self.status_message = format!("Invert Stack failed: {e}");
                        self.dialogs.last_error = Some(format!("Invert Stack failed: {e}"));
                        Err(e)
                    }
                }
            }
        }
    }

    pub fn undo_active(&mut self) -> Result<CommandOutcome, SessionError> {
        if let Some(err) = self.reject_while_closing("Undo") {
            return Err(err);
        }
        match self.session.dispatch(Command::Undo) {
            Ok(outcome @ CommandOutcome::Undone) => {
                self.status_message = "Undo applied".to_string();
                Ok(outcome)
            }
            Ok(outcome @ CommandOutcome::NothingToUndo) => {
                // ImageJ's Undo with no snapshot is a silent no-op
                // (Undo.java 121-132); do not claim an edit happened.
                self.status_message = "Nothing to undo".to_string();
                Ok(outcome)
            }
            Ok(outcome) => Ok(outcome),
            Err(e) => {
                self.status_message = format!("Undo failed: {e}");
                self.dialogs.last_error = Some(format!("Undo failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn smooth_active(&mut self) -> Result<(), SessionError> {
        self.begin_stack_aware(StackOperation::Smooth)
    }

    pub fn sharpen_active(&mut self) -> Result<(), SessionError> {
        self.begin_stack_aware(StackOperation::Sharpen)
    }

    pub fn find_edges_active(&mut self) -> Result<(), SessionError> {
        self.begin_stack_aware(StackOperation::FindEdges)
    }

    pub fn gaussian_blur_active(&mut self, sigma: f64) -> Result<(), SessionError> {
        self.begin_stack_aware(StackOperation::GaussianBlur { sigma })
    }

    pub fn median_active(&mut self, radius: f64) -> Result<(), SessionError> {
        self.begin_stack_aware(StackOperation::Median { radius })
    }

    pub fn minimum_active(&mut self, radius: f64) -> Result<(), SessionError> {
        self.begin_stack_aware(StackOperation::Minimum { radius })
    }

    pub fn maximum_active(&mut self, radius: f64) -> Result<(), SessionError> {
        self.begin_stack_aware(StackOperation::Maximum { radius })
    }

    pub fn despeckle_active(&mut self) -> Result<(), SessionError> {
        self.begin_stack_aware(StackOperation::Despeckle)
    }

    pub fn auto_threshold_active(
        &mut self,
        method: &str,
        dark_background: bool,
    ) -> Result<(), SessionError> {
        self.auto_threshold_active_with_options(method, dark_background, false)
    }

    pub fn auto_threshold_active_with_options(
        &mut self,
        method: &str,
        dark_background: bool,
        histogram_16: bool,
    ) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("AutoThreshold") {
            return Err(err);
        }
        let outcome = match self.session.dispatch(Command::AutoThreshold {
            method: method.to_string(),
            dark_background,
            histogram_16,
        }) {
            Ok(o) => o,
            Err(e) => {
                self.status_message = format!("AutoThreshold failed: {e}");
                self.dialogs.last_error = Some(format!("AutoThreshold failed: {e}"));
                return Err(e);
            }
        };
        if let CommandOutcome::Thresholded(levels) = outcome {
            let mode = if histogram_16 {
                " 16-bit histogram"
            } else {
                ""
            };
            self.status_message = format!(
                "AutoThreshold ({method}{mode}): lower={:.1}, upper={:.1}",
                levels.lower, levels.upper
            );
        }
        Ok(())
    }

    pub fn reset_threshold_active(&mut self) -> Result<CommandOutcome, SessionError> {
        if let Some(err) = self.reject_while_closing("Reset threshold") {
            return Err(err);
        }
        match self.session.dispatch(Command::ResetThreshold) {
            Ok(outcome) => {
                self.status_message = "Threshold reset".into();
                Ok(outcome)
            }
            Err(e) => {
                self.status_message = format!("Reset threshold failed: {e}");
                self.dialogs.last_error = Some(format!("Reset threshold failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn set_display_range_active(&mut self, min: f64, max: f64) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("Display range") {
            return Err(err);
        }
        match self.session.dispatch(Command::SetDisplayRange { min, max }) {
            Ok(_) => {
                self.status_message = format!("Display range: {min} - {max}");
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Display range failed: {e}");
                self.dialogs.last_error = Some(format!("Display range failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn set_slice_active(&mut self, index: i32) -> Result<(), SessionError> {
        // Slider/keyboard navigation must not be recorded; only the explicit
        // Image > Stack > Set Slice... command records setSlice(n).
        match self.session.dispatch(Command::NavigateSlice { index }) {
            Ok(_) => {
                self.status_message = format!("Slice {index}");
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Set slice failed: {e}");
                self.dialogs.last_error = Some(format!("Set slice failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn invert_stack_active(&mut self) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("Invert Stack") {
            return Err(err);
        }
        match self.session.dispatch(Command::InvertStack) {
            Ok(_) => {
                self.status_message = "Invert Stack applied".to_string();
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Invert Stack failed: {e}");
                self.dialogs.last_error = Some(format!("Invert Stack failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn update_status_for_pixel(&mut self, image_id: ImageId, x: i32, y: i32) {
        let sample = if let Some(doc) = self.session.get_image(image_id) {
            if x >= 0 && (x as u32) < doc.width && y >= 0 && (y as u32) < doc.height {
                let idx = (y as u32 * doc.width + x as u32) as usize;
                Some(doc.plane().sample_f64(idx))
            } else {
                None
            }
        } else {
            None
        };

        if let Some(val) = sample {
            self.status_message = format!("x={x}, y={y}, value={val}");
        }
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
        let directory = directory.into();
        let display = directory.display().to_string();
        self.plugins_browser.directory_input = display.clone();
        self.plugin_catalog_generation = self.plugin_catalog_generation.wrapping_add(1);
        let generation = self.plugin_catalog_generation;
        self.plugins_browser.phase = PluginCatalogPhase::Scanning;
        self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
        self.plugins_browser.catalog = None;
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
                    self.plugins_browser.phase = PluginCatalogPhase::Ready;
                    self.plugins_browser.selected = None;
                    self.plugins_browser.scan_error = None;
                    self.plugins_browser.inspect_phase = PluginInspectPhase::Idle;
                    self.plugins_browser.status = Some(format!(
                        "Scan complete: {entries} entries, {warnings} warnings{}.",
                        if truncated {
                            " (warning limit reached)"
                        } else {
                            ""
                        }
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
                    let Some(catalog) = self.plugins_browser.catalog.as_mut() else {
                        return false;
                    };
                    let Some(slot) = catalog.plugins.get_mut(index) else {
                        return false;
                    };
                    let label = inspected.label.clone();
                    let kind = inspected.kind;
                    *slot = inspected;
                    if self.plugins_browser.selected.is_none() {
                        self.plugins_browser.selected = Some(index);
                    }
                    self.plugins_browser.inspect_phase = PluginInspectPhase::Done { index };
                    self.plugins_browser.status = Some(format!(
                        "Inspected \"{label}\": {}. Inspected {}, supported {}, unsupported {}, internal {}, load-failed {}.",
                        plugin_kind_badge(kind),
                        summary.inspected,
                        summary.supported,
                        summary.unsupported,
                        summary.internal,
                        summary.load_failed
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
        self.pending_plugin_filter.is_some()
    }

    pub fn is_plugin_filter_running_for(&self, id: ImageId) -> bool {
        self.pending_plugin_filter
            .as_ref()
            .is_some_and(|p| p.target_image_id == id)
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

        let bridge = match self.session.clone_plugin_bridge() {
            Some(b) => b,
            None => {
                if self.try_detect_plugin_bridge() {
                    self.session.clone_plugin_bridge().unwrap()
                } else {
                    let err = SessionError::PluginBridgeUnavailable;
                    self.status_message =
                        "No plugin bridge is configured in this session".to_string();
                    self.dialogs.last_error = Some(
                        "Plugin bridge is unavailable. Auto-detect from repository root or configure one."
                            .to_string(),
                    );
                    return Err(err);
                }
            }
        };

        let prepared =
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

        let ij_session::PreparedPluginFilter { ticket, request } = prepared;
        let cancel_token = CancellationToken::new();
        let worker_token = cancel_token.clone();
        let (tx, rx) = channel();

        thread::spawn(move || {
            let res = bridge
                .run_filter_with_cancel(&request, &worker_token)
                .map_err(SessionError::Plugin);
            let _ = tx.send(res);
        });

        self.pending_plugin_filter = Some(PendingPluginFilter {
            target_image_id: target_id,
            class_name: class_name.clone(),
            arg,
            ticket,
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

/// Wraps popup menu contents in a vertical ScrollArea constrained to the available
/// vertical space in the root window viewport (such as the default 680x200 root window).
/// This prevents long menus, submenus, or open window lists from being clipped
/// by viewport bounds, keeping all items accessible and interactable via scroll.
pub fn scrollable_menu<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let screen_h = ui.ctx().screen_rect().height();
    let cursor_y = ui.cursor().top();
    let max_h = (screen_h - cursor_y - 8.0).max(60.0);
    egui::ScrollArea::vertical()
        .max_height(max_h)
        .auto_shrink([true, true])
        .show(ui, add_contents)
        .inner
}

impl eframe::App for ImageJApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_ui(ctx);
    }
}

impl ImageJApp {
    /// Handles keyboard shortcuts targeted at a specific image window.
    /// Uses key consumption (`consume_key`) so shortcuts execute exactly once
    /// without duplicate dispatches across viewports or frames.
    pub fn handle_image_shortcuts(&mut self, ctx: &egui::Context, target_id: ImageId) {
        // Respect text input focus: do not capture typing intended for TextEdit / DragValue
        if ctx.wants_keyboard_input() {
            return;
        }

        // Respect modal close/save confirmation, stack process prompts, and error dialogs
        if self.pending_close.is_some()
            || self.pending_stack_process.is_some()
            || self.dialogs.last_error.is_some()
        {
            return;
        }

        // Quit: Cmd+Q or Ctrl+Q
        let quit = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Q)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Q)
        });
        if quit {
            self.request_quit();
            return;
        }

        // Close All: Shift+W or Command/Ctrl+Shift+W
        let close_all = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::W,
            ) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::W)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::W)
        });
        if close_all {
            if self.open_window_count() > 0 {
                self.request_close_all();
            }
            return;
        }

        // Close: W or Command/Ctrl+W (without Shift)
        let close = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::W)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::W)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::W)
        });
        if close {
            self.request_close_window(target_id);
            return;
        }

        // Undo: Z or Command/Ctrl+Z (without Shift)
        let undo = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Z)
        });
        if undo {
            let _ = self.set_active_window(target_id);
            let _ = self.undo_active();
            return;
        }

        // Invert: Shift+I or Command/Ctrl+Shift+I
        let invert = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::I,
            ) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::I)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::I)
        });
        if invert {
            let _ = self.set_active_window(target_id);
            let _ = self.invert_active();
            return;
        }

        // Measure: M or Command/Ctrl+M (without Shift)
        let measure = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::M)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::M)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::M)
        });
        if measure {
            let _ = self.set_active_window(target_id);
            let _ = self.measure_active();
            return;
        }

        // Select None: Shift+A or Command/Ctrl+Shift+A
        let select_none = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::A,
            ) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::A)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::A)
        });
        if select_none {
            let _ = self.set_active_window(target_id);
            let _ = self.clear_roi();
            return;
        }

        // Zoom In: '+' or '='
        let zoom_in = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Plus)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Plus)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Plus)
                || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Equals)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Equals)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Equals)
        });
        if zoom_in {
            if let Some(view) = self.image_views.get_mut(&target_id) {
                view.zoom_in();
            }
            return;
        }

        // Zoom Out: '-'
        let zoom_out = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Minus)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Minus)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Minus)
        });
        if zoom_out {
            if let Some(view) = self.image_views.get_mut(&target_id) {
                view.zoom_out();
            }
            return;
        }

        // Stack Navigation
        let has_roi = self
            .session
            .get_image(target_id)
            .map(|doc| doc.roi.is_some())
            .unwrap_or(false);

        let next_slice = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Period)
                || (!has_roi && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight))
        });
        if next_slice {
            if let Some(doc) = self.session.get_image(target_id) {
                let sz = doc.stack_size();
                let cur = doc.current_slice;
                if sz > 1 && (cur as usize) < sz {
                    let _ = self.set_active_window(target_id);
                    let _ = self.set_slice_active((cur + 1) as i32);
                }
            }
            return;
        }

        let prev_slice = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Comma)
                || (!has_roi && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft))
        });
        if prev_slice {
            if let Some(doc) = self.session.get_image(target_id) {
                let sz = doc.stack_size();
                let cur = doc.current_slice;
                if sz > 1 && cur > 1 {
                    let _ = self.set_active_window(target_id);
                    let _ = self.set_slice_active((cur - 1) as i32);
                }
            }
        }
    }

    /// Handles advertised keyboard shortcuts for supported actions per ImageJ keyPressed and Menus.
    /// When invoked on the root toolbar viewport, acts on the active image window (if any).
    pub fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input()
            || self.pending_close.is_some()
            || self.pending_stack_process.is_some()
            || self.dialogs.last_error.is_some()
        {
            return;
        }

        // Quit: Cmd+Q or Ctrl+Q
        let quit = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Q)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Q)
        });
        if quit {
            self.request_quit();
            return;
        }

        // Close All: Shift+W or Command/Ctrl+Shift+W
        let close_all = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::W,
            ) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::W)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::W)
        });
        if close_all {
            if self.open_window_count() > 0 {
                self.request_close_all();
            }
            return;
        }

        // Forward to active image if present
        if let Some(active_id) = self.active_image_id() {
            self.handle_image_shortcuts(ctx, active_id);
        }
    }

    pub fn update_ui(&mut self, ctx: &egui::Context) {
        self.poll_plugin_filter();
        if self.is_plugin_filter_running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let catalog_changed = self.poll_plugin_catalog_job();
        if catalog_changed {
            ctx.request_repaint();
        }
        if self.is_plugin_catalog_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        crate::macos_bridge::install_macos_termination_bridge(Some(ctx));
        crate::macos_bridge::set_dirty_state(self.any_dirty());

        // Native OS termination requested (macOS Cmd-Q / App Menu Quit / Dock Quit)
        if crate::macos_bridge::poll_native_quit_requested() {
            if self.quit_app {
                crate::macos_bridge::set_quit_authorized(true);
            } else {
                let can_quit = self.request_quit();
                if !can_quit {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
            }
        }

        // Root window close requested (close route 5)
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.quit_app {
                // Quit already resolved: do not send CancelClose!
            } else {
                let can_quit = self.request_quit();
                if !can_quit {
                    // Deferred: dirty documents require user prompt.
                    // Must send CancelClose so eframe does not exit before user decides.
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                // When can_quit is true, quit is resolved immediately; do NOT send CancelClose!
            }
        }

        // Global keyboard shortcuts for supported actions on root viewport
        self.handle_keyboard_shortcuts(ctx);

        // Main Toolbar Window
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.render_menu_bar(ui);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_toolbar_tools(ui);
            ui.separator();
            self.render_status_bar(ui);
        });

        // Independent Image Windows via multi-viewport
        self.prune_texture_cache();
        let image_ids: Vec<ImageId> = self.session.image_ids();
        let mut close_requests = Vec::new();
        let mut focused_id = None;
        for &id in &image_ids {
            let doc_exists = self.session.get_image(id).is_some();
            if !doc_exists {
                continue;
            }

            let title = self
                .session
                .get_image(id)
                .map(|d| d.title.clone())
                .unwrap_or_else(|| format!("Image {}", id.0));

            let zoom = self.image_views.get(&id).map(|v| v.zoom).unwrap_or(1.0);
            let zoom_pct = (zoom * 100.0).round() as i32;

            let viewport_id = egui::ViewportId::from_hash_of(&("ij_image_window", id.0));

            let win_w = self
                .session
                .get_image(id)
                .map(|d| ((d.width as f32 * zoom as f32).max(200.0) + 30.0).min(1200.0))
                .unwrap_or(400.0);
            let win_h = self
                .session
                .get_image(id)
                .map(|d| ((d.height as f32 * zoom as f32).max(100.0) + 80.0).min(900.0))
                .unwrap_or(300.0);

            let mut close_this = false;
            ctx.show_viewport_immediate(
                viewport_id,
                egui::ViewportBuilder::default()
                    .with_title(format!("{title} ({zoom_pct}%)"))
                    .with_inner_size([win_w, win_h]),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if ctx.input(|i| i.viewport().focused == Some(true)) {
                        focused_id = Some(id);
                    }
                    self.handle_image_shortcuts(ctx, id);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        self.render_image_canvas_content(ui, id);
                    });
                    if !is_embedded && ctx.input(|i| i.viewport().close_requested()) {
                        if self.is_plugin_filter_running_for(id) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                            self.status_message =
                                "Cannot close window while Java PlugInFilter is running; cancel it first".into();
                        } else if self.pending_close.is_some() {
                            // While a close/quit prompt is open, ignore other
                            // window closes so the prompt's snapshot stays valid.
                            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        } else {
                            if self.is_dirty(id) {
                                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                            }
                            close_this = true;
                        }
                    }
                },
            );

            if close_this {
                close_requests.push(id);
            }
        }

        if let Some(f_id) = focused_id {
            let _ = self.set_active_window(f_id);
        }

        for id in close_requests {
            self.request_close_window(id);
        }

        // Render modal / secondary dialogs
        self.render_dialogs(ctx);

        // Results Table Window if opened
        if self.show_results_window {
            self.render_results_window(ctx);
        }

        if self.quit_app {
            self.cancel_plugin_catalog_job();
            crate::macos_bridge::set_quit_authorized(true);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn render_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                scrollable_menu(ui, |ui| {
                    ui.menu_button("New", |ui| {
                        scrollable_menu(ui, |ui| {
                            if ui.button("Image... (8-bit Ramp)").clicked() {
                                let _ = self.open_ramp_8bit("ramp", 256, 256);
                                ui.close_menu();
                            }
                        });
                    });

                    if ui.button("Open...").clicked() {
                        ui.close_menu();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("TIFF Image", &["tif", "tiff"])
                            .pick_file()
                        {
                            let _ = self.open_tiff_file(path);
                        }
                    }

                    let has_active = self.active_image_id().is_some();
                    if ui
                        .add_enabled(has_active, egui::Button::new("Save As... Tiff"))
                        .clicked()
                    {
                        ui.close_menu();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("TIFF Image", &["tif", "tiff"])
                            .save_file()
                        {
                            let _ = self.save_active_tiff(path);
                        }
                    }

                    if ui
                        .add_enabled(
                            !self.session.results.rows.is_empty(),
                            egui::Button::new("Save As... Results"),
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("TSV Table", &["tsv", "txt"])
                            .save_file()
                        {
                            let _ = self.save_results_tsv(path);
                        }
                    }

                    ui.separator();

                    if ui
                        .add_enabled(has_active, egui::Button::new("Close\tW"))
                        .clicked()
                    {
                        if let Some(id) = self.active_image_id() {
                            self.request_close_window(id);
                        }
                        ui.close_menu();
                    }

                    if ui
                        .add_enabled(
                            self.open_window_count() > 0,
                            egui::Button::new("Close All\tShift+W"),
                        )
                        .clicked()
                    {
                        self.request_close_all();
                        ui.close_menu();
                    }

                    ui.separator();

                    // Unsupported classic actions visibly disabled per guardrail
                    let _ = ui.add_enabled(false, egui::Button::new("Revert (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Page Setup... (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Print... (unsupported)"));

                    ui.separator();
                    let quit_shortcut = if cfg!(target_os = "macos") {
                        "Quit\tCmd+Q"
                    } else {
                        "Quit\tCtrl+Q"
                    };
                    if ui.button(quit_shortcut).clicked() {
                        self.request_quit();
                        ui.close_menu();
                    }
                });
            });

            ui.menu_button("Edit", |ui| {
                scrollable_menu(ui, |ui| {
                    let has_active = self.active_image_id().is_some();
                    if ui
                        .add_enabled(has_active, egui::Button::new("Undo\tZ"))
                        .clicked()
                    {
                        let _ = self.undo_active();
                        ui.close_menu();
                    }

                    ui.separator();

                    // Unsupported clipboard actions
                    let _ = ui.add_enabled(false, egui::Button::new("Cut\tX (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Copy\tC (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Paste\tV (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Clear (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Fill\tF (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Draw\tD (unsupported)"));

                    ui.separator();

                    if ui
                        .add_enabled(has_active, egui::Button::new("Invert\tShift+I"))
                        .clicked()
                    {
                        let _ = self.invert_active();
                        ui.close_menu();
                    }

                    ui.separator();

                    ui.menu_button("Selection", |ui| {
                        scrollable_menu(ui, |ui| {
                            if ui
                                .add_enabled(has_active, egui::Button::new("Select None\tShift+A"))
                                .clicked()
                            {
                                let _ = self.clear_roi();
                                ui.close_menu();
                            }
                        });
                    });
                });
            });

            ui.menu_button("Image", |ui| {
                scrollable_menu(ui, |ui| {
                    let has_active = self.active_image_id().is_some();
                    ui.menu_button("Adjust", |ui| {
                        scrollable_menu(ui, |ui| {
                            let can_bc = self
                                .active_image_id()
                                .and_then(|id| self.session.get_image(id))
                                .map(|d| !matches!(d.plane(), PixelPlane::ColorRgb(_)))
                                .unwrap_or(false);

                            let bc_btn = egui::Button::new("Brightness/Contrast...");
                            let bc_resp = if can_bc {
                                ui.add(bc_btn)
                            } else {
                                ui.add_enabled(false, bc_btn)
                                    .on_hover_text("B&C is not supported on RGB images")
                            };
                            if can_bc && bc_resp.clicked() {
                                if let Ok(doc) = self.session.active_doc() {
                                    self.dialogs.bc_min = doc.display_range.min;
                                    self.dialogs.bc_max = doc.display_range.max;
                                }
                                self.dialogs.show_brightness_contrast = true;
                                ui.close_menu();
                            }

                            let can_thresh = self
                                .active_image_id()
                                .and_then(|id| self.session.get_image(id))
                                .map(|d| {
                                    matches!(
                                        d.plane(),
                                        PixelPlane::Gray8(_) | PixelPlane::Gray16(_)
                                    ) && d.stack_size() == 1
                                })
                                .unwrap_or(false);

                            let thresh_btn = egui::Button::new("Threshold...");
                            let thresh_resp = if can_thresh {
                                ui.add(thresh_btn)
                            } else {
                                ui.add_enabled(false, thresh_btn).on_hover_text(
                                    "Threshold is only supported on single-slice 8-bit or 16-bit grayscale images",
                                )
                            };
                            if can_thresh && thresh_resp.clicked() {
                                self.dialogs.show_threshold = true;
                                ui.close_menu();
                            }
                        });
                    });

                    ui.menu_button("Stacks", |ui| {
                        scrollable_menu(ui, |ui| {
                            if ui
                                .add_enabled(has_active, egui::Button::new("Invert Stack"))
                                .clicked()
                            {
                                let _ = self.invert_stack_active();
                                ui.close_menu();
                            }
                        });
                    });

                    // Unsupported type converters
                    let _ = ui.add_enabled(false, egui::Button::new("Type > 8-bit (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Duplicate... (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Crop (unsupported)"));
                });
            });

            ui.menu_button("Process", |ui| {
                scrollable_menu(ui, |ui| {
                    let has_active = self.active_image_id().is_some();
                    if ui
                        .add_enabled(has_active, egui::Button::new("Smooth"))
                        .clicked()
                    {
                        let _ = self.smooth_active();
                        ui.close_menu();
                    }

                    if ui
                        .add_enabled(has_active, egui::Button::new("Sharpen"))
                        .clicked()
                    {
                        let _ = self.sharpen_active();
                        ui.close_menu();
                    }

                    if ui
                        .add_enabled(has_active, egui::Button::new("Find Edges"))
                        .clicked()
                    {
                        let _ = self.find_edges_active();
                        ui.close_menu();
                    }

                    ui.separator();

                    ui.menu_button("Filters", |ui| {
                        scrollable_menu(ui, |ui| {
                            if ui
                                .add_enabled(has_active, egui::Button::new("Gaussian Blur..."))
                                .clicked()
                            {
                                self.dialogs.show_gaussian_blur = true;
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(has_active, egui::Button::new("Median (radius 2)"))
                                .clicked()
                            {
                                let _ = self.median_active(2.0);
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(has_active, egui::Button::new("Minimum (radius 2)"))
                                .clicked()
                            {
                                let _ = self.minimum_active(2.0);
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(has_active, egui::Button::new("Maximum (radius 2)"))
                                .clicked()
                            {
                                let _ = self.maximum_active(2.0);
                                ui.close_menu();
                            }
                        });
                    });

                    ui.menu_button("Noise", |ui| {
                        scrollable_menu(ui, |ui| {
                            if ui
                                .add_enabled(has_active, egui::Button::new("Despeckle"))
                                .clicked()
                            {
                                let _ = self.despeckle_active();
                                ui.close_menu();
                            }
                        });
                    });

                    // Unsupported Process items
                    let _ = ui.add_enabled(false, egui::Button::new("Binary (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Math (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("FFT (unsupported)"));
                });
            });

            ui.menu_button("Analyze", |ui| {
                scrollable_menu(ui, |ui| {
                    let has_active = self.active_image_id().is_some();
                    if ui
                        .add_enabled(has_active, egui::Button::new("Measure\tM"))
                        .clicked()
                    {
                        let _ = self.measure_active();
                        ui.close_menu();
                    }

                    if ui
                        .add_enabled(
                            !self.session.results.rows.is_empty(),
                            egui::Button::new("Clear Results"),
                        )
                        .clicked()
                    {
                        let _ = self.session.dispatch(Command::ClearResults);
                        self.status_message = "Results cleared".to_string();
                        ui.close_menu();
                    }

                    // Unsupported Analyze items
                    let _ = ui.add_enabled(
                        false,
                        egui::Button::new("Set Measurements... (unsupported)"),
                    );
                    let _ = ui.add_enabled(false, egui::Button::new("Plot Profile (unsupported)"));
                    let _ = ui.add_enabled(false, egui::Button::new("Calibrate... (unsupported)"));
                });
            });

            ui.menu_button("Plugins", |ui| {
                scrollable_menu(ui, |ui| {
                    if ui.button("Installed Plugins...").clicked() {
                        let _ = self.open_plugins_browser();
                        ui.ctx().send_viewport_cmd_to(
                            egui::ViewportId::from_hash_of("ij_plugin_browser_window"),
                            egui::ViewportCommand::Minimized(false),
                        );
                        ui.ctx().send_viewport_cmd_to(
                            egui::ViewportId::from_hash_of("ij_plugin_browser_window"),
                            egui::ViewportCommand::Focus,
                        );
                        ui.close_menu();
                    }

                    ui.separator();

                    let has_active = self.active_image_id().is_some();
                    let is_running = self.is_plugin_filter_running();
                    let btn = egui::Button::new("Run PlugInFilter (Subset)...");
                    let resp = if has_active && !is_running {
                        ui.add(btn)
                    } else if is_running {
                        ui.add_enabled(false, btn)
                            .on_hover_text("A PlugInFilter is already executing")
                    } else {
                        ui.add_enabled(false, btn)
                            .on_hover_text("Requires an active image")
                    };
                    if has_active && !is_running && resp.clicked() {
                        self.open_plugin_filter_dialog();
                        ui.close_menu();
                    }

                    ui.separator();

                    let _ = ui.add_enabled(false, egui::Button::new("Macros > (unsupported)"));
                    let _ = ui
                        .add_enabled(false, egui::Button::new("Compile and Run... (unsupported)"));
                });
            });

            ui.menu_button("Window", |ui| {
                scrollable_menu(ui, |ui| {
                    if ui.button("Results").clicked() {
                        self.show_results_window = !self.show_results_window;
                        ui.close_menu();
                    }

                    ui.separator();

                    let ids = self.session.image_ids();
                    for id in ids {
                        let title = self
                            .session
                            .get_image(id)
                            .map(|d| d.title.clone())
                            .unwrap_or_else(|| format!("Image {}", id.0));
                        let is_active = self.active_image_id() == Some(id);
                        let prefix = if is_active { "✓ " } else { "  " };
                        if ui.button(format!("{prefix}{title}")).clicked() {
                            let _ = self.activate_image_window(id, ui.ctx());
                            ui.close_menu();
                        }
                    }
                });
            });

            ui.menu_button("Help", |ui| {
                scrollable_menu(ui, |ui| {
                    if ui.button("About ImageJ...").clicked() {
                        self.dialogs.show_about = true;
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn render_toolbar_tools(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
            for &tool in Tool::ALL {
                let is_selected = self.active_tool == tool;
                let is_supp = tool.is_supported();

                let text = format!(" {} ", tool.symbol());
                let btn = egui::Button::new(text).min_size(Vec2::new(28.0, 24.0));

                let resp = if is_supp {
                    if is_selected {
                        ui.add(btn.fill(Color32::from_rgb(180, 205, 240)))
                    } else {
                        ui.add(btn)
                    }
                } else {
                    ui.add_enabled(false, btn)
                };

                if is_supp && resp.clicked() {
                    self.active_tool = tool;
                    self.status_message = format!("Selected tool: {}", tool.name());
                }

                if !is_supp {
                    resp.on_hover_text(format!("{} (not yet implemented)", tool.name()));
                } else {
                    resp.on_hover_text(tool.name());
                }
            }
        });
    }

    fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.status_message);
            if let Some(prog) = self.progress {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::ProgressBar::new(prog)
                            .desired_width(120.0)
                            .show_percentage(),
                    );
                });
            }
        });
    }

    pub fn render_dialogs(&mut self, ctx: &egui::Context) {
        if self.dialogs.show_about {
            let mut is_open = true;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_about_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("About ImageJ-rs")
                    .with_inner_size([380.0, 180.0])
                    .with_resizable(false),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        is_open = false;
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.heading("ImageJ 1.54u");
                        ui.label("Rust reproduction of Wayne Rasband's ImageJ.");
                        ui.label("NIH Image & ImageJ: Wayne Rasband (1949-2021)");
                        ui.separator();
                        ui.label("ImageJ-rs Desktop Shell");
                        ui.label("Pixel-exact & measurement-exact verification against ImageJ 1.54u oracle.");
                        ui.separator();
                        if ui.button("OK").clicked() {
                            is_open = false;
                        }
                    });
                },
            );
            if !is_open {
                self.dialogs.show_about = false;
            }
        }

        if self.dialogs.show_gaussian_blur {
            let mut close_dialog = false;
            let mut apply_sigma = None;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_gaussian_blur_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("Gaussian Blur...")
                    .with_inner_size([280.0, 130.0])
                    .with_resizable(false),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        close_dialog = true;
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Sigma (Radius):");
                            ui.add(
                                egui::DragValue::new(&mut self.dialogs.gaussian_blur_sigma)
                                    .speed(0.1)
                                    .range(0.1..=100.0),
                            );
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                apply_sigma = Some(self.dialogs.gaussian_blur_sigma);
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
                },
            );
            if let Some(sigma) = apply_sigma {
                let _ = self.gaussian_blur_active(sigma);
            }
            if close_dialog {
                self.dialogs.show_gaussian_blur = false;
            }
        }

        if self.dialogs.show_brightness_contrast {
            let mut close_dialog = false;
            let mut apply_range = None;
            let can_bc = self
                .active_image_id()
                .and_then(|id| self.session.get_image(id))
                .map(|d| !matches!(d.plane(), PixelPlane::ColorRgb(_)))
                .unwrap_or(false);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_bc_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("B&C")
                    .with_inner_size([300.0, 170.0])
                    .with_resizable(false),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        close_dialog = true;
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if !can_bc {
                            ui.colored_label(
                                Color32::from_rgb(220, 80, 80),
                                "B&C is not supported on RGB images.",
                            );
                            ui.separator();
                        }
                        ui.horizontal(|ui| {
                            ui.label("Minimum:");
                            ui.add(egui::DragValue::new(&mut self.dialogs.bc_min).speed(1.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Maximum:");
                            ui.add(egui::DragValue::new(&mut self.dialogs.bc_max).speed(1.0));
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.add_enabled(can_bc, egui::Button::new("Apply")).clicked() {
                                apply_range = Some((self.dialogs.bc_min, self.dialogs.bc_max));
                            }
                            if ui.add_enabled(can_bc, egui::Button::new("Reset")).clicked() {
                                self.dialogs.bc_min = 0.0;
                                self.dialogs.bc_max = 255.0;
                                apply_range = Some((0.0, 255.0));
                            }
                            if ui.button("Close").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
                },
            );
            if let Some((min, max)) = apply_range {
                let _ = self.set_display_range_active(min, max);
            }
            if close_dialog {
                self.dialogs.show_brightness_contrast = false;
            }
        }

        if self.dialogs.show_threshold {
            let mut close_dialog = false;
            let mut apply_thresh = None;
            let mut reset_thresh = false;
            let threshold_target = self
                .active_image_id()
                .and_then(|id| self.session.get_image(id))
                .map(|d| {
                    (
                        matches!(d.plane(), PixelPlane::Gray8(_) | PixelPlane::Gray16(_))
                            && d.stack_size() == 1,
                        matches!(d.plane(), PixelPlane::Gray16(_)),
                    )
                })
                .unwrap_or((false, false));
            let can_thresh = threshold_target.0;
            let is_16bit = threshold_target.1;
            if !is_16bit {
                // ImageJ only shows the "16-bit histogram" checkbox for
                // 16-bit images (ThresholdAdjuster.java lines 502-503).
                self.dialogs.threshold_16bit = false;
            }

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_threshold_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("Threshold")
                    .with_inner_size([320.0, 210.0])
                    .with_resizable(false),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        close_dialog = true;
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if !can_thresh {
                            ui.colored_label(
                                Color32::from_rgb(220, 80, 80),
                                "Requires single-slice 8-bit or 16-bit grayscale image.",
                            );
                            ui.separator();
                        }
                        ui.horizontal(|ui| {
                            ui.label("Method:");
                            egui::ComboBox::from_id_salt("thresh_method")
                                .selected_text(&self.dialogs.threshold_method)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.dialogs.threshold_method,
                                        "IsoData".into(),
                                        "IsoData",
                                    );
                                    ui.selectable_value(
                                        &mut self.dialogs.threshold_method,
                                        "Otsu".into(),
                                        "Otsu",
                                    );
                                });
                        });
                        ui.checkbox(&mut self.dialogs.threshold_dark_bg, "Dark background");
                        ui.add_enabled(
                            is_16bit,
                            egui::Checkbox::new(
                                &mut self.dialogs.threshold_16bit,
                                "16-bit histogram",
                            ),
                        )
                        .on_disabled_hover_text(
                            "The 16-bit histogram option requires a 16-bit image",
                        );
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(can_thresh, egui::Button::new("Apply"))
                                .clicked()
                            {
                                apply_thresh = Some((
                                    self.dialogs.threshold_method.clone(),
                                    self.dialogs.threshold_dark_bg,
                                    self.dialogs.threshold_16bit && is_16bit,
                                ));
                            }
                            if ui
                                .add_enabled(can_thresh, egui::Button::new("Reset"))
                                .clicked()
                            {
                                reset_thresh = true;
                            }
                            if ui.button("Close").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
                },
            );
            if let Some((method, dark, histogram_16)) = apply_thresh {
                let _ = self.auto_threshold_active_with_options(&method, dark, histogram_16);
            }
            if reset_thresh {
                let _ = self.reset_threshold_active();
            }
            if close_dialog {
                self.dialogs.show_threshold = false;
            }
        }

        // "Process Stack?" decision, matching IJ.setupDialog. Yes is offered
        // only where a verified whole-stack implementation exists; otherwise
        // the dialog says so and requires an explicit Current Slice or Cancel.
        if let Some(pending) = self.pending_stack_process.clone() {
            let stack_size = pending.stack_size;
            let supports_stack = pending.operation.supports_whole_stack();
            let limitation = pending
                .operation
                .whole_stack_limitation()
                .unwrap_or_default();
            let current_slice = self
                .active_image_id()
                .and_then(|id| self.session.get_image(id))
                .map(|doc| doc.current_slice)
                .unwrap_or(1);
            let mut choice = None;

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_process_stack_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("Process Stack?")
                    .with_inner_size([380.0, 170.0])
                    .with_resizable(false),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        choice = Some(StackProcessChoice::Cancel);
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if supports_stack {
                            ui.label(format!("Process all {stack_size} images?"));
                            ui.label("There is no Undo if you select \"Yes\".");
                        } else {
                            ui.colored_label(Color32::from_rgb(220, 80, 80), &limitation);
                            ui.label(format!("Current slice: {current_slice} of {stack_size}."));
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            if supports_stack {
                                if ui.button("Yes").clicked() {
                                    choice = Some(StackProcessChoice::All);
                                }
                                if ui.button("No").clicked() {
                                    choice = Some(StackProcessChoice::Current);
                                }
                            } else if ui.button("Current Slice").clicked() {
                                choice = Some(StackProcessChoice::Current);
                            }
                            if ui.button("Cancel").clicked() {
                                choice = Some(StackProcessChoice::Cancel);
                            }
                        });
                    });
                },
            );

            if let Some(choice) = choice {
                let _ = self.resolve_stack_process(choice);
            }
        }

        // Modal "Save changes?" guard dialog for pending close routes
        if let Some(pending) = &self.pending_close {
            if let Some(id) = pending.current_prompt {
                let title = self
                    .session
                    .get_image(id)
                    .map(|d| d.title.clone())
                    .unwrap_or_else(|| format!("Image {}", id.0));
                let msg = if title.len() > 22 {
                    format!("Save changes to\n\"{title}\"?")
                } else {
                    format!("Save changes to \"{title}\"?")
                };

                let mut action = None;
                ctx.show_viewport_immediate(
                    egui::ViewportId::from_hash_of("ij_confirm_close_dialog"),
                    egui::ViewportBuilder::default()
                        .with_title("ImageJ")
                        .with_inner_size([360.0, 140.0])
                        .with_resizable(false),
                    |ctx, class| {
                        let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                        if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                            || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            action = Some(CloseAction::Cancel);
                        }
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.add(egui::Label::new(&msg).wrap());
                            ui.separator();
                            ui.horizontal(|ui| {
                                if ui.button("  Save  ").clicked() {
                                    action = Some(CloseAction::Save);
                                }
                                if ui.button("Don't Save").clicked() {
                                    action = Some(CloseAction::Discard);
                                }
                                if ui.button("  Cancel  ").clicked() {
                                    action = Some(CloseAction::Cancel);
                                }
                            });
                        });
                    },
                );

                match action {
                    Some(CloseAction::Cancel) => {
                        self.confirm_pending_close_cancel();
                    }
                    Some(CloseAction::Discard) => {
                        let _ = self.confirm_pending_close_discard();
                    }
                    Some(CloseAction::Save) => {
                        // An image opened from or previously saved to a TIFF is
                        // re-saved in place, with no Save dialog (FileSaver.save).
                        if let Some(result) = self.save_pending_close_to_remembered_path() {
                            let _ = result;
                        } else {
                            let default_name = proposed_tiff_file_name(&title);
                            let save_path = rfd::FileDialog::new()
                                .add_filter("TIFF Image", &["tif", "tiff"])
                                .set_file_name(&default_name)
                                .save_file();
                            let _ = self.confirm_pending_close_save(save_path);
                        }
                    }
                    None => {}
                }
            }
        }

        // Error alert dialog
        if let Some(err_msg) = &self.dialogs.last_error.clone() {
            let mut dismiss = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_error_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("ImageJ Error")
                    .with_inner_size([460.0, 180.0])
                    .with_min_inner_size([320.0, 140.0])
                    .with_resizable(true),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                        || ctx.input(|i| {
                            i.key_pressed(egui::Key::Escape)
                                || i.key_pressed(egui::Key::Enter)
                                || i.key_pressed(egui::Key::Space)
                        })
                    {
                        dismiss = true;
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(100.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.add(egui::Label::new(err_msg).wrap());
                            });
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("  OK  ").clicked() {
                                dismiss = true;
                            }
                        });
                    });
                },
            );
            if dismiss {
                self.dialogs.last_error = None;
            }
        }

        // Java PlugInFilter configuration dialog
        if self.dialogs.show_plugin_filter {
            let mut close_dialog = false;
            let mut trigger_run = false;
            let mut trigger_detect_bridge = false;
            // A pinned target must never outlive its document. Clearing (rather
            // than silently retargeting) keeps the displayed target honest.
            if self
                .dialogs
                .plugin_filter_target_id
                .is_some_and(|id| self.session.get_image(id).is_none())
            {
                self.dialogs.plugin_filter_target_id = None;
            }
            let target_id = self.dialogs.plugin_filter_target_id;
            let doc_info = target_id.and_then(|id| {
                self.session
                    .get_image(id)
                    .map(|d| (d.title.clone(), d.width, d.height, d.stack_size()))
            });
            // One bridge-owned validator for both the UI and the session.
            let preflight_error = target_id
                .and_then(|id| self.session.plugin_filter_preflight(id).err())
                .map(|err| match err {
                    SessionError::Plugin(PluginBridgeError::UnsupportedFilterInput(error)) => {
                        error.to_string()
                    }
                    other => other.to_string(),
                });

            let has_target = doc_info.is_some();
            let is_compatible = has_target && preflight_error.is_none();
            let bridge_available = self.session.plugin_bridge().is_some();
            let class_valid = !self.dialogs.plugin_filter_class.trim().is_empty();
            let can_run = is_compatible
                && bridge_available
                && class_valid
                && !self.is_plugin_filter_running();

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_plugin_filter_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("Run Java PlugInFilter (Subset)")
                    .with_inner_size([500.0, 520.0])
                    .with_resizable(true),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        close_dialog = true;
                    }

                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.group(|ui| {
                                ui.label(
                                    egui::RichText::new("Java PlugInFilter Narrow Compatibility")
                                        .strong(),
                                );
                                ui.label(
                                    "Runs an explicit Java PlugInFilter class through an isolated JVM process.\n\
                                    • Supported: Single-slice 8-bit grayscale images with optional in-bounds rectangular ROI.\n\
                                    • Unsupported: Stacks/multislice, RGB/16-bit, non-rectangular selections, active thresholds, custom LUTs/calibration, interactive dialogs, or macro replay.",
                                );
                            });

                            ui.add_space(6.0);

                            ui.group(|ui| {
                                ui.label(egui::RichText::new("Target Document").strong());
                                if let Some((title, width, height, stack_size)) = &doc_info {
                                    let id_str = target_id
                                        .map(|i| format!("ID {}", i.0))
                                        .unwrap_or_default();
                                    ui.label(format!(
                                        "Image: \"{title}\" ({id_str}, {width}x{height})"
                                    ));
                                    if *stack_size != 1 {
                                        ui.label(format!("Stack: {stack_size} slices"));
                                    }
                                    if let Some(error) = &preflight_error {
                                        ui.colored_label(
                                            Color32::from_rgb(220, 80, 80),
                                            format!("❌ {error}"),
                                        );
                                    }
                                    if is_compatible {
                                        ui.colored_label(
                                            Color32::from_rgb(60, 180, 60),
                                            "✔ Image is compatible with the narrow PlugInFilter subset.",
                                        );
                                    }
                                } else {
                                    ui.colored_label(
                                        Color32::from_rgb(220, 80, 80),
                                        "❌ No target image is selected. Reopen the dialog from an image window.",
                                    );
                                }
                            });

                            ui.add_space(6.0);

                            ui.label(egui::RichText::new("Class Name:").strong());
                            ui.add(
                                egui::TextEdit::singleline(&mut self.dialogs.plugin_filter_class)
                                    .hint_text("e.g. fixtures.RectInvertFilter"),
                            );

                            ui.add_space(6.0);

                            let mut remove_idx = None;
                            egui::CollapsingHeader::new(format!(
                                "Classpath / JAR Files ({} entries)",
                                self.dialogs.plugin_filter_classpath.len()
                            ))
                            .id_salt("ij_plugin_filter_classpath")
                            .show(ui, |ui| {
                                for (idx, path) in
                                    self.dialogs.plugin_filter_classpath.iter().enumerate()
                                {
                                    ui.horizontal(|ui| {
                                        ui.monospace(path.display().to_string());
                                        if ui.button("Remove").clicked() {
                                            remove_idx = Some(idx);
                                        }
                                    });
                                }
                            });
                            if let Some(idx) = remove_idx {
                                self.dialogs.plugin_filter_classpath.remove(idx);
                            }

                            ui.horizontal(|ui| {
                                if ui.button("Browse JAR...").clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("JAR File", &["jar"])
                                        .pick_file()
                                    {
                                        self.dialogs.plugin_filter_classpath.push(path);
                                    }
                                }
                                if ui.button("Browse Folder...").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                        self.dialogs.plugin_filter_classpath.push(path);
                                    }
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(
                                        &mut self.dialogs.plugin_filter_classpath_input,
                                    )
                                    .hint_text("Or enter classpath entry"),
                                );
                                if ui.button("Add Path").clicked() {
                                    let trimmed =
                                        self.dialogs.plugin_filter_classpath_input.trim();
                                    if !trimmed.is_empty() {
                                        self.dialogs
                                            .plugin_filter_classpath
                                            .push(PathBuf::from(trimmed));
                                        self.dialogs.plugin_filter_classpath_input.clear();
                                    }
                                }
                            });

                            ui.add_space(6.0);

                            ui.label(
                                egui::RichText::new("Argument (passed to setup / run):").strong(),
                            );
                            ui.text_edit_singleline(&mut self.dialogs.plugin_filter_arg);

                            ui.add_space(6.0);

                            ui.horizontal(|ui| {
                                if bridge_available {
                                    ui.colored_label(
                                        Color32::from_rgb(60, 180, 60),
                                        "Bridge: Ready",
                                    );
                                } else {
                                    ui.colored_label(
                                        Color32::from_rgb(220, 80, 80),
                                        "Bridge: Not configured",
                                    );
                                    if ui.button("Auto-detect Bridge").clicked() {
                                        trigger_detect_bridge = true;
                                    }
                                }
                            });

                            ui.separator();

                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(can_run, egui::Button::new("Run Filter"))
                                    .clicked()
                                {
                                    trigger_run = true;
                                    close_dialog = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    close_dialog = true;
                                }
                            });
                        });
                    });
                },
            );

            if trigger_detect_bridge {
                let _ = self.try_detect_plugin_bridge();
            }
            // Start while the pinned target is still available, then dismiss:
            // closing/cancelling must not leave a target for a later reopen.
            if trigger_run {
                let _ = self.start_plugin_filter_from_dialog();
            }
            if close_dialog || trigger_run {
                self.dismiss_plugin_filter_dialog();
            }
        }

        // Modal execution progress dialog while JVM is running
        if let Some(pending) = &self.pending_plugin_filter {
            let class_name = pending.class_name.clone();
            let target_id = pending.target_image_id;
            let cancelling = pending.phase == PluginFilterPhase::Cancelling;
            let mut request_cancel = false;

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_plugin_executing_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("Executing PlugInFilter")
                    .with_inner_size([380.0, 150.0])
                    .with_resizable(false),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        request_cancel = true;
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if cancelling {
                            ui.heading(format!("Cancelling {class_name}"));
                        } else {
                            ui.heading(format!("Running {class_name}"));
                        }
                        ui.label(format!("Target image: ID {}", target_id.0));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            if cancelling {
                                ui.label("Waiting for the child JVM to exit...");
                            } else {
                                ui.label("Executing in child JVM... The UI remains interactive.");
                            }
                        });
                        ui.separator();
                        if ui
                            .add_enabled(!cancelling, egui::Button::new("Cancel Execution"))
                            .clicked()
                        {
                            request_cancel = true;
                        }
                    });
                },
            );

            if request_cancel {
                self.cancel_plugin_filter();
            }
        }

        self.render_plugin_browser_window(ctx);
    }

    /// Independent, scrollable Installed Plugins window.
    ///
    /// The window only performs read-only discovery and explicit inspection.
    /// The panel states plainly that loading/inspecting does not sandbox code
    /// and that running a verified filter still uses the external JVM.
    pub fn render_plugin_browser_window(&mut self, ctx: &egui::Context) {
        if !self.plugins_browser.show {
            return;
        }

        let mut close_window = false;
        let mut trigger_browse = false;
        let mut trigger_scan = false;
        let mut trigger_inspect = false;
        let mut trigger_prefill = false;
        let mut trigger_cancel = false;

        let busy = self.is_plugin_catalog_busy();
        let is_ready = self.plugins_browser.phase == PluginCatalogPhase::Ready;
        let close_guard = self.pending_close.is_some() || self.quit_app;
        let target_id = self.active_image_id();
        let target_info = target_id.and_then(|id| {
            self.session
                .get_image(id)
                .map(|doc| (doc.title.clone(), doc.width, doc.height, doc.stack_size()))
        });
        let target_preflight = target_id
            .and_then(|id| self.session.plugin_filter_preflight(id).err())
            .map(|error| error.to_string());
        let label_counts = self
            .plugins_browser
            .catalog
            .as_ref()
            .map(plugin_label_counts)
            .unwrap_or_default();

        let can_scan =
            !busy && !close_guard && !self.plugins_browser.directory_input.trim().is_empty();

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_plugin_browser_window"),
            egui::ViewportBuilder::default()
                .with_title("Installed Plugins")
                .with_inner_size([780.0, 600.0])
                .with_min_inner_size([520.0, 360.0])
                .with_resizable(true),
            |ctx, class| {
                let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                        && !ctx.wants_keyboard_input()
                {
                    close_window = true;
                }

                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("ij_plugin_browser_content")
                        .show(ui, |ui| {
                    ui.heading("Installed Plugins");
                    ui.label(
                        "Read-only discovery of classic ImageJ plug-ins. Scanning reads directory \
                         names and archive config files only; inspection loads a class with \
                         Class.forName(name, false, loader) and never runs static initializers, \
                         setup() or run().",
                    );
                    ui.colored_label(
                        Color32::from_rgb(190, 120, 20),
                        "No sandbox: inspected or executed Java code runs in a child JVM with your \
                         user privileges. Only scan and run trusted plug-ins.",
                    );
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label("Plugins directory:");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut self.plugins_browser.directory_input,
                            )
                            .hint_text("/path/to/ImageJ/plugins")
                            .desired_width(ui.available_width()),
                        );
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!busy && !close_guard, egui::Button::new("Browse..."))
                            .clicked()
                        {
                            trigger_browse = true;
                        }
                        if ui
                            .add_enabled(can_scan, egui::Button::new("Scan"))
                            .on_disabled_hover_text(if busy {
                                "A scan or inspection is already running"
                            } else if self.plugins_browser.directory_input.trim().is_empty() {
                                "Enter a plugins directory path first"
                            } else {
                                "Resolve close/quit confirmation first"
                            })
                            .clicked()
                        {
                            trigger_scan = true;
                        }
                        if ui
                            .add_enabled(busy, egui::Button::new("Cancel"))
                            .on_disabled_hover_text("Nothing is running")
                            .clicked()
                        {
                            trigger_cancel = true;
                        }
                    });

                    ui.add_space(4.0);
                    if let Some(status) = &self.plugins_browser.status {
                        ui.label(status);
                    }
                    if let Some(error) = &self.plugins_browser.scan_error {
                        ui.colored_label(
                            Color32::from_rgb(220, 80, 80),
                            format!("Scan error: {error}"),
                        );
                    }

                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Narrow PlugInFilter target").strong());
                        if let Some((title, width, height, stack_size)) = &target_info {
                            ui.label(format!(
                                "Active image: \"{title}\" (ID {}, {width}x{height})",
                                target_id.map(|id| id.0).unwrap_or_default()
                            ));
                            if *stack_size != 1 {
                                ui.label(format!("Stack: {stack_size} slices"));
                            }
                            if let Some(error) = &target_preflight {
                                ui.colored_label(
                                    Color32::from_rgb(220, 80, 80),
                                    format!("Preflight: {error}"),
                                );
                            } else {
                                ui.colored_label(
                                    Color32::from_rgb(60, 180, 60),
                                    "Preflight: compatible with the narrow 8-bit single-slice subset.",
                                );
                            }
                        } else {
                            ui.colored_label(
                                Color32::from_rgb(220, 80, 80),
                                "No active image. Open/activate an image before prefilling a filter.",
                            );
                        }
                        ui.label(
                            "Prefill pins the image that is active when you click, so a later \
                             window switch cannot retarget the run.",
                        );
                        if self.session.plugin_bridge().is_none() {
                            ui.label(
                                "Java bridge is not configured; Inspect Selected will try to find it.",
                            );
                        }
                    });

                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Catalog entries").strong());
                    let mut catalog_info = None;
                    if let Some(catalog) = &self.plugins_browser.catalog {
                        catalog_info = Some((
                            catalog.plugins.len(),
                            catalog.archives.len(),
                            catalog.warnings.len(),
                            catalog.warnings_truncated,
                            catalog.plugins_dir.display().to_string(),
                            catalog.classpath.len(),
                        ));
                    }
                    if let Some((entries, archives, warnings, truncated, dir, classpath)) =
                        catalog_info
                    {
                        ui.label(format!(
                            "{entries} entries, {archives} archives, {warnings} warnings{} · {dir} · {classpath} classpath entries",
                            if truncated { " (warning limit reached)" } else { "" }
                        ));
                    } else if busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Scanning or inspecting in the background; the UI stays responsive.");
                        });
                    } else {
                        ui.label("No catalog yet. Choose a plugins directory and press Scan.");
                    }

                    let mut new_selection = None;
                    let selected = self.plugins_browser.selected;
                    if let Some(catalog) = &self.plugins_browser.catalog {
                        egui::ScrollArea::vertical()
                            .id_salt("ij_plugin_catalog_entries")
                            .max_height(260.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for (index, plugin) in catalog.plugins.iter().enumerate() {
                                    let conflicting = label_counts
                                        .get(&plugin.label)
                                        .is_some_and(|count| *count > 1);
                                    let is_selected = selected == Some(index);
                                    let row = plugin_row_text(plugin, conflicting, is_selected);
                                    let response = ui
                                        .selectable_label(is_selected, row)
                                        .on_hover_text(format!(
                                            "{}\nSource: {}\nClass spec: {}",
                                            plugin_kind_support_note(plugin.kind),
                                            plugin.source,
                                            plugin.class_spec
                                        ));
                                    if response.clicked() {
                                        new_selection = Some(index);
                                    }
                                }
                            });
                    }

                    if let Some(index) = new_selection {
                        self.plugins_browser.selected = Some(index);
                    }
                    let selected = self.plugins_browser.selected;
                    let prefill_reason =
                        selected.and_then(|index| self.plugin_prefill_block_reason(index));
                    let can_inspect = !busy && is_ready && selected.is_some() && !close_guard;
                    let can_prefill =
                        selected.is_some() && prefill_reason.is_none() && !busy && !close_guard;

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_inspect, egui::Button::new("Inspect Selected"))
                            .on_disabled_hover_text(if busy {
                                "Wait for the current scan/inspection"
                            } else if !is_ready {
                                "Scan a plugins directory first"
                            } else {
                                "Select an entry first"
                            })
                            .clicked()
                        {
                            trigger_inspect = true;
                        }
                        if ui
                            .add_enabled(can_prefill, egui::Button::new("Open in PlugInFilter..."))
                            .on_disabled_hover_text(
                                prefill_reason
                                    .as_deref()
                                    .unwrap_or("Requires a verified PlugInFilter and a compatible active image"),
                            )
                            .on_hover_text(
                                "Fills the existing narrow dialog and pins the active image; it does not run the filter.",
                            )
                            .clicked()
                        {
                            trigger_prefill = true;
                        }
                        if ui.button("Close").clicked() {
                            close_window = true;
                        }
                    });

                    ui.separator();
                    ui.label(egui::RichText::new("Selected entry").strong());
                    if let Some(index) = selected {
                        if let Some(plugin) = self
                            .plugins_browser
                            .catalog
                            .as_ref()
                            .and_then(|catalog| catalog.plugins.get(index))
                        {
                            ui.label(format!(
                                "{}  [{}]",
                                plugin.label,
                                plugin_kind_badge(plugin.kind)
                            ));
                            ui.label(plugin_kind_support_note(plugin.kind));
                            ui.label(format!(
                                "Menu: {} · Class spec: {} · Source: {}",
                                crate::dialogs::plugin_menu_display(plugin),
                                plugin.class_spec,
                                plugin.source
                            ));
                            if plugin.duplicate {
                                ui.label(
                                    "ImageJ flags this label as a duplicate of an earlier entry.",
                                );
                            }
                            if let Some(inspection) = &plugin.inspection {
                                ui.monospace(format!(
                                    "Inspected class: {}",
                                    inspection.class_name
                                ));
                                if let Some(message) = &inspection.message {
                                    ui.label(format!("Host message: {message}"));
                                }
                                if let Some(failure) = &inspection.failure {
                                    ui.colored_label(
                                        Color32::from_rgb(220, 80, 80),
                                        format!("Failure: {failure:?}"),
                                    );
                                }
                            } else {
                                ui.label("Inspection detail: none (Uninspected).");
                            }
                            if let Some(reason) = &prefill_reason {
                                ui.colored_label(
                                    Color32::from_rgb(190, 120, 20),
                                    format!("Prefill disabled: {reason}"),
                                );
                            }
                            if let PluginInspectPhase::Failed { index: failed_index, message } =
                                &self.plugins_browser.inspect_phase
                            {
                                if *failed_index == index {
                                    ui.colored_label(
                                        Color32::from_rgb(220, 80, 80),
                                        format!("Inspection worker: {message}"),
                                    );
                                }
                            }
                        }
                    } else {
                        ui.label("Nothing selected.");
                    }

                    ui.separator();
                    ui.label(egui::RichText::new("Warnings").strong());
                    if let Some(catalog) = &self.plugins_browser.catalog {
                        if catalog.warnings.is_empty() {
                            ui.label("No warnings.");
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("ij_plugin_catalog_warnings")
                                .max_height(110.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    for warning in &catalog.warnings {
                                        ui.label(format!("• {warning}"));
                                    }
                                });
                        }
                    } else {
                        ui.label("—");
                    }

                    if busy {
                        ui.ctx()
                            .request_repaint_after(Duration::from_millis(50));
                    }
                        });
                });
            },
        );

        if trigger_browse {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                self.plugins_browser.directory_input = path.display().to_string();
            }
        }
        if trigger_scan {
            if let Err(error) = self.start_plugin_directory_scan_from_input() {
                self.status_message = error.clone();
                self.plugins_browser.status = Some(error);
            }
        }
        if trigger_cancel {
            self.cancel_plugin_catalog_job();
        }
        if trigger_inspect {
            if let Err(error) = self.inspect_selected_plugin_entry() {
                self.status_message = error.clone();
                self.plugins_browser.status = Some(error);
            }
        }
        if trigger_prefill {
            if let Some(index) = self.plugins_browser.selected {
                if let Err(error) = self.prefill_plugin_filter_from_catalog(index) {
                    self.status_message = error.clone();
                    self.plugins_browser.status = Some(error);
                }
            }
        }
        if close_window {
            self.close_plugins_browser();
        }
    }

    pub fn results_grid_cells(&self) -> Vec<Vec<String>> {
        self.session
            .results
            .to_tsv_basic()
            .lines()
            .map(|line| line.split('\t').map(str::to_string).collect())
            .collect()
    }

    pub fn render_results_window(&mut self, ctx: &egui::Context) {
        if !self.show_results_window {
            return;
        }

        let mut is_open = true;
        let mut save_results_path = None;
        let mut clear_results = false;
        let grid_cells = self.results_grid_cells();

        let viewport_id = egui::ViewportId::from_hash_of("ij_results_window");
        ctx.show_viewport_immediate(
            viewport_id,
            egui::ViewportBuilder::default()
                .with_title("Results")
                .with_inner_size([450.0, 250.0])
                .with_min_inner_size([250.0, 150.0])
                .with_resizable(true),
            |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    is_open = false;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Clear").clicked() {
                            clear_results = true;
                        }
                        if ui.button("Save As...").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("TSV Table", &["tsv", "txt"])
                                .save_file()
                            {
                                save_results_path = Some(path);
                            }
                        }
                    });
                    ui.separator();

                    egui::ScrollArea::both().show(ui, |ui| {
                        egui::Grid::new("results_grid")
                            .striped(true)
                            .min_col_width(60.0)
                            .show(ui, |ui| {
                                for row in &grid_cells {
                                    for cell in row {
                                        ui.label(cell);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                });
            },
        );

        if clear_results {
            let _ = self.session.dispatch(Command::ClearResults);
        }
        if let Some(path) = save_results_path {
            let _ = self.save_results_tsv(path);
        }
        if !is_open {
            self.show_results_window = false;
        }
    }

    /// Renders an image window with canvas, pixel display, zoom, pan, ROI tools, and slice slider.
    pub fn render_image_canvas_content(&mut self, ui: &mut egui::Ui, image_id: ImageId) {
        let (doc_w, doc_h, subtitle, stack_size, current_slice) = {
            let Some(doc) = self.session.get_image(image_id) else {
                ui.label("Image not found");
                return;
            };
            (
                doc.width,
                doc.height,
                ImageView::create_subtitle(doc),
                doc.stack_size(),
                doc.current_slice,
            )
        };

        let mut view = self
            .image_views
            .remove(&image_id)
            .unwrap_or_else(|| ImageView::new(doc_w, doc_h));

        // Subtitle bar
        ui.horizontal(|ui| {
            ui.label(&subtitle);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+").clicked() {
                    view.zoom_in();
                }
                if ui.button("-").clicked() {
                    view.zoom_out();
                }
                if ui.button("100%").clicked() {
                    view.set_zoom(1.0);
                    view.pan = (0.0, 0.0);
                }
                ui.label(format!("{}%", (view.zoom * 100.0).round() as i32));
            });
        });

        ui.separator();

        // Stack slice slider if stack has more than 1 slice
        let mut new_slice = None;
        if stack_size > 1 {
            let mut slice_idx = current_slice as i32;
            ui.horizontal(|ui| {
                ui.label(format!("Slice {}/{}", current_slice, stack_size));
                if ui
                    .add(egui::Slider::new(&mut slice_idx, 1..=stack_size as i32))
                    .changed()
                {
                    new_slice = Some(slice_idx);
                }
            });
        }

        // Per-image texture caching keyed by reliable Session revision
        let current_revision = self.session.image_revision(image_id);
        let needs_upload = match self.texture_cache.get(&image_id) {
            Some((cached_rev, _)) => *cached_rev != current_revision,
            None => true,
        };

        if needs_upload {
            let rgba = {
                let doc = self.session.get_image(image_id).unwrap();
                plane_to_rgba(
                    doc.plane(),
                    doc.width,
                    doc.height,
                    doc.display_range,
                    doc.threshold,
                )
            };

            let color_image =
                ColorImage::from_rgba_unmultiplied([doc_w as usize, doc_h as usize], &rgba);

            // Nearest-neighbor texture so pixels remain crisp squares when zoomed
            let texture = ui.ctx().load_texture(
                format!("doc_tex_{}", image_id.0),
                color_image,
                TextureOptions::NEAREST,
            );
            self.texture_cache
                .insert(image_id, (current_revision, texture));
            self.texture_upload_count += 1;
        } else {
            self.texture_cache_hit_count += 1;
        }

        let texture_id = self.texture_cache.get(&image_id).unwrap().1.id();

        // The interaction area is the visible viewport. Panning can move the
        // image beyond its original bounds while keeping visible pixels usable.
        let canvas_size = Vec2::new(
            doc_w as f32 * view.zoom as f32,
            doc_h as f32 * view.zoom as f32,
        );

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        let tool = self.active_tool;
        if tool == Tool::Hand && response.dragged() {
            let delta = ui.input(|i| i.pointer.delta());
            view.pan.0 += delta.x;
            view.pan.1 += delta.y;
        }

        // Draw image texture with pan
        let img_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + view.pan.0, rect.min.y + view.pan.1),
            canvas_size,
        );
        painter.image(
            texture_id,
            img_rect,
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        if response.clicked() || response.is_pointer_button_down_on() {
            let _ = self.set_active_window(image_id);
        }

        // Mouse hover: update status bar with pixel coordinate and value
        let mut hover_pixel = None;
        if let Some(hover_pos) = response.hover_pos().filter(|pos| img_rect.contains(*pos)) {
            let (ix, iy) = view.screen_to_pixel(hover_pos.x - rect.min.x, hover_pos.y - rect.min.y);
            hover_pixel = Some((ix, iy));
        }

        // Active Tool Interactions
        if response.drag_started() {
            let _ = self.set_active_window(image_id);
            let start_pos = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos());
            if let Some(pos) = start_pos.filter(|pos| tool == Tool::Hand || img_rect.contains(*pos))
            {
                let (px, py) = view.screen_to_pixel(pos.x - rect.min.x, pos.y - rect.min.y);
                view.is_dragging = true;
                view.drag_start_pixel = Some((px, py));
                view.drag_current_pixel = Some((px, py));
            }
        }

        if view.is_dragging {
            let _ = self.set_active_window(image_id);
            if let Some(pos) = response.interact_pointer_pos() {
                if tool != Tool::Hand {
                    let (px, py) = view.screen_to_pixel(pos.x - rect.min.x, pos.y - rect.min.y);
                    view.drag_current_pixel = Some((px, py));
                }
            }
        }

        enum DragAction {
            Rect(i32, i32, i32, i32),
            Oval(i32, i32, i32, i32),
            Line(i32, i32, i32, i32),
        }

        let mut drag_action = None;
        if response.drag_stopped() {
            view.is_dragging = false;
            if let (Some(start), Some(curr)) = (view.drag_start_pixel, view.drag_current_pixel) {
                let x = start.0.min(curr.0);
                let y = start.1.min(curr.1);
                let w = (start.0 - curr.0).abs();
                let h = (start.1 - curr.1).abs();

                match tool {
                    Tool::Rectangle => {
                        if w > 0 && h > 0 {
                            drag_action = Some(DragAction::Rect(x, y, w, h));
                        }
                    }
                    Tool::Oval => {
                        if w > 0 && h > 0 {
                            drag_action = Some(DragAction::Oval(x, y, w, h));
                        }
                    }
                    Tool::Line => {
                        drag_action = Some(DragAction::Line(start.0, start.1, curr.0, curr.1));
                    }
                    _ => {}
                }
            }
            view.drag_start_pixel = None;
            view.drag_current_pixel = None;
        }

        let mut was_clicked = false;
        if response.clicked() {
            was_clicked = true;
            if tool == Tool::Magnifier {
                let is_zoom_out = ui.input(|i| i.modifiers.alt);
                if is_zoom_out {
                    view.zoom_out();
                } else {
                    view.zoom_in();
                }
            }
        }

        // Draw active ROI overlay (classic ImageJ yellow outline)
        let stroke = Stroke::new(1.0_f32, Color32::from_rgb(255, 255, 0));
        let z = view.zoom as f32;

        // Draw in-progress dragging preview
        if view.is_dragging {
            if let (Some(start), Some(curr)) = (view.drag_start_pixel, view.drag_current_pixel) {
                match tool {
                    Tool::Rectangle => {
                        let x = start.0.min(curr.0) as f32 * z + img_rect.min.x;
                        let y = start.1.min(curr.1) as f32 * z + img_rect.min.y;
                        let w = (start.0 - curr.0).abs() as f32 * z;
                        let h = (start.1 - curr.1).abs() as f32 * z;
                        painter.rect_stroke(
                            Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h)),
                            CornerRadius::ZERO,
                            stroke,
                            egui::StrokeKind::Outside,
                        );
                    }
                    Tool::Oval => {
                        let x = start.0.min(curr.0) as f32 * z + img_rect.min.x;
                        let y = start.1.min(curr.1) as f32 * z + img_rect.min.y;
                        let w = (start.0 - curr.0).abs() as f32 * z;
                        let h = (start.1 - curr.1).abs() as f32 * z;
                        draw_ellipse_stroke(
                            &painter,
                            Pos2::new(x + w / 2.0, y + h / 2.0),
                            w / 2.0,
                            h / 2.0,
                            stroke,
                        );
                    }
                    Tool::Line => {
                        let p1 = Pos2::new(
                            start.0 as f32 * z + img_rect.min.x,
                            start.1 as f32 * z + img_rect.min.y,
                        );
                        let p2 = Pos2::new(
                            curr.0 as f32 * z + img_rect.min.x,
                            curr.1 as f32 * z + img_rect.min.y,
                        );
                        painter.line_segment([p1, p2], stroke);
                    }
                    _ => {}
                }
            }
        } else if let Some(doc) = self.session.get_image(image_id) {
            // Draw committed ROI from document
            if let Some(roi) = &doc.roi {
                match roi {
                    Roi::Rect(r) => {
                        let rx = r.x as f32 * z + img_rect.min.x;
                        let ry = r.y as f32 * z + img_rect.min.y;
                        let rw = r.width as f32 * z;
                        let rh = r.height as f32 * z;
                        painter.rect_stroke(
                            Rect::from_min_size(Pos2::new(rx, ry), Vec2::new(rw, rh)),
                            CornerRadius::ZERO,
                            stroke,
                            egui::StrokeKind::Outside,
                        );
                    }
                    Roi::Oval(r) => {
                        let rx = r.x as f32 * z + img_rect.min.x;
                        let ry = r.y as f32 * z + img_rect.min.y;
                        let rw = r.width as f32 * z;
                        let rh = r.height as f32 * z;
                        draw_ellipse_stroke(
                            &painter,
                            Pos2::new(rx + rw / 2.0, ry + rh / 2.0),
                            rw / 2.0,
                            rh / 2.0,
                            stroke,
                        );
                    }
                    Roi::Line(l) => {
                        let p1 = Pos2::new(
                            l.x1 as f32 * z + img_rect.min.x,
                            l.y1 as f32 * z + img_rect.min.y,
                        );
                        let p2 = Pos2::new(
                            l.x2 as f32 * z + img_rect.min.x,
                            l.y2 as f32 * z + img_rect.min.y,
                        );
                        painter.line_segment([p1, p2], stroke);
                    }
                    Roi::Polygon(p) => {
                        let pts: Vec<Pos2> = p
                            .xpoints
                            .iter()
                            .zip(&p.ypoints)
                            .map(|(&x, &y)| {
                                Pos2::new(
                                    x as f32 * z + img_rect.min.x,
                                    y as f32 * z + img_rect.min.y,
                                )
                            })
                            .collect();
                        if pts.len() >= 3 {
                            for i in 0..pts.len() {
                                let next = if i + 1 == pts.len() { 0 } else { i + 1 };
                                painter.line_segment([pts[i], pts[next]], stroke);
                            }
                        }
                    }
                }
            }
        }

        // Put the view back into self.image_views
        self.image_views.insert(image_id, view);

        // Dispatch deferred actions
        if let Some(slice_idx) = new_slice {
            let _ = self.set_active_window(image_id);
            let _ = self.set_slice_active(slice_idx);
        }

        if let Some((ix, iy)) = hover_pixel {
            self.update_status_for_pixel(image_id, ix, iy);
        }

        if was_clicked {
            let _ = self.set_active_window(image_id);
        }

        if let Some(action) = drag_action {
            let _ = self.set_active_window(image_id);
            match action {
                DragAction::Rect(x, y, w, h) => {
                    let _ = self.dispatch_rect_roi(x, y, w, h);
                }
                DragAction::Oval(x, y, w, h) => {
                    let _ = self.dispatch_oval_roi(x, y, w, h);
                }
                DragAction::Line(x1, y1, x2, y2) => {
                    let _ = self.dispatch_line_roi(x1, y1, x2, y2);
                }
            }
        }
    }
}

pub fn draw_ellipse_stroke(
    painter: &egui::Painter,
    center: Pos2,
    rad_x: f32,
    rad_y: f32,
    stroke: Stroke,
) {
    if rad_x <= 0.0 || rad_y <= 0.0 {
        return;
    }
    let n = 64;
    let pts: Vec<Pos2> = (0..n)
        .map(|i| {
            let a = i as f32 * std::f32::consts::TAU / n as f32;
            Pos2::new(center.x + rad_x * a.cos(), center.y + rad_y * a.sin())
        })
        .collect();
    for i in 0..pts.len() {
        let next = if i + 1 == pts.len() { 0 } else { i + 1 };
        painter.line_segment([pts[i], pts[next]], stroke);
    }
}
