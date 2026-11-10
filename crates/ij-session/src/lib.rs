pub mod generated_commands;

use generated_commands::{COMMAND_SPECS, CommandSpec, ExecutionTier};
pub use ij_bridge::plugin::{
    PluginBridge, PluginBridgeError, PluginFilterInputError, PluginFilterRequest, PluginFilterRun,
};
use ij_core::{
    CoreError, DisplayRange, ImageDocument, MeasurementSummary, PixelPlane, ResultsTable,
    ThresholdLevels,
};
use ij_io::{IoError, load_tiff, save_tiff};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ImageId(pub u32);

// PlugInFilter.NO_CHANGES and PlugInFilter.NO_UNDO
// (original_imagej/ij/plugin/filter/PlugInFilter.java lines 58-60).
const PLUGIN_NO_CHANGES: i32 = 128;
const PLUGIN_NO_UNDO: i32 = 256;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("core error: {0}")]
    Core(#[from] CoreError),
    #[error("I/O error: {0}")]
    Io(#[from] IoError),
    #[error("file error: {0}")]
    File(#[from] std::io::Error),
    #[error("no active image in session")]
    NoActiveImage,
    #[error("unknown command label: {0}")]
    UnknownCommand(String),
    #[error("plugin bridge error: {0}")]
    Plugin(#[from] ij_bridge::plugin::PluginBridgeError),
    #[error("no plugin bridge is configured in this session")]
    PluginBridgeUnavailable,
    #[error("unsupported document state for the narrow PlugInFilter subset: {0}")]
    UnsupportedPluginFilterState(String),
    #[error("invalid PlugInFilter request: {0}")]
    InvalidPluginFilterRequest(String),
    #[error("plugin filter result is stale: image {id} changed during execution ({reason})")]
    StalePluginFilterResult { id: u32, reason: String },
    #[error("plugin filter output does not match the validated run ticket: {0}")]
    InvalidPluginFilterOutput(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    NewRamp8Bit {
        title: String,
        width: u32,
        height: u32,
    },
    OpenTiff {
        path: PathBuf,
    },
    SaveTiff {
        path: PathBuf,
    },
    SetDisplayRange {
        min: f64,
        max: f64,
    },
    MakeRectRoi {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    MakeOvalRoi {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    MakePolygonRoi {
        points: Vec<(i32, i32)>,
    },
    MakeLineRoi {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
    },
    ClearRoi,
    SetDimensions {
        channels: u32,
        slices: u32,
        frames: u32,
    },
    SetPosition {
        channel: i32,
        slice: i32,
        frame: i32,
    },
    /// The macro function `setSlice(n)` (original_imagej/ij/macro/
    /// Functions.java 3544-3560). It is recorded as `setSlice(n);`.
    SetSlice {
        index: i32,
    },
    /// Interactive stack navigation from the slice slider or keyboard. This
    /// mirrors `ImagePlus.setSlice(n)` and, like ImageJ, is never recorded:
    /// only the explicit Image > Stack > Set Slice... command is recorded
    /// (original_imagej/ij/plugin/frame/Recorder.java 603-604).
    NavigateSlice {
        index: i32,
    },
    Invert,
    Smooth,
    Sharpen,
    FindEdges,
    GaussianBlur {
        sigma: f64,
    },
    /// `run("Median...", "radius=N")`; 8-bit grayscale only.
    Median {
        radius: f64,
    },
    /// `run("Minimum...", "radius=N")`; 8-bit grayscale only.
    Minimum {
        radius: f64,
    },
    /// `run("Maximum...", "radius=N")`; 8-bit grayscale only.
    Maximum {
        radius: f64,
    },
    /// `run("Despeckle")`, i.e. the median filter with radius 1; 8-bit
    /// grayscale only.
    Despeckle,
    /// `ImageProcessor.setAutoThreshold(String)`; only "IsoData" and "Otsu"
    /// are implemented, for single-slice 8-bit and 16-bit grayscale images.
    /// `histogram_16` is ImageJ's "16" keyword (the ThresholdAdjuster's
    /// 16-bit histogram checkbox) and is rejected for 8-bit images.
    AutoThreshold {
        method: String,
        dark_background: bool,
        histogram_16: bool,
    },
    /// `resetThreshold()` / ThresholdAdjuster's Reset button. Clears the
    /// active threshold without touching pixels.
    ResetThreshold,
    InvertStack,
    Undo,
    Measure,
    ClearResults,
    SaveResultsTsv {
        path: PathBuf,
    },
    /// Runs one explicitly named Java `PlugInFilter` class through the narrow
    /// `ij-bridge` plugin host.
    ///
    /// This is deliberately *not* a generic ImageJ menu/GUI compatibility
    /// entry point: only `class_name`, the supplied `classpath`, and `arg` are
    /// used. The document must be a single-plane 8-bit grayscale image with
    /// default calibration/display-range metadata and an optional in-bounds
    /// rectangular ROI. Unsupported inputs are rejected before the JVM starts.
    RunPluginFilter {
        class_name: String,
        classpath: Vec<PathBuf>,
        arg: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommandOutcome {
    ImageOpened(ImageId),
    Saved(PathBuf),
    Updated,
    Measured(MeasurementSummary),
    ResultsCleared,
    /// New `minThreshold`/`maxThreshold` state; pixels are unchanged.
    Thresholded(ThresholdLevels),
    /// `resetThreshold()` succeeded; threshold state was cleared and pixels
    /// were untouched.
    ThresholdReset,
    /// `Undo` restored a snapshot.
    Undone,
    /// `Undo` had no snapshot (or the active image is not undoable), so the
    /// image and its dirty state are unchanged.
    NothingToUndo,
    PluginFiltered(PluginFilterOutcome),
}

/// Reported result of a successful narrow `PlugInFilter` run. The processed
/// pixels live in the session document; this records what the Java host did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginFilterOutcome {
    pub class_name: String,
    pub arg: String,
    /// Flags returned by the filter's `setup`, as observed by the host.
    pub filter_flags: i32,
    pub run_invocations: u32,
    pub final_setup_called: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Opaque proof that a plugin run was prepared against one exact image state.
///
/// The ticket binds the target [`ImageId`], the per-image revision, the full
/// pre-run document snapshot (pixels, ROI, display range, threshold,
/// dimensions, position and undo snapshot), and the pre-run planes used for
/// Undo. It is produced only by [`Session::prepare_plugin_filter`] so callers
/// cannot stage an inconsistent caller-supplied snapshot.
#[derive(Debug, Clone)]
pub struct PluginFilterTicket {
    target: ImageId,
    revision: u64,
    expected: ImageDocument,
    pre_slices: Vec<PixelPlane>,
    class_name: String,
    arg: String,
}

impl PluginFilterTicket {
    pub fn target(&self) -> ImageId {
        self.target
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn class_name(&self) -> &str {
        &self.class_name
    }
}

/// A request paired with the ticket that must be used to apply its result.
#[derive(Debug, Clone)]
pub struct PreparedPluginFilter {
    pub ticket: PluginFilterTicket,
    pub request: PluginFilterRequest,
}

#[derive(Default)]
pub struct Session {
    next_id: u32,
    images: BTreeMap<ImageId, ImageDocument>,
    active_image: Option<ImageId>,
    pub results: ResultsTable,
    pub macro_recorder: Vec<String>,
    dirty_images: BTreeSet<ImageId>,
    /// Last TIFF path this document was opened from or successfully saved to.
    /// `FileSaver.save()` re-saves a TIFF opened from disk at this path instead
    /// of showing a Save dialog (original_imagej/ij/io/FileSaver.java 42-69).
    saved_tiff_paths: BTreeMap<ImageId, PathBuf>,
    /// Per-image change counter. Any mutable access or session mutation bumps
    /// it, so a result prepared before the change is rejected as stale.
    image_revisions: BTreeMap<ImageId, u64>,
    /// Configured explicitly by the caller; plugin execution is never enabled
    /// implicitly by a repository checkout.
    plugin_bridge: Option<Arc<PluginBridge>>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables explicit Java `PlugInFilter` execution for this session.
    /// Plugin execution is opt-in so a plain `Session` never starts a JVM.
    pub fn with_plugin_bridge(mut self, bridge: PluginBridge) -> Self {
        self.plugin_bridge = Some(Arc::new(bridge));
        self
    }

    pub fn set_plugin_bridge(&mut self, bridge: PluginBridge) {
        self.plugin_bridge = Some(Arc::new(bridge));
    }

    pub fn plugin_bridge(&self) -> Option<&PluginBridge> {
        self.plugin_bridge.as_deref()
    }

    pub fn clone_plugin_bridge(&self) -> Option<Arc<PluginBridge>> {
        self.plugin_bridge.clone()
    }

    /// Runs the bridge's single preflight validator against one document.
    ///
    /// The UI uses this for compatibility status, and `prepare_plugin_filter`
    /// uses it before starting a worker, so neither path can drift from the
    /// bridge's actual supported subset (including overflow-safe ROI bounds).
    pub fn plugin_filter_preflight(&self, id: ImageId) -> Result<(), SessionError> {
        let doc = self.images.get(&id).ok_or(SessionError::NoActiveImage)?;
        if let Some(levels) = &doc.threshold {
            return Err(SessionError::UnsupportedPluginFilterState(format!(
                "active threshold {}..{} cannot be preserved by the narrow \
                 PlugInFilter subset",
                levels.lower, levels.upper
            )));
        }
        ij_bridge::plugin::validate_filter_request(doc)
            .map_err(|error| SessionError::Plugin(PluginBridgeError::UnsupportedFilterInput(error)))
    }

    /// Pre-validates an image and returns the request plus an opaque ticket
    /// that binds the eventual result to this exact pre-run state.
    pub fn prepare_plugin_filter(
        &self,
        id: ImageId,
        class_name: &str,
        classpath: &[PathBuf],
        arg: &str,
    ) -> Result<PreparedPluginFilter, SessionError> {
        if class_name.trim().is_empty() {
            return Err(SessionError::InvalidPluginFilterRequest(
                "class name must not be empty".to_string(),
            ));
        }
        self.plugin_filter_preflight(id)?;
        let doc = self.images.get(&id).ok_or(SessionError::NoActiveImage)?;

        let request = PluginFilterRequest::new(class_name, arg, doc.clone())
            .with_classpath_entries(classpath.iter());
        let ticket = PluginFilterTicket {
            target: id,
            revision: self.image_revision(id),
            expected: doc.clone(),
            pre_slices: doc.slices.clone(),
            class_name: class_name.to_string(),
            arg: arg.to_string(),
        };
        Ok(PreparedPluginFilter { ticket, request })
    }

    /// Applies a successful `PluginFilterRun` only when its ticket still
    /// matches the target image. Identity, output shape/type/metadata and the
    /// snapshot/revision are all validated before any mutation.
    pub fn apply_plugin_filter_run(
        &mut self,
        ticket: PluginFilterTicket,
        run: PluginFilterRun,
    ) -> Result<PluginFilterOutcome, SessionError> {
        let PluginFilterRun {
            image,
            class_name: run_class,
            arg: run_arg,
            filter_flags,
            run_invocations,
            final_setup_called,
            stdout,
            stderr,
            ..
        } = run;

        validate_plugin_filter_output(&ticket, &run_class, &run_arg, run_invocations, &image)?;

        let doc = self.images.get(&ticket.target).ok_or_else(|| {
            SessionError::StalePluginFilterResult {
                id: ticket.target.0,
                reason: "target image was closed during the run".to_string(),
            }
        })?;
        if self.image_revision(ticket.target) != ticket.revision {
            return Err(SessionError::StalePluginFilterResult {
                id: ticket.target.0,
                reason: "image revision changed during the run".to_string(),
            });
        }
        if doc != &ticket.expected {
            return Err(SessionError::StalePluginFilterResult {
                id: ticket.target.0,
                reason: "pixels, ROI, display range, threshold, dimensions, position or undo state changed during the run"
                    .to_string(),
            });
        }

        let doc = self
            .images
            .get_mut(&ticket.target)
            .ok_or(SessionError::NoActiveImage)?;
        doc.slices = image.slices;
        let (snapshot, mark_dirty) =
            plugin_undo_policy(filter_flags, ticket.pre_slices, doc.snapshot.take());
        doc.snapshot = snapshot;
        if mark_dirty {
            self.dirty_images.insert(ticket.target);
        }
        self.bump_image_revision(ticket.target);

        let safe_class = run_class.replace(['\n', '\r'], " ");
        self.macro_recorder.push(format!(
            "// Java plugin filter {safe_class}: macro replay is not supported"
        ));

        Ok(PluginFilterOutcome {
            class_name: run_class,
            arg: run_arg,
            filter_flags,
            run_invocations,
            final_setup_called,
            stdout,
            stderr,
        })
    }

    pub fn lookup_command_spec(label: &str) -> Option<&'static CommandSpec> {
        COMMAND_SPECS.iter().find(|spec| spec.label == label)
    }

    pub fn active_doc(&self) -> Result<&ImageDocument, SessionError> {
        let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
        self.images.get(&id).ok_or(SessionError::NoActiveImage)
    }

    pub fn active_doc_mut(&mut self) -> Result<&mut ImageDocument, SessionError> {
        let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
        self.bump_image_revision(id);
        self.images.get_mut(&id).ok_or(SessionError::NoActiveImage)
    }

    pub fn dispatch(&mut self, cmd: Command) -> Result<CommandOutcome, SessionError> {
        match cmd {
            Command::NewRamp8Bit {
                title,
                width,
                height,
            } => {
                let doc = ImageDocument::new_ramp_8bit(&title, width, height)?;
                self.macro_recorder.push(format!(
                    "newImage(\"{title}\", \"8-bit ramp\", {width}, {height}, 1);"
                ));
                let id = self.insert_doc(doc);
                self.dirty_images.insert(id);
                Ok(CommandOutcome::ImageOpened(id))
            }
            Command::OpenTiff { path } => {
                let remembered_path = std::path::absolute(&path)?;
                let doc = load_tiff(&path)?;
                self.macro_recorder
                    .push(format!("open(\"{}\");", path.display()));
                let id = self.insert_doc(doc);
                self.dirty_images.remove(&id);
                self.saved_tiff_paths.insert(id, remembered_path);
                Ok(CommandOutcome::ImageOpened(id))
            }
            Command::SaveTiff { path } => {
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                self.save_image(id, &path)?;
                Ok(CommandOutcome::Saved(path))
            }
            Command::SetDisplayRange { min, max } => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let range = DisplayRange { min, max };
                    let changed = doc.display_range != range;
                    doc.display_range = range;
                    changed
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder
                    .push(format!("setMinAndMax({min}, {max});"));
                Ok(CommandOutcome::Updated)
            }
            Command::MakeRectRoi {
                x,
                y,
                width,
                height,
            } => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let before = doc.roi.clone();
                    doc.set_rect_roi(x, y, width, height)?;
                    doc.roi != before
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder
                    .push(format!("makeRectangle({x}, {y}, {width}, {height});"));
                Ok(CommandOutcome::Updated)
            }
            Command::MakeOvalRoi {
                x,
                y,
                width,
                height,
            } => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let before = doc.roi.clone();
                    doc.set_oval_roi(x, y, width, height)?;
                    doc.roi != before
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder
                    .push(format!("makeOval({x}, {y}, {width}, {height});"));
                Ok(CommandOutcome::Updated)
            }
            Command::MakePolygonRoi { points } => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let before = doc.roi.clone();
                    doc.set_polygon_roi(&points)?;
                    doc.roi != before
                };
                if changed {
                    self.mark_active_dirty();
                }
                let coords = points
                    .iter()
                    .map(|(x, y)| format!("{x}, {y}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.macro_recorder.push(format!("makePolygon({coords});"));
                Ok(CommandOutcome::Updated)
            }
            Command::MakeLineRoi { x1, y1, x2, y2 } => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let before = doc.roi.clone();
                    doc.set_line_roi(x1, y1, x2, y2);
                    doc.roi != before
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder
                    .push(format!("makeLine({x1}, {y1}, {x2}, {y2});"));
                Ok(CommandOutcome::Updated)
            }
            Command::ClearRoi => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let changed = doc.roi.is_some();
                    doc.clear_roi();
                    changed
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder.push("run(\"Select None\");".into());
                Ok(CommandOutcome::Updated)
            }
            Command::SetDimensions {
                channels,
                slices,
                frames,
            } => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let before = (doc.n_channels, doc.n_slices, doc.n_frames);
                    doc.set_dimensions(channels, slices, frames)?;
                    before != (doc.n_channels, doc.n_slices, doc.n_frames)
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder
                    .push(format!("setDimensions({channels}, {slices}, {frames});"));
                Ok(CommandOutcome::Updated)
            }
            Command::SetPosition {
                channel,
                slice,
                frame,
            } => {
                let doc = self.active_doc_mut()?;
                doc.set_position(channel, slice, frame);
                self.macro_recorder
                    .push(format!("setPosition({channel}, {slice}, {frame});"));
                Ok(CommandOutcome::Updated)
            }
            Command::SetSlice { index } => {
                let doc = self.active_doc_mut()?;
                // The macro function setSlice, not ImagePlus.setSlice: on a
                // hyperstack it moves to the C/Z/T position for that index
                // (Functions.java 3544-3560).
                doc.set_slice_macro(index)?;
                self.macro_recorder.push(format!("setSlice({index});"));
                Ok(CommandOutcome::Updated)
            }
            Command::NavigateSlice { index } => {
                let doc = self.active_doc_mut()?;
                // ImagePlus.setSlice is the interactive display path; it
                // discards the processor snapshot but records no macro.
                doc.set_slice(index)?;
                Ok(CommandOutcome::Updated)
            }
            Command::Invert => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.invert();
                // Running Invert on a stack asks "Process Stack?"; the
                // slice-only answer is recorded as the `slice` option
                // (IJ.java 1287-1291). A single image records no options.
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Invert\", \"slice\");".into());
                } else {
                    self.macro_recorder.push("run(\"Invert\");".into());
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::Smooth => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                // Smooth only touches the current slice: that is the "slice"
                // answer to the Process Stack? dialog, which the recorder saves
                // as the `slice` option (IJ.java 1287-1291).
                doc.smooth()?;
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Smooth\", \"slice\");".into());
                } else {
                    self.macro_recorder.push("run(\"Smooth\");".into());
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::Sharpen => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                // Sharpen only touches the current slice: that is the "slice"
                // answer to the Process Stack? dialog, which the recorder saves
                // as the `slice` option (IJ.java 1287-1291).
                doc.sharpen()?;
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Sharpen\", \"slice\");".into());
                } else {
                    self.macro_recorder.push("run(\"Sharpen\");".into());
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::FindEdges => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                // Find Edges only touches the current slice: that is the
                // "slice" answer to the Process Stack? dialog, which the
                // recorder saves as the `slice` option (IJ.java 1287-1291).
                doc.find_edges()?;
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Find Edges\", \"slice\");".into());
                } else {
                    self.macro_recorder.push("run(\"Find Edges\");".into());
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::GaussianBlur { sigma } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                // GaussianBlur only touches the current slice: that is the
                // "slice" answer to the Process Stack? dialog, which the
                // recorder appends as the `slice` option (IJ.java 1287-1291).
                doc.gaussian_blur(sigma)?;
                // Recorder.recordOption("sigma", value) uses the dialog field
                // text, and GenericDialog.trim strips trailing ".0"/".00"
                // (Recorder.java 472-480, GenericDialog.java 1243-1249).
                let sigma = format!("{sigma}");
                if is_stack {
                    self.macro_recorder.push(format!(
                        "run(\"Gaussian Blur...\", \"sigma={sigma} slice\");"
                    ));
                } else {
                    self.macro_recorder
                        .push(format!("run(\"Gaussian Blur...\", \"sigma={sigma}\");"));
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::Median { radius } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                // Only the current slice is filtered: that is the "slice"
                // answer to the Process Stack? dialog, which IJ.setupDialog
                // records as the `slice` option (IJ.java lines 1287-1291).
                doc.rank_filter(ij_core::RankFilterKind::Median, radius)?;
                // GenericDialog records the "Radius" field and trims trailing
                // ".0"/".00" (GenericDialog.java lines 1230-1250,
                // Recorder.java lines 472-482).
                let suffix = if is_stack { " slice" } else { "" };
                self.macro_recorder
                    .push(format!("run(\"Median...\", \"radius={radius}{suffix}\");"));
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::Minimum { radius } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.rank_filter(ij_core::RankFilterKind::Minimum, radius)?;
                let suffix = if is_stack { " slice" } else { "" };
                self.macro_recorder
                    .push(format!("run(\"Minimum...\", \"radius={radius}{suffix}\");"));
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::Maximum { radius } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.rank_filter(ij_core::RankFilterKind::Maximum, radius)?;
                let suffix = if is_stack { " slice" } else { "" };
                self.macro_recorder
                    .push(format!("run(\"Maximum...\", \"radius={radius}{suffix}\");"));
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::Despeckle => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.despeckle()?;
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Despeckle\", \"slice\");".into());
                } else {
                    self.macro_recorder.push("run(\"Despeckle\");".into());
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::AutoThreshold {
                method,
                dark_background,
                histogram_16,
            } => {
                let parsed = ij_core::AutoThresholdMethod::parse(&method)?;
                let (levels, display_range_changed) = {
                    let doc = self.active_doc_mut()?;
                    let display_range_before = doc.display_range;
                    let levels =
                        doc.set_auto_threshold_with_options(parsed, dark_background, histogram_16)?;
                    (levels, doc.display_range != display_range_before)
                };
                // 16-bit default mode calls resetMinAndMax(), which changes
                // the display range that FileSaver persists (ImageProcessor
                // .java lines 642-647). Threshold state itself is not saved.
                if display_range_changed {
                    self.mark_active_dirty();
                }
                // ThresholdAdjuster.autoSetLevels builds
                // `method + (dark?" dark":"") + (hist16?" 16-bit":"")` and
                // records it with Recorder.record("setAutoThreshold", ...)
                // (ThresholdAdjuster.java lines 502-528).
                let mut options = parsed.as_str().to_string();
                if dark_background {
                    options.push_str(" dark");
                }
                if histogram_16 {
                    options.push_str(" 16-bit");
                }
                self.macro_recorder
                    .push(format!("setAutoThreshold(\"{options}\");"));
                Ok(CommandOutcome::Thresholded(levels))
            }
            Command::ResetThreshold => {
                let doc = self.active_doc_mut()?;
                doc.threshold = None;
                // ThresholdAdjuster.reset calls Recorder.record("resetThreshold")
                // (ThresholdAdjuster.java 740-745).
                self.macro_recorder.push("resetThreshold();".into());
                Ok(CommandOutcome::ThresholdReset)
            }
            Command::InvertStack => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.invert_stack();
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Invert\", \"stack\");".into());
                } else {
                    self.macro_recorder.push("run(\"Invert\");".into());
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::Undo => {
                let doc = self.active_doc_mut()?;
                if doc.undo() {
                    self.macro_recorder.push("run(\"Undo\");".into());
                    self.mark_active_dirty();
                    Ok(CommandOutcome::Undone)
                } else {
                    // Undo.undo does nothing when there is no snapshot
                    // (Undo.java 121-132); report that instead of claiming
                    // success and dirtying the document.
                    Ok(CommandOutcome::NothingToUndo)
                }
            }
            Command::Measure => {
                let stats = self.active_doc()?.measure()?;
                self.results.push(stats.clone());
                self.macro_recorder.push("run(\"Measure\");".into());
                Ok(CommandOutcome::Measured(stats))
            }
            Command::ClearResults => {
                self.results.clear();
                self.macro_recorder.push("run(\"Clear Results\");".into());
                Ok(CommandOutcome::ResultsCleared)
            }
            Command::SaveResultsTsv { path } => {
                let tsv = self.results.to_tsv_basic();
                fs::write(&path, tsv)?;
                self.macro_recorder
                    .push(format!("saveAs(\"Results\", \"{}\");", path.display()));
                Ok(CommandOutcome::Saved(path))
            }
            Command::RunPluginFilter {
                class_name,
                classpath,
                arg,
            } => {
                let bridge = self
                    .plugin_bridge
                    .as_ref()
                    .ok_or(SessionError::PluginBridgeUnavailable)?;
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;

                let prepared = self.prepare_plugin_filter(id, &class_name, &classpath, &arg)?;
                let run = bridge.run_filter(&prepared.request)?;
                let outcome = self.apply_plugin_filter_run(prepared.ticket, run)?;
                Ok(CommandOutcome::PluginFiltered(outcome))
            }
        }
    }

    pub fn active_image_id(&self) -> Option<ImageId> {
        self.active_image
    }

    pub fn set_active_image(&mut self, id: ImageId) -> Result<(), SessionError> {
        if self.images.contains_key(&id) {
            self.active_image = Some(id);
            Ok(())
        } else {
            Err(SessionError::NoActiveImage)
        }
    }

    pub fn image_ids(&self) -> Vec<ImageId> {
        self.images.keys().copied().collect()
    }

    pub fn get_image(&self, id: ImageId) -> Option<&ImageDocument> {
        self.images.get(&id)
    }

    pub fn get_image_mut(&mut self, id: ImageId) -> Option<&mut ImageDocument> {
        if self.images.contains_key(&id) {
            self.bump_image_revision(id);
        }
        self.images.get_mut(&id)
    }

    pub fn image_revision(&self, id: ImageId) -> u64 {
        self.image_revisions.get(&id).copied().unwrap_or(0)
    }

    /// Returns the per-image revision if the image exists in the session.
    pub fn get_image_revision(&self, id: ImageId) -> Option<u64> {
        self.image_revisions.get(&id).copied()
    }

    /// Increments the revision counter for the specified image.
    pub fn bump_image_revision(&mut self, id: ImageId) {
        if let Some(revision) = self.image_revisions.get_mut(&id) {
            *revision = revision.wrapping_add(1);
        }
    }

    pub fn is_dirty(&self, id: ImageId) -> bool {
        self.dirty_images.contains(&id)
    }

    pub fn set_dirty(&mut self, id: ImageId, dirty: bool) {
        if dirty {
            self.dirty_images.insert(id);
        } else {
            self.dirty_images.remove(&id);
        }
    }

    pub fn any_dirty(&self) -> bool {
        !self.dirty_images.is_empty()
    }

    pub fn save_image(&mut self, id: ImageId, path: &std::path::Path) -> Result<(), SessionError> {
        let remembered_path = std::path::absolute(path)?;
        let doc = self.images.get(&id).ok_or(SessionError::NoActiveImage)?;
        save_tiff(path, doc)?;
        self.dirty_images.remove(&id);
        self.saved_tiff_paths.insert(id, remembered_path);
        self.macro_recorder
            .push(format!("saveAs(\"Tiff\", \"{}\");", path.display()));
        Ok(())
    }

    /// The TIFF destination a close prompt should re-save to, or `None` for a
    /// document that has never been opened from or saved to a file.
    pub fn image_path(&self, id: ImageId) -> Option<&std::path::Path> {
        self.saved_tiff_paths.get(&id).map(PathBuf::as_path)
    }

    fn mark_active_dirty(&mut self) {
        if let Some(id) = self.active_image {
            self.dirty_images.insert(id);
        }
    }

    pub fn close_image(&mut self, id: ImageId) -> bool {
        self.dirty_images.remove(&id);
        self.saved_tiff_paths.remove(&id);
        self.image_revisions.remove(&id);
        let removed = self.images.remove(&id).is_some();
        if self.active_image == Some(id) {
            self.active_image = self.images.keys().last().copied();
        }
        removed
    }

    pub fn close_all_images(&mut self) {
        self.dirty_images.clear();
        self.saved_tiff_paths.clear();
        self.image_revisions.clear();
        self.images.clear();
        self.active_image = None;
    }

    pub fn insert_doc(&mut self, doc: ImageDocument) -> ImageId {
        self.next_id += 1;
        let id = ImageId(self.next_id);
        self.images.insert(id, doc);
        self.image_revisions.insert(id, 0);
        self.active_image = Some(id);
        id
    }
}

