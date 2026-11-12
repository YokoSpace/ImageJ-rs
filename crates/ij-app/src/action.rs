use crate::app::ImageJApp;
use ij_core::PixelPlane;
use ij_session::{Command, CommandOutcome, ImageId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    NewRamp,
    OpenTiff,
    /// `File > Save` (`Cmd+S`). `FileSaver.save()` re-saves a document that
    /// already has a path and otherwise opens the Save As dialog
    /// (original_imagej/ij/plugin/Commands.java 58-69).
    Save,
    SaveAs,
    SaveAsPng,
    SaveAsJpeg,
    SaveResults,
    Close,
    CloseImage(ImageId),
    CloseAll,
    Quit,
    Revert,
    Undo,
    Invert,
    Duplicate,
    Crop,
    ConvertTo8Bit,
    ConvertTo16Bit,
    ConvertTo32Bit,
    ConvertToRgb,
    SelectAll,
    SelectNone,
    RestoreSelection,
    AddToRoiManager,
    RoiManager,
    BrightnessContrast,
    Threshold,
    InvertStack,
    Smooth,
    Sharpen,
    FindEdges,
    GaussianBlur,
    Median,
    Minimum,
    Maximum,
    Despeckle,
    MathAdd,
    MathSubtract,
    MathMultiply,
    MathDivide,
    MathMin,
    MathMax,
    MathGamma,
    MathLog,
    MathExp,
    MathSquare,
    MathSquareRoot,
    MathReciprocal,
    MathAbs,
    MathSet,
    /// `Process > Binary > Make Binary` (`ij.plugin.Thresholder`).
    MakeBinary,
    /// `Process > Binary > Convert to Mask` and the Threshold palette's
    /// `Apply` button (ThresholdAdjuster.runThresholdCommand).
    ConvertToMask,
    /// `Process > Binary > Erode` through `Close` (`ij.plugin.filter.Binary`,
    /// IJ_Props.txt binary04-binary07; the Close item is labelled `Close-`).
    BinaryErode,
    BinaryDilate,
    BinaryOpen,
    BinaryClose,
    ImageCalculator,
    Measure,
    Histogram,
    PlotProfile,
    SetMeasurements,
    AnalyzeParticles,
    SetScale,
    ClearResults,
    InstalledPlugins,
    RunPluginFilter,
    RunCatalogPlugin { index: usize, generation: u64 },
    ToggleResults,
    ActivateImage(ImageId),
    About,
    ZoomIn(ImageId),
    ZoomOut(ImageId),
    NextSlice(ImageId),
    PreviousSlice(ImageId),
    ToggleAnimation(ImageId),
    /// Edit > Clear: fills the selection with the background color
    /// (Filler.java "clear").
    Clear,
    /// Edit > Clear Outside: fills everything outside the selection with
    /// the background color (Filler.java "outside").
    ClearOutside,
    /// Edit > Fill (`F`): fills the selection with the foreground color
    /// (Filler.java "fill").
    Fill,
    Copy,
    Cut,
    Paste,
    /// Edit > Draw (`D`): draws the selection outline with the foreground
    /// color (Filler.java "draw").
    Draw,
}

