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
    SaveResults,
    Close,
    CloseImage(ImageId),
    CloseAll,
    Quit,
    Undo,
    Invert,
    Duplicate,
    Crop,
    ConvertTo8Bit,
    ConvertTo16Bit,
    ConvertTo32Bit,
    ConvertToRgb,
    SelectNone,
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
    Measure,
    Histogram,
    PlotProfile,
    SetMeasurements,
    ClearResults,
    InstalledPlugins,
    RunPluginFilter,
    ToggleResults,
    ActivateImage(ImageId),
    About,
    ZoomIn(ImageId),
    ZoomOut(ImageId),
    NextSlice(ImageId),
    PreviousSlice(ImageId),
    ToggleAnimation(ImageId),
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
// IJ_Props.txt File>Save As entries (original_imagej/IJ_Props.txt 49-66). Only
// the two formats this app can actually write are wired.
const FILE_SAVE_AS: &[MenuNode] = &[
    item("Tiff...", None, AppAction::SaveAs),
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
    unsupported("Revert", None),
    unsupported("Page Setup...", None),
    unsupported("Print...", None),
    SEPARATOR,
    item("Quit", Some("Cmd+Q"), AppAction::Quit),
];

const EDIT_SELECTION: &[MenuNode] = &[item("Select None", Some("Shift+A"), AppAction::SelectNone)];
const EDIT_MENU: &[MenuNode] = &[
    item("Undo", Some("Z"), AppAction::Undo),
    SEPARATOR,
    unsupported("Cut", Some("X")),
    unsupported("Copy", Some("C")),
    unsupported("Paste", Some("V")),
    unsupported("Clear", None),
    unsupported("Fill", Some("F")),
    unsupported("Draw", Some("D")),
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
const PROCESS_MENU: &[MenuNode] = &[
    item("Smooth", None, AppAction::Smooth),
    item("Sharpen", None, AppAction::Sharpen),
    item("Find Edges", None, AppAction::FindEdges),
    SEPARATOR,
    submenu("Filters", PROCESS_FILTERS),
    submenu("Noise", PROCESS_NOISE),
    unsupported("Binary", None),
    unsupported("Math", None),
    unsupported("FFT", None),
];

const ANALYZE_MENU: &[MenuNode] = &[
    item("Measure", Some("M"), AppAction::Measure),
    item("Histogram", Some("H"), AppAction::Histogram),
    item("Clear Results", None, AppAction::ClearResults),
    item("Set Measurements...", None, AppAction::SetMeasurements),
    item("Plot Profile", Some("K"), AppAction::PlotProfile),
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

impl ImageJApp {
    pub fn action_enabled(&self, action: AppAction) -> Result<(), String> {
        use AppAction::*;
        match action {
            // Set Measurements is a user-option dialog, not an image command:
            // ImageJ's Analyze menu enables it with no image open.
            NewRamp | OpenTiff | Quit | InstalledPlugins | ToggleResults | About
            | SetMeasurements => Ok(()),
            SaveResults | ClearResults => {
                if self.session.results.rows.is_empty() {
                    Err("No results to save or clear".into())
                } else {
                    Ok(())
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
                    Threshold if !matches!(doc.plane(), PixelPlane::Gray8(_) | PixelPlane::Gray16(_)) || doc.stack_size() != 1 => Err("Threshold is only supported on single-slice 8-bit or 16-bit grayscale images".into()),
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
                    // ProfilePlot.java 53-62: only a line or rectangular
                    // selection produces a profile, and the two rejection
                    // messages are the ones the constructor prints.
                    PlotProfile => match doc.roi.as_ref() {
                        None => Err("Selection required.".into()),
                        Some(ij_core::Roi::Rect(_) | ij_core::Roi::Line(_)) => Ok(()),
                        Some(_) => Err("Line or rectangular selection required.".into()),
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
                    .add_filter("TIFF Image", &["tif", "tiff"])
                    .pick_file()
                {
                    let _ = self.open_tiff_file(path);
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
                    let _ = self.save_active_tiff(path);
                }
            }
            SaveAs => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("TIFF Image", &["tif", "tiff"])
                    .save_file()
                {
                    let _ = self.save_active_tiff(path);
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
            SelectNone => {
                let _ = self.clear_roi();
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
            SetMeasurements => {
                self.open_set_measurements_dialog();
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