/// Validates the host-shaped result against the ticket before the session
/// document is touched. The bridge already rejects malformed engine output,
/// but `PluginFilterRun` is public and a caller (or a future host) must not be
/// able to stage a wrong-image, wrong-type or wrong-metadata plane.
fn validate_plugin_filter_output(
    ticket: &PluginFilterTicket,
    class_name: &str,
    arg: &str,
    run_invocations: u32,
    image: &ImageDocument,
) -> Result<(), SessionError> {
    if class_name != ticket.class_name {
        return Err(SessionError::InvalidPluginFilterOutput(format!(
            "class name mismatch: expected {}, got {class_name}",
            ticket.class_name
        )));
    }
    if arg != ticket.arg {
        return Err(SessionError::InvalidPluginFilterOutput(format!(
            "argument mismatch: expected {:?}, got {:?}",
            ticket.arg, arg
        )));
    }
    if run_invocations == 0 {
        return Err(SessionError::InvalidPluginFilterOutput(
            "host reported success without calling run(ip)".to_string(),
        ));
    }

    let expected = &ticket.expected;
    if image.title != expected.title {
        return Err(SessionError::InvalidPluginFilterOutput(format!(
            "target identity mismatch: expected image {:?}, got {:?}",
            expected.title, image.title
        )));
    }
    if image.width != expected.width || image.height != expected.height {
        return Err(SessionError::InvalidPluginFilterOutput(format!(
            "dimension mismatch: expected {}x{}, got {}x{}",
            expected.width, expected.height, image.width, image.height
        )));
    }
    let single_plane = image.slices.len() == 1
        && image.n_channels == 1
        && image.n_slices == 1
        && image.n_frames == 1
        && image.current_slice == 1
        && image.position == [1, 1, 1];
    if !single_plane {
        return Err(SessionError::InvalidPluginFilterOutput(
            "output is not a single-plane image".to_string(),
        ));
    }
    let expected_len = image.width as usize * image.height as usize;
    match image.plane() {
        PixelPlane::Gray8(pixels) if pixels.len() == expected_len => {}
        PixelPlane::Gray8(pixels) => {
            return Err(SessionError::InvalidPluginFilterOutput(format!(
                "pixel buffer length {} does not match {}x{}",
                pixels.len(),
                image.width,
                image.height
            )));
        }
        other => {
            return Err(SessionError::InvalidPluginFilterOutput(format!(
                "expected an 8-bit grayscale output plane, got {other:?}"
            )));
        }
    }
    if image.calibration != expected.calibration
        || image.display_range != expected.display_range
        || image.roi != expected.roi
        || image.threshold != expected.threshold
    {
        return Err(SessionError::InvalidPluginFilterOutput(
            "calibration, display range, selection or threshold metadata changed".to_string(),
        ));
    }

    Ok(())
}