#[derive(Debug, Clone, Copy)]
pub enum MenuKind {
    Item(AppAction),
    Submenu(&'static [MenuNode]),
    Separator,
    Unsupported,
    WindowList,
}

#[derive(Debug, Clone, Copy)]
pub struct MenuNode {
    pub label: &'static str,
    pub shortcut: Option<&'static str>,
    pub kind: MenuKind,
}

const fn item(label: &'static str, shortcut: Option<&'static str>, action: AppAction) -> MenuNode {
    MenuNode {
        label,
        shortcut,
        kind: MenuKind::Item(action),
    }
}

const fn submenu(label: &'static str, nodes: &'static [MenuNode]) -> MenuNode {
    MenuNode {
        label,
        shortcut: None,
        kind: MenuKind::Submenu(nodes),
    }
}

const fn unsupported(label: &'static str, shortcut: Option<&'static str>) -> MenuNode {
    MenuNode {
        label,
        shortcut,
        kind: MenuKind::Unsupported,
    }
}

const SEPARATOR: MenuNode = MenuNode {
    label: "",
    shortcut: None,
    kind: MenuKind::Separator,
};

const FILE_NEW: &[MenuNode] = &[item("Image... (8-bit Ramp)", None, AppAction::NewRamp)];
// IJ_Props.txt File>Save As entries (original_imagej/IJ_Props.txt 49-66).
const FILE_SAVE_AS: &[MenuNode] = &[
    item("Tiff...", None, AppAction::SaveAs),
    item("Jpeg...", None, AppAction::SaveAsJpeg),
    item("PNG...", None, AppAction::SaveAsPng),
    item("Results...", None, AppAction::SaveResults),
];
const FILE_MENU: &[MenuNode] = &[
    submenu("New", FILE_NEW),
    item("Open...", None, AppAction::OpenTiff),
    SEPARATOR,
    item("Close", Some("W"), AppAction::Close),
    item("Close All", Some("Shift+W"), AppAction::CloseAll),
    item("Save", Some("Cmd+S"), AppAction::Save),
    submenu("Save As", FILE_SAVE_AS),
    SEPARATOR,
    item("Revert", None, AppAction::Revert),
    unsupported("Page Setup...", None),
    unsupported("Print...", None),
    SEPARATOR,
    item("Quit", Some("Cmd+Q"), AppAction::Quit),
];

const EDIT_SELECTION: &[MenuNode] = &[
    item("Select All", Some("Cmd+A"), AppAction::SelectAll),
    item("Select None", Some("Shift+A"), AppAction::SelectNone),
    item("Restore Selection", Some("Shift+E"), AppAction::RestoreSelection),
    item("Add to Manager", Some("T"), AppAction::AddToRoiManager),
];
const EDIT_MENU: &[MenuNode] = &[
    item("Undo", Some("Z"), AppAction::Undo),
    SEPARATOR,
    item("Cut", Some("X"), AppAction::Cut),
    item("Copy", Some("C"), AppAction::Copy),
    item("Paste", Some("V"), AppAction::Paste),
    item("Clear", None, AppAction::Clear),
    item("Clear Outside", None, AppAction::ClearOutside),
    item("Fill", Some("F"), AppAction::Fill),
    item("Draw", Some("D"), AppAction::Draw),
    SEPARATOR,
    item("Invert", Some("Shift+I"), AppAction::Invert),
    SEPARATOR,
    submenu("Selection", EDIT_SELECTION),
];

const IMAGE_ADJUST: &[MenuNode] = &[
    item(
        "Brightness/Contrast...",
        None,
        AppAction::BrightnessContrast,
    ),
    item("Threshold...", None, AppAction::Threshold),
];
const IMAGE_STACKS: &[MenuNode] = &[item("Invert Stack", None, AppAction::InvertStack)];
const IMAGE_TYPE: &[MenuNode] = &[
    item("8-bit", None, AppAction::ConvertTo8Bit),
    item("16-bit", None, AppAction::ConvertTo16Bit),
    item("32-bit", None, AppAction::ConvertTo32Bit),
    item("RGB Color", None, AppAction::ConvertToRgb),
];
const IMAGE_MENU: &[MenuNode] = &[
    submenu("Adjust", IMAGE_ADJUST),
    submenu("Stacks", IMAGE_STACKS),
    submenu("Type", IMAGE_TYPE),
    item("Duplicate...", Some("Shift+D"), AppAction::Duplicate),
    item("Crop", Some("Shift+X"), AppAction::Crop),
];

const PROCESS_FILTERS: &[MenuNode] = &[
    item("Gaussian Blur...", None, AppAction::GaussianBlur),
    item("Median (radius 2)", None, AppAction::Median),
    item("Minimum (radius 2)", None, AppAction::Minimum),
    item("Maximum (radius 2)", None, AppAction::Maximum),
];
const PROCESS_NOISE: &[MenuNode] = &[item("Despeckle", None, AppAction::Despeckle)];
const PROCESS_MATH: &[MenuNode] = &[
    item("Add...", None, AppAction::MathAdd),
    item("Subtract...", None, AppAction::MathSubtract),
    item("Multiply...", None, AppAction::MathMultiply),
    item("Divide...", None, AppAction::MathDivide),
    item("Min...", None, AppAction::MathMin),
    item("Max...", None, AppAction::MathMax),
    item("Gamma...", None, AppAction::MathGamma),
    item("Log", None, AppAction::MathLog),
    item("Exp", None, AppAction::MathExp),
    item("Square", None, AppAction::MathSquare),
    item("Square Root", None, AppAction::MathSquareRoot),
    item("Reciprocal", None, AppAction::MathReciprocal),
    item("Abs", None, AppAction::MathAbs),
    item("Set...", None, AppAction::MathSet),
];
// IJ_Props.txt binary01-binary07 (Process > Binary; the morphology items'
// labels match the original submenu, including the `Close-` label).
const PROCESS_BINARY: &[MenuNode] = &[
    item("Make Binary", None, AppAction::MakeBinary),
    item("Convert to Mask", None, AppAction::ConvertToMask),
    SEPARATOR,
    item("Erode", None, AppAction::BinaryErode),
    item("Dilate", None, AppAction::BinaryDilate),
    item("Open", None, AppAction::BinaryOpen),
    item("Close-", None, AppAction::BinaryClose),
];
const PROCESS_MENU: &[MenuNode] = &[
    item("Smooth", None, AppAction::Smooth),
    item("Sharpen", None, AppAction::Sharpen),
    item("Find Edges", None, AppAction::FindEdges),
    SEPARATOR,
    submenu("Filters", PROCESS_FILTERS),
    submenu("Noise", PROCESS_NOISE),
    submenu("Binary", PROCESS_BINARY),
    submenu("Math", PROCESS_MATH),
    unsupported("FFT", None),
    // Menus.java 203-207: after the FFT submenu, Image Calculator follows
    // the separator (the unwired Batch submenu sits in between).
    SEPARATOR,
    item("Image Calculator...", None, AppAction::ImageCalculator),
];

const ANALYZE_TOOLS: &[MenuNode] = &[
    item("ROI Manager...", None, AppAction::RoiManager),
];
const ANALYZE_MENU: &[MenuNode] = &[
    item("Measure", Some("M"), AppAction::Measure),
    item("Histogram", Some("H"), AppAction::Histogram),
    item("Clear Results", None, AppAction::ClearResults),
    item("Analyze Particles...", None, AppAction::AnalyzeParticles),
    item("Set Measurements...", None, AppAction::SetMeasurements),
    item("Set Scale...", None, AppAction::SetScale),
    item("Plot Profile", Some("K"), AppAction::PlotProfile),
    submenu("Tools", ANALYZE_TOOLS),
    unsupported("Calibrate...", None),
];

const PLUGINS_MENU: &[MenuNode] = &[
    item("Installed Plugins...", None, AppAction::InstalledPlugins),
    SEPARATOR,
    item(
        "Run PlugInFilter (Subset)...",
        None,
        AppAction::RunPluginFilter,
    ),
    SEPARATOR,
    unsupported("Macros >", None),
    unsupported("Compile and Run...", None),
];

const WINDOW_MENU: &[MenuNode] = &[
    item("Results", None, AppAction::ToggleResults),
    SEPARATOR,
    MenuNode {
        label: "",
        shortcut: None,
        kind: MenuKind::WindowList,
    },
];
const HELP_MENU: &[MenuNode] = &[item("About ImageJ...", None, AppAction::About)];

pub static MENU_TREE: &[MenuNode] = &[
    submenu("File", FILE_MENU),
    submenu("Edit", EDIT_MENU),
    submenu("Image", IMAGE_MENU),
    submenu("Process", PROCESS_MENU),
    submenu("Analyze", ANALYZE_MENU),
    submenu("Plugins", PLUGINS_MENU),
    submenu("Window", WINDOW_MENU),
    submenu("Help", HELP_MENU),
];

/// OS-independent description of a native menu, derived from [`MENU_TREE`].
/// `Command` is implicit in every shortcut because only `Cmd+` chords are
/// promoted to OS key equivalents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeShortcut {
    pub key: String,
    pub shift: bool,
}

