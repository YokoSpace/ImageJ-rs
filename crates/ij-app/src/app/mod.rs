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

mod menu;
mod adjust_dialogs;
mod image_window;
mod plugin_ops;
mod plugins_ui;
mod close_guard;
mod shortcuts;
pub use self::menu::scrollable_menu;
pub use self::image_window::draw_ellipse_stroke;

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
    last_status_pixel: Option<(ImageId, i32, i32)>,
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
            last_status_pixel: None,
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

    pub fn crop_active(&mut self) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("Crop") {
            return Err(err);
        }
        match self.session.dispatch(Command::Crop) {
            Ok(_) => {
                self.status_message = "Crop".to_string();
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Crop failed: {e}");
                self.dialogs.last_error = Some(format!("Crop failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn convert_to_8bit_active(&mut self) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("8-bit") {
            return Err(err);
        }
        match self.session.dispatch(Command::ConvertTo8Bit) {
            Ok(_) => {
                self.status_message = "Converting to 8-bit...".to_string();
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("8-bit conversion failed: {e}");
                self.dialogs.last_error = Some(format!("8-bit conversion failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn convert_to_16bit_active(&mut self) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("16-bit") {
            return Err(err);
        }
        match self.session.dispatch(Command::ConvertTo16Bit) {
            Ok(_) => {
                self.status_message = "Converting to 16-bit...".to_string();
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("16-bit conversion failed: {e}");
                self.dialogs.last_error = Some(format!("16-bit conversion failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn convert_to_32bit_active(&mut self) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("32-bit") {
            return Err(err);
        }
        match self.session.dispatch(Command::ConvertTo32Bit) {
            Ok(_) => {
                self.status_message = "Converting to 32-bit...".to_string();
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("32-bit conversion failed: {e}");
                self.dialogs.last_error = Some(format!("32-bit conversion failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn convert_to_rgb_active(&mut self) -> Result<(), SessionError> {
        if let Some(err) = self.reject_while_closing("RGB Color") {
            return Err(err);
        }
        match self.session.dispatch(Command::ConvertToRgb) {
            Ok(_) => {
                self.status_message = "Converting to RGB Color...".to_string();
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("RGB Color conversion failed: {e}");
                self.dialogs.last_error = Some(format!("RGB Color conversion failed: {e}"));
                Err(e)
            }
        }
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
                if let Some((id, x, y)) = self.last_status_pixel {
                    if self.active_image_id() == Some(id) {
                        self.update_status_for_pixel(id, x, y);
                    }
                }
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
        if let Some(doc) = self.session.get_image(image_id) {
            if let Some(text) = crate::status_text::pixel_status(doc, x, y) {
                self.last_status_pixel = Some((image_id, x, y));
                self.status_message = text;
            }
        }
    }



}


impl eframe::App for ImageJApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_ui(ctx);
    }
}
