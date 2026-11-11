pub mod generated_commands;

use generated_commands::{COMMAND_SPECS, CommandSpec, ExecutionTier};
pub use ij_bridge::plugin::{
    PluginBridge, PluginBridgeError, PluginFilterInputError, PluginFilterRequest, PluginFilterRun,
};
use ij_core::{
    default_duplicate_title, display_histogram, CoreError, DisplayRange, ImageDocument,
    MeasurementConfig, MeasurementFlags, MeasurementSummary, PixelPlane, ProfileData, ResultsTable,
    Roi, ThresholdLevels,
};
use ij_io::{ImageFileFormat, IoError, detect_image_format, load_image, save_jpeg, save_png, save_tiff};
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
    #[error("active image has no saved file path")]
    NoSavedTiffPath,
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
    OpenImage {
        path: PathBuf,
    },
    /// `File > Revert`: reloads the active image from its last opened/saved
    /// TIFF path (ImagePlus.revert, original_imagej/ij/ImagePlus.java
    /// 2293-2333). The current ROI is restored after the pixels and metadata
    /// are reloaded, matching ImagePlus.revert's saveRoi/setRoi sequence.
    Revert,
    SaveTiff {
        path: PathBuf,
    },
    SavePng {
        path: PathBuf,
    },
    SaveJpeg {
        path: PathBuf,
    },
    SetDisplayRange {
        min: f64,
        max: f64,
    },
    PreviewDisplayRange {
        min: f64,
        max: f64,
    },
    AutoDisplayRange {
        min: f64,
        max: f64,
    },
    ResetDisplayRange {
        min: f64,
        max: f64,
    },
    ApplyDisplayLut,
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
    /// User-drawn freehand selection. It shares polygon geometry, but ImageJ
    /// does not record FREEROI construction (PolygonRoi.java:594-596).
    MakeFreehandRoi {
        points: Vec<(i32, i32)>,
    },
    /// Wand shares polygon geometry but records the original click command.
    MakeWandRoi {
        x: i32,
        y: i32,
        points: Vec<(i32, i32)>,
    },
    MakeLineRoi {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
    },
    SelectAll,
    ClearRoi,
    RestoreSelection,
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
    /// Interactive hyperstack navigation from the C/Z/T sliders. This
    /// mirrors `ImagePlus.setPosition(c, z, t)` and, like ImageJ, is never recorded:
    /// only the explicit macro command `setPosition(c, z, t)` is recorded
    /// (original_imagej/ij/plugin/frame/Recorder.java 603-604,
    /// original_imagej/ij/gui/StackWindow.java 294-306).
    NavigatePosition {
        channel: i32,
        slice: i32,
        frame: i32,
    },
    Invert,
    /// `run("Crop")` (Resizer.java crop branch). Crops every slice to the
    /// clipped area-selection bounds and records `run("Crop");`.
    Crop,
    /// `Image > Duplicate...` (Duplicator.java 36-135). `title: None` asks
    /// for a `WindowManager.getUniqueName`-style title; an explicit title is
    /// used verbatim. `duplicate_stack` copies the whole stack or the
    /// `first_slice`..=`last_slice` range (inclusive), otherwise only the
    /// current slice is copied.
    Duplicate {
        title: Option<String>,
        duplicate_stack: bool,
        first_slice: Option<u32>,
        last_slice: Option<u32>,
    },
    /// `run("8-bit")` (Converter.java 38-39). Converts every slice to 8-bit
    /// grayscale with scaling enabled and records `run("8-bit");`.
    ConvertTo8Bit,
    /// `run("16-bit")` (Converter.java 38-39). Converts every slice to 16-bit
    /// grayscale with scaling enabled and records `run("16-bit");`.
    ConvertTo16Bit,
    /// `run("32-bit")` (Converter.java 38-39). Converts every slice to 32-bit
    /// grayscale with scaling enabled and records `run("32-bit");`.
    ConvertTo32Bit,
    /// `run("RGB Color")` (Converter.java 38-39). Converts every slice to RGB
    /// through the displayed 8-bit image and records `run("RGB Color");`.
    ConvertToRgb,
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
    /// Interactive threshold slider update; never recorded as a macro command.
    PreviewThreshold {
        lower: f64,
        upper: f64,
    },
    /// Numeric Set button in the threshold palette.
    SetThreshold {
        lower: f64,
        upper: f64,
    },
    /// `resetThreshold()` / ThresholdAdjuster's Reset button. Clears the
    /// active threshold without touching pixels.
    ResetThreshold,
    InvertStack,
    /// Whole-stack answers to the "Process Stack?" prompt for the grayscale
    /// filters. Each one records ImageJ's `stack` option
    /// (original_imagej/ij/IJ.java 1287).
    SmoothStack,
    SharpenStack,
    FindEdgesStack,
    GaussianBlurStack {
        sigma: f64,
    },
    MedianStack {
        radius: f64,
    },
    MinimumStack {
        radius: f64,
    },
    MaximumStack {
        radius: f64,
    },
    DespeckleStack,
    Undo,
    /// `Analyze > Set Measurements...` updates session measurement options.
    SetMeasurements { flags: u32, precision: u8 },
    /// `Analyze > Set Scale...` (ScaleDialog.java 80-108). The dialog fields
    /// become `Calibration.pixelWidth = known/distance`,
    /// `pixelHeight = pixelWidth*aspect` and `unit`; a non-positive distance or
    /// known distance resets the calibration to pixels.
    SetScale {
        distance: f64,
        known: f64,
        aspect: f64,
        unit: String,
    },
    /// The Set Scale dialog's "Click to Remove Scale" button and the macro
    /// statement `Image.removeScale` (ImagePlus.java 2206-2216).
    RemoveScale,
    Measure,
    Histogram,
    /// `Analyze > Plot Profile` (`K`). The document is unchanged; only the
    /// profile data is returned and `run("Plot Profile");` is recorded.
    PlotProfile,
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
    HistogramComputed(HistogramSummary),
    ProfileComputed(ProfileData),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistogramSummary {
    pub counts: Vec<u64>,
    pub hmin: f64,
    pub hmax: f64,
    pub bin_size: f64,
    pub count: u64,
    pub mean: f64,
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
    pub mode: f64,
}