/// One entry of a native menu bar, independent of AppKit or any other OS API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeMenuSpec {
    pub label: String,
    pub shortcut: Option<NativeShortcut>,
    pub action: Option<AppAction>,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub checked: bool,
    pub is_separator: bool,
    pub children: Vec<NativeMenuSpec>,
}

/// Only explicit `Cmd+` chords in the menu table become OS key equivalents.
/// Bare keys such as `Z`, `W`, `M` and `Shift+X` stay with `shortcuts.rs`, so
/// typing in a text field can never fire a native menu command (N1 spec).
fn native_shortcut(shortcut: Option<&str>) -> Option<NativeShortcut> {
    let chord = shortcut?.strip_prefix("Cmd+")?;
    let (key, shift) = match chord.strip_prefix("Shift+") {
        Some(key) => (key, true),
        None => (chord, false),
    };
    if key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(NativeShortcut {
            key: key.to_ascii_uppercase(),
            shift,
        })
    } else {
        None
    }
}

impl ImageJApp {
    /// Converts the shared menu table into an OS-independent native menu tree.
    /// `WindowList` expands in place to one selectable entry per open image.
    pub fn build_native_menu_spec(&self) -> Vec<NativeMenuSpec> {
        let mut spec = self.build_native_menu_nodes(MENU_TREE);
        let entries = self.plugin_menu_entries();
        if !entries.is_empty() {
            if let Some(menu) = spec.iter_mut().find(|menu| menu.label == "Plugins") {
                menu.children.push(NativeMenuSpec {
                    label: String::new(), shortcut: None, action: None,
                    enabled: false, disabled_reason: None, checked: false,
                    is_separator: true, children: Vec::new(),
                });
                for entry in entries {
                    menu.children.push(NativeMenuSpec {
                        label: entry.label, shortcut: None,
                        action: Some(AppAction::RunCatalogPlugin {
                            index: entry.index, generation: self.plugin_menu_generation(),
                        }),
                        enabled: entry.enabled, disabled_reason: entry.reason,
                        checked: false, is_separator: false, children: Vec::new(),
                    });
                }
            }
        }
        spec
    }