/// Undo and dirty-state policy for a successful single-image `PlugInFilter`
/// run, matching `PlugInFilterRunner` (original_imagej lines 107-121 and
/// 169-172).
///
/// `NO_CHANGES` leaves both the existing Undo state and the changes flag
/// untouched. Otherwise the runner either snapshots the processor before
/// `run(ip)` or, with `NO_UNDO`, calls `Undo.reset()`.
fn plugin_undo_policy(
    filter_flags: i32,
    pre_filter_slices: Vec<PixelPlane>,
    previous_snapshot: Option<Vec<PixelPlane>>,
) -> (Option<Vec<PixelPlane>>, bool) {
    if filter_flags & PLUGIN_NO_CHANGES != 0 {
        (previous_snapshot, false)
    } else if filter_flags & PLUGIN_NO_UNDO != 0 {
        (None, true)
    } else {
        (Some(pre_filter_slices), true)
    }
}

pub fn execution_tier_for_label(label: &str) -> Option<ExecutionTier> {
    Session::lookup_command_spec(label).map(|s| s.tier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_undo_policy_matches_pluginfilterrunner_flags() {
        let pre = vec![PixelPlane::Gray8(vec![1, 2, 3, 4])];
        let previous = vec![PixelPlane::Gray8(vec![9, 9, 9, 9])];

        // Default single-image filter: snapshot before run(ip), mark dirty.
        assert_eq!(
            plugin_undo_policy(0, pre.clone(), Some(previous.clone())),
            (Some(pre.clone()), true)
        );
        // NO_CHANGES leaves Undo and the changes flag alone.
        assert_eq!(
            plugin_undo_policy(PLUGIN_NO_CHANGES, pre.clone(), Some(previous.clone())),
            (Some(previous.clone()), false)
        );
        // NO_UNDO clears Undo but still changes the image.
        assert_eq!(
            plugin_undo_policy(PLUGIN_NO_UNDO, pre.clone(), Some(previous.clone())),
            (None, true)
        );
        // PlugInFilterRunner's outer NO_CHANGES check wins when both are set.
        assert_eq!(
            plugin_undo_policy(
                PLUGIN_NO_CHANGES | PLUGIN_NO_UNDO,
                pre,
                Some(previous.clone())
            ),
            (Some(previous), false)
        );
    }

    #[test]
    fn command_ledger_contains_measure_in_rust_tier1() {
        let spec = Session::lookup_command_spec("Measure").unwrap();
        assert_eq!(spec.tier, ExecutionTier::RustTier1);
        assert_eq!(spec.java_class, "ij.plugin.filter.Analyzer");
    }

    #[test]
    fn session_vertical_slice_records_macro_and_measures() {
        let mut session = Session::new();
        session
            .dispatch(Command::NewRamp8Bit {
                title: "ramp".into(),
                width: 16,
                height: 16,
            })
            .unwrap();
        session
            .dispatch(Command::MakeRectRoi {
                x: 2,
                y: 3,
                width: 8,
                height: 6,
            })
            .unwrap();
        let outcome = session.dispatch(Command::Measure).unwrap();
        match outcome {
            CommandOutcome::Measured(m) => {
                assert_eq!(m.area, 48.0);
                assert_eq!(m.mean, 88.0);
                assert_eq!(m.min, 32.0);
                assert_eq!(m.max, 144.0);
            }
            _ => panic!("expected Measured outcome"),
        }
        assert_eq!(session.macro_recorder.len(), 3);
    }

    #[test]
    fn session_records_oval_polygon_and_line_macros() {
        let mut session = Session::new();
        session
            .dispatch(Command::NewRamp8Bit {
                title: "ramp".into(),
                width: 16,
                height: 16,
            })
            .unwrap();
        session
            .dispatch(Command::MakeOvalRoi {
                x: 2,
                y: 3,
                width: 8,
                height: 6,
            })
            .unwrap();
        session
            .dispatch(Command::MakePolygonRoi {
                points: vec![(2, 2), (12, 3), (9, 11)],
            })
            .unwrap();
        session
            .dispatch(Command::MakeLineRoi {
                x1: 1,
                y1: 2,
                x2: 9,
                y2: 7,
            })
            .unwrap();

        assert_eq!(
            session.macro_recorder,
            vec![
                "newImage(\"ramp\", \"8-bit ramp\", 16, 16, 1);",
                "makeOval(2, 3, 8, 6);",
                "makePolygon(2, 2, 12, 3, 9, 11);",
                "makeLine(1, 2, 9, 7);",
            ]
        );
    }

    #[test]
    fn session_multi_window_management() {
        let mut session = Session::new();
        assert!(session.image_ids().is_empty());
        assert_eq!(session.active_image_id(), None);

        let id1 = match session
            .dispatch(Command::NewRamp8Bit {
                title: "ramp1".into(),
                width: 10,
                height: 10,
            })
            .unwrap()
        {
            CommandOutcome::ImageOpened(id) => id,
            _ => panic!("expected ImageOpened"),
        };

        let id2 = match session
            .dispatch(Command::NewRamp8Bit {
                title: "ramp2".into(),
                width: 20,
                height: 20,
            })
            .unwrap()
        {
            CommandOutcome::ImageOpened(id) => id,
            _ => panic!("expected ImageOpened"),
        };

        assert_eq!(session.image_ids(), vec![id1, id2]);
        assert_eq!(session.active_image_id(), Some(id2));
        assert_eq!(session.get_image(id1).unwrap().title, "ramp1");
        assert_eq!(session.get_image(id2).unwrap().title, "ramp2");

        session.set_active_image(id1).unwrap();
        assert_eq!(session.active_image_id(), Some(id1));

        assert!(session.close_image(id2));
        assert_eq!(session.image_ids(), vec![id1]);
        assert_eq!(session.active_image_id(), Some(id1));

        session.close_all_images();
        assert!(session.image_ids().is_empty());
        assert_eq!(session.active_image_id(), None);
    }
}