/// Recorder.addQuotes (original_imagej/ij/plugin/frame/Recorder.java 773-780)
/// wraps option values containing spaces in square brackets.
fn macro_option_value(value: &str) -> String {
    if value.contains(' ') {
        format!("[{value}]")
    } else {
        value.to_string()
    }
}

/// Numeric dialog text as Recorder records it: GenericDialog.trim removes a
/// trailing ".0"/".00" from the field text (GenericDialog.java 1238-1242).
fn format_scale_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1.0e15 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

fn measurement_macro_options(config: MeasurementConfig) -> String {
    // Analyzer.java 28-33: checkbox order in the Set Measurements dialog.
    const CHECKBOXES: &[(u32, &str)] = &[
        (MeasurementFlags::AREA, "area"),
        (MeasurementFlags::MEAN, "mean"),
        (MeasurementFlags::STD_DEV, "standard"),
        (MeasurementFlags::MODE, "modal"),
        (MeasurementFlags::MIN_MAX, "min"),
        (MeasurementFlags::CENTROID, "centroid"),
        (MeasurementFlags::CENTER_OF_MASS, "center"),
        (MeasurementFlags::PERIMETER, "perimeter"),
        (MeasurementFlags::RECT, "bounding"),
        (MeasurementFlags::ELLIPSE, "fit"),
        (MeasurementFlags::SHAPE_DESCRIPTORS, "shape"),
        (MeasurementFlags::FERET, "feret's"),
        (MeasurementFlags::INTEGRATED_DENSITY, "integrated"),
        (MeasurementFlags::MEDIAN, "median"),
        (MeasurementFlags::SKEWNESS, "skewness"),
        (MeasurementFlags::KURTOSIS, "kurtosis"),
        (MeasurementFlags::AREA_FRACTION, "area_fraction"),
        (MeasurementFlags::STACK_POSITION, "stack"),
        (MeasurementFlags::LIMIT, "limit"),
        (MeasurementFlags::LABELS, "display"),
        (MeasurementFlags::INVERT_Y, "invert"),
        (MeasurementFlags::SCIENTIFIC_NOTATION, "scientific"),
        (MeasurementFlags::ADD_TO_OVERLAY, "add"),
        (MeasurementFlags::NAN_EMPTY_CELLS, "nan"),
    ];
    let mut options: Vec<&str> = CHECKBOXES
        .iter()
        .filter_map(|(flag, name)| config.contains(*flag).then_some(*name))
        .collect();
    options.push("redirect=None");
    let mut result = options.join(" ");
    result.push_str(&format!(" decimal={}", config.precision));
    result
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

#[derive(Clone, Copy)]
enum RasterSaveFormat {
    Tiff,
    Png,
    Jpeg,
}

impl RasterSaveFormat {
    fn macro_label(self) -> &'static str {
        match self {
            Self::Tiff => "Tiff",
            Self::Png => "PNG",
            Self::Jpeg => "Jpeg",
        }
    }
}