    fn build_native_menu_nodes(&self, nodes: &[MenuNode]) -> Vec<NativeMenuSpec> {
        let mut spec = Vec::new();
        for node in nodes {
            self.push_native_menu_node(&mut spec, node);
        }
        spec
    }

    fn push_native_menu_node(&self, out: &mut Vec<NativeMenuSpec>, node: &MenuNode) {
        match node.kind {
            MenuKind::Item(action) => out.push(NativeMenuSpec {
                label: node.label.to_string(),
                shortcut: native_shortcut(node.shortcut),
                action: Some(action),
                enabled: self.action_enabled(action).is_ok(),
                disabled_reason: self.action_enabled(action).err(),
                checked: false,
                is_separator: false,
                children: Vec::new(),
            }),
            MenuKind::Submenu(children) => out.push(NativeMenuSpec {
                label: node.label.to_string(),
                shortcut: None,
                action: None,
                enabled: true,
                disabled_reason: None,
                checked: false,
                is_separator: false,
                children: self.build_native_menu_nodes(children),
            }),
            MenuKind::Separator => out.push(NativeMenuSpec {
                label: String::new(),
                shortcut: None,
                action: None,
                enabled: false,
                disabled_reason: None,
                checked: false,
                is_separator: true,
                children: Vec::new(),
            }),
            MenuKind::Unsupported => out.push(NativeMenuSpec {
                label: node.label.to_string(),
                shortcut: None,
                action: None,
                enabled: false,
                disabled_reason: Some("Not implemented in ImageJ-rs yet".into()),
                checked: false,
                is_separator: false,
                children: Vec::new(),
            }),
            MenuKind::WindowList => {
                for id in self.session.image_ids() {
                    let title = self
                        .session
                        .get_image(id)
                        .map(|doc| doc.title.clone())
                        .unwrap_or_else(|| format!("Image {}", id.0));
                    let action = AppAction::ActivateImage(id);
                    out.push(NativeMenuSpec {
                        label: title,
                        shortcut: None,
                        action: Some(action),
                        enabled: self.action_enabled(action).is_ok(),
                        disabled_reason: self.action_enabled(action).err(),
                        checked: self.active_image_id() == Some(id),
                        is_separator: false,
                        children: Vec::new(),
                    });
                }
            }
        }
    }
}

impl ImageJApp {
    pub fn action_enabled(&self, action: AppAction) -> Result<(), String> {
        if self.pending_revert.is_some() {
            return Err("Resolve revert confirmation first".into());
        }
        use AppAction::*;
        match action {
            RunCatalogPlugin { index, generation } => {
                if generation != self.plugin_menu_generation() {
                    Err("Plugin catalog changed; select the current menu entry".into())
                } else {
                    self.plugin_prefill_block_reason(index).map_or(Ok(()), Err)
                }
            }
            // Set Measurements is a user-option dialog, not an image command:
            // ImageJ's Analyze menu enables it with no image open.
            NewRamp | OpenTiff | Quit | InstalledPlugins | ToggleResults | About
            | SetMeasurements | RoiManager => Ok(()),
            SaveResults | ClearResults => {
                if self.session.results.rows.is_empty() {
                    Err("No results to save or clear".into())
                } else {
                    Ok(())
                }
            }
            Revert => {
                let Some(id) = self.active_image_id() else {
                    return Err("Requires an active image".into());
                };
                if self.session.image_path(id).is_some() {
                    Ok(())
                } else {
                    Err("Image has not been saved to a file".into())
                }
            }
            CloseAll => {
                if self.open_window_count() > 0 {
                    Ok(())
                } else {
                    Err("No image windows are open".into())
                }
            }
            CloseImage(id) | ActivateImage(id) | ZoomIn(id) | ZoomOut(id) => {
                if self.session.get_image(id).is_some() {
                    Ok(())
                } else {
                    Err("Image is no longer open".into())
                }
            }
            NextSlice(id) | PreviousSlice(id) | ToggleAnimation(id) => {
                let Some(doc) = self.session.get_image(id) else {
                    return Err("Image is no longer open".into());
                };
                if doc.stack_size() > 1 {
                    Ok(())
                } else {
                    Err("Image has one slice".into())
                }
            }
            _ => {
                let Some(id) = self.active_image_id() else {
                    return Err("Requires an active image".into());
                };
                let Some(doc) = self.session.get_image(id) else {
                    return Err("Requires an active image".into());
                };
                match action {
                    BrightnessContrast if matches!(doc.plane(), PixelPlane::ColorRgb(_)) => Err("B&C is not supported on RGB images".into()),
                    Crop => match doc.roi.as_ref() {
                        Some(roi) if roi.is_area() => {
                            let (x, y, w, h) = roi
                                .clipped_bounds(doc.width, doc.height)
                                .map_err(|_| "Selection is outside image".to_string())?;
                            let (bx, by, bw, bh) = roi.bounds();
                            let clipped = (i64::from(bx), i64::from(by), i64::from(bw), i64::from(bh))
                                != (i64::from(x), i64::from(y), i64::from(w), i64::from(h));
                            if !matches!(roi, ij_core::Roi::Rect(_)) && clipped {
                                Err("Crop of clipped non-rectangular ROI requires ShapeRoi".into())
                            } else {
                                Ok(())
                            }
                        }
                        _ => Err("Area selection required".into()),
                    },
                    ClearOutside => match doc.roi.as_ref() {
                        None => Err("This command requires a selection".into()),
                        Some(roi) if roi.is_line() => {
                            Err("\"Clear Outside\" does not work with line selections.".into())
                        }
                        _ => Ok(()),
                    },
                    Threshold if !matches!(doc.plane(), PixelPlane::Gray8(_) | PixelPlane::Gray16(_) | PixelPlane::Gray32(_)) => Err("Threshold requires a grayscale image".into()),
                    ConvertTo16Bit => match doc.plane() {
                        // ImageConverter.java 59-62 rejects COLOR_256.
                        PixelPlane::Color256 { .. } => Err("16-bit conversion is not supported for 8-bit color images".into()),
                        // StackConverter.java 145-148 rejects non-GRAY8/GRAY32 stacks.
                        PixelPlane::ColorRgb(_) if doc.stack_size() > 1 => Err("16-bit conversion is not supported for RGB stacks".into()),
                        _ => Ok(()),
                    },
                    ConvertTo32Bit => match doc.plane() {
                        // ImageConverter.java 174-175 and StackConverter.java 184-185 reject COLOR_256.
                        PixelPlane::Color256 { .. } => Err("32-bit conversion is not supported for 8-bit color images".into()),
                        _ => Ok(()),
                    },
                    Median if !matches!(doc.plane(), PixelPlane::Gray8(_) | PixelPlane::Gray16(_) | PixelPlane::Gray32(_)) => Err("Median requires a grayscale image".into()),
                    // ProfilePlot.java 53-62 accepts lines (including
                    // segmented/freehand lines) and rectangular selections.
                    PlotProfile => match doc.roi.as_ref() {
                        None => Err("Selection required.".into()),
                        Some(ij_core::Roi::Rect(_) | ij_core::Roi::Line(_) | ij_core::Roi::Polyline(_)) => Ok(()),
                        Some(_) => Err("Line or rectangular selection required.".into()),
                    },
                    AddToRoiManager if doc.roi.is_none() => Err("Selection required".into()),
                    // Thresholder.applyThreshold rejects RGB/indexed images;
                    // stacks run the "Convert Stack to Binary" dialog
                    // (Thresholder.java 54-147).
                    MakeBinary | ConvertToMask => match doc.plane() {
                        PixelPlane::Gray8(_) | PixelPlane::Gray16(_) | PixelPlane::Gray32(_) => Ok(()),
                        _ => Err("Binary conversion requires a grayscale image".into()),
                    },
                    RunPluginFilter if self.is_plugin_filter_running() => Err("A PlugInFilter is already executing".into()),
                    _ => Ok(()),
                }
            }
        }
    }