#[derive(Default)]
pub struct Session {
    next_id: u32,
    images: BTreeMap<ImageId, ImageDocument>,
    active_image: Option<ImageId>,
    pub results: ResultsTable,
    pub macro_recorder: Vec<String>,
    dirty_images: BTreeSet<ImageId>,
    preview_display_ranges: BTreeMap<ImageId, DisplayRange>,
    /// Last image path this document was opened from or successfully saved to.
    /// `FileSaver.save()` re-saves an image opened from disk at this path instead
    /// of showing a Save dialog (original_imagej/ij/io/FileSaver.java 42-69).
    saved_image_paths: BTreeMap<ImageId, PathBuf>,
    /// Per-image change counter. Any mutable access or session mutation bumps
    /// it, so a result prepared before the change is rejected as stale.
    image_revisions: BTreeMap<ImageId, u64>,
    /// Configured explicitly by the caller; plugin execution is never enabled
    /// implicitly by a repository checkout.
    plugin_bridge: Option<Arc<PluginBridge>>,
    pub previous_roi: Option<Roi>,
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

    /// Saves the active document's current ROI to `previous_roi` if it has width > 0 or height > 0
    /// (ImagePlus.saveRoi, ImagePlus.java lines 2245-2258).
    pub fn save_roi(&mut self) {
        if let Ok(doc) = self.active_doc() {
            if let Some(ref roi) = doc.roi {
                let (_, _, w, h) = roi.bounds();
                if w > 0 || h > 0 {
                    self.previous_roi = Some(roi.clone());
                }
            }
        }
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
            doc.snapshot_display_range = None;
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
                self.save_roi();
                let doc = ImageDocument::new_ramp_8bit(&title, width, height)?;
                self.macro_recorder.push(format!(
                    "newImage(\"{title}\", \"8-bit ramp\", {width}, {height}, 1);"
                ));
                let id = self.insert_doc(doc);
                self.dirty_images.insert(id);
                Ok(CommandOutcome::ImageOpened(id))
            }
            Command::OpenTiff { path } | Command::OpenImage { path } => {
                self.save_roi();
                let remembered_path = std::path::absolute(&path)?;
                let doc = load_image(&path)?;
                self.macro_recorder
                    .push(format!("open(\"{}\");", path.display()));
                let id = self.insert_doc(doc);
                self.dirty_images.remove(&id);
                self.saved_image_paths.insert(id, remembered_path);
                Ok(CommandOutcome::ImageOpened(id))
            }
            Command::Revert => {
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                let path = self
                    .saved_image_paths
                    .get(&id)
                    .cloned()
                    .ok_or(SessionError::NoSavedTiffPath)?;
                let saved_roi = self
                    .images
                    .get(&id)
                    .ok_or(SessionError::NoActiveImage)?
                    .roi
                    .clone();
                let mut reverted = load_image(&path)?;
                reverted.roi = saved_roi;
                self.images.insert(id, reverted);
                self.preview_display_ranges.remove(&id);
                self.dirty_images.remove(&id);
                self.bump_image_revision(id);
                self.macro_recorder.push("run(\"Revert\");".into());
                Ok(CommandOutcome::Updated)
            }
            Command::SaveTiff { path } => {
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                self.save_image_as(id, &path, RasterSaveFormat::Tiff)?;
                Ok(CommandOutcome::Saved(path))
            }
            Command::SavePng { path } => {
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                self.save_image_as(id, &path, RasterSaveFormat::Png)?;
                Ok(CommandOutcome::Saved(path))
            }
            Command::SaveJpeg { path } => {
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                self.save_image_as(id, &path, RasterSaveFormat::Jpeg)?;
                Ok(CommandOutcome::Saved(path))
            }
            Command::SetDisplayRange { min, max } => {
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                let preview_original = self.preview_display_ranges.remove(&id);
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let range = DisplayRange { min, max };
                    let changed = preview_original.unwrap_or(doc.display_range) != range;
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
            Command::PreviewDisplayRange { min, max } => {
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                let original = self.images.get(&id).ok_or(SessionError::NoActiveImage)?.display_range;
                self.preview_display_ranges.entry(id).or_insert(original);
                self.active_doc_mut()?.display_range = DisplayRange { min, max };
                Ok(CommandOutcome::Updated)
            }
            Command::ApplyDisplayLut => {
                if let Some(id) = self.active_image {
                    self.preview_display_ranges.remove(&id);
                }
                self.active_doc_mut()?.apply_display_lut()?;
                self.mark_active_dirty();
                self.macro_recorder.push("run(\"Apply LUT\");".into());
                Ok(CommandOutcome::Updated)
            }
            Command::AutoDisplayRange { min, max } => {
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                let preview_original = self.preview_display_ranges.remove(&id);
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let range = DisplayRange { min, max };
                    let changed = preview_original.unwrap_or(doc.display_range) != range;
                    doc.display_range = range;
                    changed
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder.push("run(\"Enhance Contrast\", \"saturated=0.35\");".into());
                Ok(CommandOutcome::Updated)
            }
            Command::ResetDisplayRange { min, max } => {
                let id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                let preview_original = self.preview_display_ranges.remove(&id);
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let range = DisplayRange { min, max };
                    let changed = preview_original.unwrap_or(doc.display_range) != range;
                    doc.display_range = range;
                    changed
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder.push("resetMinAndMax();".into());
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
            Command::MakeFreehandRoi { points } => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let before = doc.roi.clone();
                    doc.set_polygon_roi(&points)?;
                    doc.roi != before
                };
                if changed {
                    self.mark_active_dirty();
                }
                Ok(CommandOutcome::Updated)
            }
            Command::MakeWandRoi { x, y, points } => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let before = doc.roi.clone();
                    doc.set_polygon_roi(&points)?;
                    doc.roi != before
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder.push(format!("doWand({x}, {y});"));
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
            Command::SelectAll => {
                self.save_roi();
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let before = doc.roi.clone();
                    doc.select_all()?;
                    doc.roi != before
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder.push("run(\"Select All\");".into());
                Ok(CommandOutcome::Updated)
            }
            Command::ClearRoi => {
                self.save_roi();
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
            Command::RestoreSelection => {
                let changed = if let Some(prev) = self.previous_roi.clone() {
                    let doc = self.active_doc_mut()?;
                    let before = doc.roi.clone();
                    let restored = doc.restore_roi(&prev);
                    restored && doc.roi != before
                } else {
                    false
                };
                if changed {
                    self.mark_active_dirty();
                }
                self.macro_recorder.push("run(\"Restore Selection\");".into());
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
            Command::NavigatePosition {
                channel,
                slice,
                frame,
            } => {
                let doc = self.active_doc_mut()?;
                // ImagePlus.setPosition is the interactive hyperstack display path;
                // discards the processor snapshot but records no macro.
                doc.set_position(channel, slice, frame);
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
            Command::Crop => {
                self.active_doc_mut()?.crop()?;
                self.mark_active_dirty();
                self.macro_recorder.push("run(\"Crop\");".into());
                Ok(CommandOutcome::Updated)
            }
            Command::Duplicate {
                title,
                duplicate_stack,
                first_slice,
                last_slice,
            } => {
                let source_id = self.active_image.ok_or(SessionError::NoActiveImage)?;
                let (new_doc, options) = {
                    let source = self
                        .images
                        .get(&source_id)
                        .ok_or(SessionError::NoActiveImage)?;
                    let existing_titles: Vec<&str> =
                        self.images.values().map(|doc| doc.title.as_str()).collect();
                    let resolved = title
                        .unwrap_or_else(|| default_duplicate_title(&existing_titles, &source.title));
                    let stack_size = source.stack_size() as u32;
                    // Duplicator.showDialog defaults an empty Range field to
                    // 1..stackSize and clamps typed values (Duplicator.java
                    // 530-558).
                    let range = if duplicate_stack {
                        let mut first = first_slice.unwrap_or(1).max(1);
                        let mut last = last_slice.unwrap_or(stack_size).min(stack_size);
                        if first > last {
                            first = 1;
                            last = stack_size;
                        }
                        Some((first, last))
                    } else {
                        None
                    };
                    let new_doc = source.duplicate(resolved.clone(), duplicate_stack, range)?;
                    // GenericDialog records Title, then the "Duplicate stack"
                    // checkbox key ("duplicate"), then Range (Recorder.java
                    // 472-480, trimKey at 545-553). A full-stack range equals
                    // the unchanged Range default, so smart recording omits it.
                    let mut options = format!("title={}", macro_option_value(&resolved));
                    if duplicate_stack && stack_size > 1 {
                        options.push_str(" duplicate");
                        if let Some((first, last)) = range {
                            if first != 1 || last != stack_size {
                                options.push_str(&format!(" range={first}-{last}"));
                            }
                        }
                    }
                    (new_doc, options)
                };
                let new_id = self.insert_doc(new_doc);
                self.dirty_images.insert(new_id);
                self.macro_recorder
                    .push(format!("run(\"Duplicate...\", \"{options}\");"));
                Ok(CommandOutcome::ImageOpened(new_id))
            }
            Command::ConvertTo8Bit => {
                self.active_doc_mut()?.convert_to_8bit();
                self.mark_active_dirty();
                self.macro_recorder.push("run(\"8-bit\");".into());
                Ok(CommandOutcome::Updated)
            }
            Command::ConvertTo16Bit => {
                self.active_doc_mut()?.convert_to_16bit()?;
                self.mark_active_dirty();
                self.macro_recorder.push("run(\"16-bit\");".into());
                Ok(CommandOutcome::Updated)
            }
            Command::ConvertTo32Bit => {
                self.active_doc_mut()?.convert_to_32bit()?;
                self.mark_active_dirty();
                self.macro_recorder.push("run(\"32-bit\");".into());
                Ok(CommandOutcome::Updated)
            }
            Command::ConvertToRgb => {
                self.active_doc_mut()?.convert_to_rgb();
                self.mark_active_dirty();
                self.macro_recorder.push("run(\"RGB Color\");".into());
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
                // RankFilters.setup calls IJ.setupDialog before showDialog
                // records the Radius field (RankFilters.java 160, 116-145), so
                // the stack answer comes first: "slice radius=2".
                let prefix = if is_stack { "slice " } else { "" };
                self.macro_recorder
                    .push(format!("run(\"Median...\", \"{prefix}radius={radius}\");"));
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::Minimum { radius } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.rank_filter(ij_core::RankFilterKind::Minimum, radius)?;
                let prefix = if is_stack { "slice " } else { "" };
                self.macro_recorder
                    .push(format!("run(\"Minimum...\", \"{prefix}radius={radius}\");"));
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::Maximum { radius } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.rank_filter(ij_core::RankFilterKind::Maximum, radius)?;
                let prefix = if is_stack { "slice " } else { "" };
                self.macro_recorder
                    .push(format!("run(\"Maximum...\", \"{prefix}radius={radius}\");"));
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
            Command::PreviewThreshold { lower, upper } => {
                self.active_doc_mut()?.threshold = Some(ThresholdLevels { lower, upper });
                Ok(CommandOutcome::Thresholded(ThresholdLevels {
                    lower,
                    upper,
                }))
            }
            Command::SetThreshold { lower, upper } => {
                let upper = upper.max(lower);
                let levels = ThresholdLevels { lower, upper };
                self.active_doc_mut()?.threshold = Some(levels);
                self.macro_recorder.push(format!(
                    "setThreshold({}, {}, \"raw\");",
                    ij_core::format_ij_num(lower),
                    ij_core::format_ij_num(upper)
                ));
                Ok(CommandOutcome::Thresholded(levels))
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
            Command::SmoothStack => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.smooth_stack()?;
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Smooth\", \"stack\");".into());
                } else {
                    self.macro_recorder.push("run(\"Smooth\");".into());
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::SharpenStack => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.sharpen_stack()?;
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Sharpen\", \"stack\");".into());
                } else {
                    self.macro_recorder.push("run(\"Sharpen\");".into());
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::FindEdgesStack => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.find_edges_stack()?;
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Find Edges\", \"stack\");".into());
                } else {
                    self.macro_recorder.push("run(\"Find Edges\");".into());
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::GaussianBlurStack { sigma } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.gaussian_blur_stack(sigma)?;
                // GaussianBlur.showDialog records sigma before IJ.setupDialog
                // records the stack answer (GaussianBlur.java 96-104).
                let sigma = format!("{sigma}");
                if is_stack {
                    self.macro_recorder.push(format!(
                        "run(\"Gaussian Blur...\", \"sigma={sigma} stack\");"
                    ));
                } else {
                    self.macro_recorder
                        .push(format!("run(\"Gaussian Blur...\", \"sigma={sigma}\");"));
                }
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::MedianStack { radius } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.median_stack(radius)?;
                let prefix = if is_stack { "stack " } else { "" };
                self.macro_recorder.push(format!(
                    "run(\"Median...\", \"{prefix}radius={radius}\");"
                ));
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::MinimumStack { radius } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.minimum_stack(radius)?;
                let prefix = if is_stack { "stack " } else { "" };
                self.macro_recorder.push(format!(
                    "run(\"Minimum...\", \"{prefix}radius={radius}\");"
                ));
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::MaximumStack { radius } => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.maximum_stack(radius)?;
                let prefix = if is_stack { "stack " } else { "" };
                self.macro_recorder.push(format!(
                    "run(\"Maximum...\", \"{prefix}radius={radius}\");"
                ));
                self.mark_active_dirty();
                Ok(CommandOutcome::Updated)
            }
            Command::DespeckleStack => {
                let doc = self.active_doc_mut()?;
                let is_stack = doc.stack_size() > 1;
                doc.despeckle_stack()?;
                if is_stack {
                    self.macro_recorder
                        .push("run(\"Despeckle\", \"stack\");".into());
                } else {
                    self.macro_recorder.push("run(\"Despeckle\");".into());
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
            Command::SetMeasurements { flags, precision } => {
                let config = MeasurementConfig {
                    flags,
                    precision: precision.min(9),
                };
                self.results.config = config;
                self.macro_recorder.push(format!(
                    "run(\"Set Measurements...\", \"{}\");",
                    measurement_macro_options(config)
                ));
                Ok(CommandOutcome::Updated)
            }
            Command::SetScale {
                distance,
                known,
                aspect,
                unit,
            } => {
                let changed = {
                    let doc = self.active_doc_mut()?;
                    let previous = doc.calibration.clone();
                    doc.calibration.apply_scale(distance, known, aspect, &unit);
                    doc.calibration != previous
                };
                // ScaleDialog.java 109-112 sets imp.changes only when the
                // calibration actually differs from its previous value.
                if changed {
                    self.mark_active_dirty();
                }
                // Recorder.recordOption lowercases the first label word and
                // GenericDialog.trim drops a trailing ".0" (Recorder.java
                // 472-481, GenericDialog.java 1238-1242). The aspect field is
                // always recorded here so the macro does not depend on the
                // dialog's smart-recording state.
                let recorded_unit = if unit.is_empty() {
                    "[]".to_string()
                } else {
                    unit
                };
                self.macro_recorder.push(format!(
                    "run(\"Set Scale...\", \"distance={} known={} pixel={} unit={}\");",
                    format_scale_number(distance),
                    format_scale_number(known),
                    format_scale_number(aspect),
                    recorded_unit
                ));
                Ok(CommandOutcome::Updated)
            }
            Command::RemoveScale => {
                self.active_doc_mut()?.calibration.remove_scale();
                self.macro_recorder.push("Image.removeScale;".into());
                Ok(CommandOutcome::Updated)
            }
            Command::Measure => {
                let stats = self.active_doc()?.measure_with_config(self.results.config)?;
                self.results.push(stats.clone());
                self.macro_recorder.push("run(\"Measure\");".into());
                Ok(CommandOutcome::Measured(stats))
            }
            Command::Histogram => {
                let mut doc = self.active_doc()?.clone();
                // ImagePlus.getStatistics resets non-area ROI before the
                // Histogram plugin reads it (ImagePlus.java 1141-1145).
                if doc.roi.as_ref().is_some_and(|roi| !roi.is_area()) {
                    doc.roi = None;
                }
                let config = MeasurementConfig {
                    flags: MeasurementFlags::AREA
                        | MeasurementFlags::MEAN
                        | MeasurementFlags::MODE
                        | MeasurementFlags::MIN_MAX
                        | (self.results.config.flags & MeasurementFlags::LIMIT),
                    precision: self.results.config.precision,
                };
                let stats = doc.measure_with_config(config)?;
                let mut display = display_histogram(&doc);
                if config.contains(MeasurementFlags::LIMIT) {
                    if let Some(levels) = doc.threshold {
                        match doc.plane() {
                            PixelPlane::Gray8(_) | PixelPlane::Color256 { .. } => {
                                let lower = (levels.lower as i32).clamp(0, 255) as usize;
                                let upper = (levels.upper as i32).clamp(0, 255) as usize;
                                for (index, count) in display.counts.iter_mut().enumerate() {
                                    if index < lower || index > upper {
                                        *count = 0;
                                    }
                                }
                            }
                            PixelPlane::Gray16(_) => {
                                display.hmin = stats.min;
                                display.hmax = stats.max;
                                display.counts = [0; 256];
                                let width = (stats.max - stats.min + 1.0) / 256.0;
                                if width > 0.0 {
                                    for (value, &count) in stats.histogram.iter().enumerate() {
                                        if count > 0 && (value as f64) >= stats.min && (value as f64) <= stats.max {
                                            let bin = (((value as f64 - stats.min) / width) as usize).min(255);
                                            display.counts[bin] += count;
                                        }
                                    }
                                }
                            }
                            PixelPlane::Gray32(_) => {
                                display.hmin = stats.min;
                                display.hmax = stats.max;
                                display.counts.copy_from_slice(&stats.histogram);
                            }
                            PixelPlane::ColorRgb(_) => {}
                        }
                    }
                }
                let summary = HistogramSummary {
                    counts: display.counts.to_vec(),
                    hmin: display.hmin,
                    hmax: display.hmax,
                    bin_size: match doc.plane() {
                        PixelPlane::Gray16(_) => (display.hmax - display.hmin + 1.0) / 256.0,
                        PixelPlane::Gray32(_) => (display.hmax - display.hmin) / 256.0,
                        _ => 1.0,
                    },
                    count: stats.pixel_count,
                    mean: stats.mean,
                    std_dev: stats.std_dev,
                    min: stats.min,
                    max: stats.max,
                    mode: stats.mode,
                };
                self.macro_recorder.push("run(\"Histogram\");".into());
                Ok(CommandOutcome::HistogramComputed(summary))
            }
            Command::PlotProfile => {
                // ProfilePlot only reads the processor; the image, ROI and
                // dirty state are unchanged.
                let profile = self.active_doc()?.plot_profile()?;
                self.macro_recorder
                    .push("run(\"Plot Profile\");".into());
                Ok(CommandOutcome::ProfileComputed(profile))
            }
            Command::ClearResults => {
                self.results.clear();
                self.macro_recorder.push("run(\"Clear Results\");".into());
                Ok(CommandOutcome::ResultsCleared)
            }
            Command::SaveResultsTsv { path } => {
                let tsv = self.results.to_tsv();
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
        let extension = path
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase());
        let format = match detect_image_format(path) {
            Ok(ImageFileFormat::Png) => RasterSaveFormat::Png,
            Ok(ImageFileFormat::Jpeg) => RasterSaveFormat::Jpeg,
            Ok(ImageFileFormat::Tiff) => RasterSaveFormat::Tiff,
            Err(_) => match extension.as_deref() {
                Some("png") => RasterSaveFormat::Png,
                Some("jpg" | "jpeg") => RasterSaveFormat::Jpeg,
                _ => RasterSaveFormat::Tiff,
            },
        };
        self.save_image_as(id, path, format)
    }

    fn save_image_as(
        &mut self,
        id: ImageId,
        path: &std::path::Path,
        format: RasterSaveFormat,
    ) -> Result<(), SessionError> {
        let remembered_path = std::path::absolute(path)?;
        let doc = self.images.get(&id).ok_or(SessionError::NoActiveImage)?;
        match format {
            RasterSaveFormat::Tiff => save_tiff(path, doc)?,
            RasterSaveFormat::Png => save_png(path, doc)?,
            RasterSaveFormat::Jpeg => save_jpeg(path, doc, 75)?,
        }
        self.dirty_images.remove(&id);
        self.saved_image_paths.insert(id, remembered_path);
        self.macro_recorder.push(format!(
            "saveAs(\"{}\", \"{}\");",
            format.macro_label(),
            path.display()
        ));
        Ok(())
    }

    /// The destination a close prompt should re-save to, or `None` for a
    /// document that has never been opened from or saved to a file.
    pub fn image_path(&self, id: ImageId) -> Option<&std::path::Path> {
        self.saved_image_paths.get(&id).map(PathBuf::as_path)
    }

    fn mark_active_dirty(&mut self) {
        if let Some(id) = self.active_image {
            self.dirty_images.insert(id);
        }
    }

    pub fn close_image(&mut self, id: ImageId) -> bool {
        self.preview_display_ranges.remove(&id);
        self.dirty_images.remove(&id);
        self.saved_image_paths.remove(&id);
        self.image_revisions.remove(&id);
        let removed = self.images.remove(&id).is_some();
        if self.active_image == Some(id) {
            self.active_image = self.images.keys().last().copied();
        }
        removed
    }

    pub fn close_all_images(&mut self) {
        self.preview_display_ranges.clear();
        self.dirty_images.clear();
        self.saved_image_paths.clear();
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