    pub fn perform(&mut self, action: AppAction, ctx: &egui::Context) {
        if self.action_enabled(action).is_err() {
            return;
        }
        use AppAction::*;
        match action {
            NewRamp => {
                let _ = self.open_ramp_8bit("ramp", 256, 256);
            }
            OpenTiff => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Image", &["tif", "tiff", "png", "jpg", "jpeg"])
                    .pick_file()
                {
                    let _ = self.open_image_file(path);
                }
            }
            Save => {
                let Some(id) = self.active_image_id() else {
                    return;
                };
                let path = match self.session.image_path(id).map(|path| path.to_path_buf()) {
                    Some(path) => Some(path),
                    None => rfd::FileDialog::new()
                        .add_filter("TIFF Image", &["tif", "tiff"])
                        .save_file(),
                };
                if let Some(path) = path {
                    let _ = self.save_active_image(path);
                }
            }
            Revert => {
                self.request_revert_active();
            }
            SaveAs => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("TIFF Image", &["tif", "tiff"])
                    .save_file()
                {
                    let _ = self.save_active_tiff(path);
                }
            }
            SaveAsPng => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG Image", &["png"])
                    .save_file()
                {
                    let _ = self.save_active_png(path);
                }
            }
            SaveAsJpeg => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("JPEG Image", &["jpg", "jpeg"])
                    .save_file()
                {
                    let _ = self.save_active_jpeg(path);
                }
            }
            SaveResults => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("TSV Table", &["tsv", "txt"])
                    .save_file()
                {
                    let _ = self.save_results_tsv(path);
                }
            }
            Close => {
                if let Some(id) = self.active_image_id() {
                    self.stop_animation(id);
                    self.request_close_window(id);
                }
            }
            CloseImage(id) => {
                self.stop_animation(id);
                self.request_close_window(id);
            }
            CloseAll => {
                self.stop_all_animations();
                self.request_close_all();
            }
            Quit => {
                self.request_quit();
            }
            Undo => {
                let _ = self.undo_active();
            }
            Clear => {
                let _ = self.filler_clear_active();
            }
            ClearOutside => {
                let _ = self.clear_outside_active();
            }
            Copy | Cut | Paste => {
                let command = match action { Copy => ij_session::Command::Copy, Cut => ij_session::Command::Cut, _ => ij_session::Command::Paste };
                match self.session.dispatch(command) {
                    Ok(_) => { self.status_message = format!("{action:?}"); },
                    Err(error) => { self.status_message = error.to_string(); }
                }
            }
            Fill => {
                let _ = self.fill_active();
            }
            Draw => {
                let _ = self.draw_active();
            }
            Invert => {
                let _ = self.invert_active();
            }
            Duplicate => {
                self.open_duplicate_dialog();
            }
            Crop => {
                let _ = self.crop_active();
            }
            ConvertTo8Bit => {
                let _ = self.convert_to_8bit_active();
            }
            ConvertTo16Bit => {
                let _ = self.convert_to_16bit_active();
            }
            ConvertTo32Bit => {
                let _ = self.convert_to_32bit_active();
            }
            ConvertToRgb => {
                let _ = self.convert_to_rgb_active();
            }
            SelectAll => {
                let _ = self.select_all();
            }
            SelectNone => {
                let _ = self.clear_roi();
            }
            RestoreSelection => {
                let _ = self.restore_selection();
            }
            AddToRoiManager => {
                if self.session.dispatch(Command::RoiManagerAdd).is_ok() {
                    self.show_roi_manager_window = true;
                }
            }
            RoiManager => {
                self.show_roi_manager_window = true;
            }
            BrightnessContrast => {
                if let Ok(doc) = self.session.active_doc() {
                    self.dialogs.bc_min = doc.display_range.min;
                    self.dialogs.bc_max = doc.display_range.max;
                }
                self.dialogs.show_brightness_contrast = true;
            }
            Threshold => {
                self.dialogs.show_threshold = true;
            }
            InvertStack => {
                let _ = self.invert_stack_active();
            }
            Smooth => {
                let _ = self.smooth_active();
            }
            Sharpen => {
                let _ = self.sharpen_active();
            }
            FindEdges => {
                let _ = self.find_edges_active();
            }
            GaussianBlur => {
                self.dialogs.show_gaussian_blur = true;
            }
            Median => {
                let _ = self.median_active(2.0);
            }
            Minimum => {
                let _ = self.minimum_active(2.0);
            }
            Maximum => {
                let _ = self.maximum_active(2.0);
            }
            Despeckle => {
                let _ = self.despeckle_active();
            }
            MathAdd => {
                let _ = self.math_active(ij_core::math::MathOperation::Add(25.0));
            }
            MathSubtract => {
                let _ = self.math_active(ij_core::math::MathOperation::Subtract(25.0));
            }
            MathMultiply => {
                let _ = self.math_active(ij_core::math::MathOperation::Multiply(1.25));
            }
            MathDivide => {
                let _ = self.math_active(ij_core::math::MathOperation::Divide(1.25));
            }
            MathMin => {
                let _ = self.math_active(ij_core::math::MathOperation::Min(0.0));
            }
            MathMax => {
                let _ = self.math_active(ij_core::math::MathOperation::Max(255.0));
            }
            MathGamma => {
                let _ = self.math_active(ij_core::math::MathOperation::Gamma(0.5));
            }
            MathLog => {
                let _ = self.math_active(ij_core::math::MathOperation::Log);
            }
            MathExp => {
                let _ = self.math_active(ij_core::math::MathOperation::Exp);
            }
            MathSquare => {
                let _ = self.math_active(ij_core::math::MathOperation::Square);
            }
            MathSquareRoot => {
                let _ = self.math_active(ij_core::math::MathOperation::SquareRoot);
            }
            MathReciprocal => {
                let _ = self.math_active(ij_core::math::MathOperation::Reciprocal);
            }
            MathAbs => {
                let _ = self.math_active(ij_core::math::MathOperation::Abs);
            }
            MathSet => {
                let _ = self.math_active(ij_core::math::MathOperation::Set(25.0));
            }
            MakeBinary => {
                let _ = self.make_binary_active();
            }
            ConvertToMask => {
                let _ = self.convert_to_mask_active(false);
            }
            BinaryErode => {
                let _ = self.binary_erode_active();
            }
            BinaryDilate => {
                let _ = self.binary_dilate_active();
            }
            BinaryOpen => {
                let _ = self.binary_open_active();
            }
            BinaryClose => {
                let _ = self.binary_close_active();
            }
            ImageCalculator => {
                let _ = self.open_image_calculator_dialog();
            }
            Measure => {
                let _ = self.measure_active();
            }
            Histogram => {
                if let (Ok(doc), Ok(CommandOutcome::HistogramComputed(summary))) = (
                    self.session.active_doc().map(|doc| doc.title.clone()),
                    self.session.dispatch(Command::Histogram),
                ) {
                    self.histogram_window = Some(crate::app::HistogramWindowState::new(doc, summary));
                }
            }
            AnalyzeParticles => { self.particles.open = true; }
            SetMeasurements => {
                self.open_set_measurements_dialog();
            }
            SetScale => {
                self.open_set_scale_dialog();
            }
            PlotProfile => {
                let title = self.session.active_doc().map(|doc| doc.title.clone()).ok();
                if let (Some(title), Ok(CommandOutcome::ProfileComputed(profile))) =
                    (title, self.session.dispatch(Command::PlotProfile))
                {
                    self.plot_profile_window =
                        Some(crate::app::PlotProfileWindowState::new(&title, &profile));
                }
            }
            ClearResults => {
                let _ = self.session.dispatch(Command::ClearResults);
                self.status_message = "Results cleared".to_string();
            }
            InstalledPlugins => {
                let _ = self.open_plugins_browser();
                let viewport_id = egui::ViewportId::from_hash_of("ij_plugin_browser_window");
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::Focus);
            }
            RunPluginFilter => {
                self.open_plugin_filter_dialog();
            }
            RunCatalogPlugin { index, .. } => {
                if let Err(error) = self.run_catalog_plugin_immediately(index) {
                    self.dialogs.last_error = Some(error);
                }
            }
            ToggleResults => {
                self.show_results_window = !self.show_results_window;
            }
            ActivateImage(id) => {
                let _ = self.activate_image_window(id, ctx);
            }
            About => {
                self.dialogs.show_about = true;
            }
            ZoomIn(id) => {
                if let Some(view) = self.image_views.get_mut(&id) {
                    view.zoom_in();
                }
            }
            ZoomOut(id) => {
                if let Some(view) = self.image_views.get_mut(&id) {
                    view.zoom_out();
                }
            }
            NextSlice(id) | PreviousSlice(id) => {
                if let Some(doc) = self.session.get_image(id) {
                    let size = doc.stack_size();
                    let current = doc.current_slice;
                    let next = if matches!(action, NextSlice(_)) {
                        current + 1
                    } else {
                        current.saturating_sub(1)
                    };
                    if size > 1 && next >= 1 && (next as usize) <= size {
                        self.stop_animation(id);
                        let _ = self.set_active_window(id);
                        let _ = self.set_slice_active(next as i32);
                    }
                }
            }
            ToggleAnimation(id) => {
                self.toggle_animation(id);
            }
        }
    }
}
