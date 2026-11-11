use super::*;
use ij_core::format_ij_num;

#[path = "bc_palette.rs"]
mod bc_palette;
#[path = "threshold_palette.rs"]
mod threshold_palette;

/// One checkbox of ImageJ's `Set Measurements` dialog.
///
/// `label` is the exact 1.54u text and the order follows Analyzer's ordered
/// `list` (original_imagej/ij/plugin/filter/Analyzer.java 29-32). `supported`
/// records the flags ImageJ-rs actually honours; the remaining rows are still
/// displayed (so the layout matches ImageJ) but cannot be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetMeasurementsOption {
    pub label: &'static str,
    pub flag: u32,
    pub supported: bool,
}

const SUPPORTED: bool = true;
const NOT_SUPPORTED: bool = false;

pub const SET_MEASUREMENTS_OPTIONS: [SetMeasurementsOption; 24] = [
    SetMeasurementsOption {
        label: "Area",
        flag: MeasurementFlags::AREA,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Mean gray value",
        flag: MeasurementFlags::MEAN,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Standard deviation",
        flag: MeasurementFlags::STD_DEV,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Modal gray value",
        flag: MeasurementFlags::MODE,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Min & max gray value",
        flag: MeasurementFlags::MIN_MAX,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Centroid",
        flag: MeasurementFlags::CENTROID,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Center of mass",
        flag: MeasurementFlags::CENTER_OF_MASS,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Perimeter",
        flag: MeasurementFlags::PERIMETER,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Bounding rectangle",
        flag: MeasurementFlags::RECT,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Fit ellipse",
        flag: MeasurementFlags::ELLIPSE,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Shape descriptors",
        flag: MeasurementFlags::SHAPE_DESCRIPTORS,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Feret's diameter",
        flag: MeasurementFlags::FERET,
        supported: NOT_SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Integrated density",
        flag: MeasurementFlags::INTEGRATED_DENSITY,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Median",
        flag: MeasurementFlags::MEDIAN,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Skewness",
        flag: MeasurementFlags::SKEWNESS,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Kurtosis",
        flag: MeasurementFlags::KURTOSIS,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Area_fraction",
        flag: MeasurementFlags::AREA_FRACTION,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Stack position",
        flag: MeasurementFlags::STACK_POSITION,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Limit to threshold",
        flag: MeasurementFlags::LIMIT,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Display label",
        flag: MeasurementFlags::LABELS,
        supported: NOT_SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Invert Y coordinates",
        flag: MeasurementFlags::INVERT_Y,
        supported: NOT_SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Scientific notation",
        flag: MeasurementFlags::SCIENTIFIC_NOTATION,
        supported: SUPPORTED,
    },
    SetMeasurementsOption {
        label: "Add to overlay",
        flag: MeasurementFlags::ADD_TO_OVERLAY,
        supported: NOT_SUPPORTED,
    },
    SetMeasurementsOption {
        label: "NaN empty cells",
        flag: MeasurementFlags::NAN_EMPTY_CELLS,
        supported: NOT_SUPPORTED,
    },
];

/// First `addCheckboxGroup(10, 2, ...)` covers this many options
/// (Analyzer.java 154-183); the `addCheckboxGroup(3, 2, ...)` group covers the
/// remaining six (Analyzer.java 184-200).
pub const SET_MEASUREMENTS_FIRST_GROUP: usize = 18;
pub const SET_MEASUREMENTS_GROUP_ROWS: usize = 10;
pub const SET_MEASUREMENTS_GROUP_COLUMNS: usize = 2;
pub const SET_MEASUREMENTS_SECOND_ROWS: usize = 3;

/// Option index drawn at `(row, column)` of one checkbox panel.
///
/// `GenericDialog.addCheckboxGroup` fills its `GridLayout` column by column
/// (`i2 = col*rows+row`, GenericDialog.java 653-656), so the left column holds
/// the first `rows` labels rather than every other label.
pub fn set_measurements_slot(
    first: usize,
    count: usize,
    rows: usize,
    row: usize,
    column: usize,
) -> Option<usize> {
    let index = first + column * rows + row;
    (row < rows && index < first + count).then_some(index)
}

/// `doSetDialog` clamps the numeric field to 0..=9 before storing it
/// (Analyzer.java 219-221).
pub fn clamp_set_measurements_precision(value: i64) -> u8 {
    value.clamp(0, 9) as u8
}

/// Editable state of the `Set Measurements` window.
///
/// The defaults reproduce ImageJ's startup options: Area, Mean and Min/Max at
/// three decimals (Analyzer.java 40-42). The dialog edits a copy and only
/// dispatches on OK, so Cancel leaves the session unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetMeasurementsDialog {
    pub open: bool,
    pub flags: u32,
    pub precision: u8,
}

impl Default for SetMeasurementsDialog {
    fn default() -> Self {
        let config = MeasurementConfig::default();
        Self {
            open: false,
            flags: config.flags,
            precision: config.precision,
        }
    }
}

impl SetMeasurementsDialog {
    pub fn contains(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    fn toggle(&mut self, flag: u32, checked: bool) {
        if checked {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }
}

const OPTION_RECT_ID: &str = "ij_set_measurements_option_rect";
const BUTTON_RECT_ID: &str = "ij_set_measurements_button_rect";

impl ImageJApp {
    pub fn render_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(dialog) = self.dialogs.math.as_ref() {
            let title = dialog.operation.name();
            let mut cancel = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_math_dialog"),
                egui::ViewportBuilder::default().with_title(title)
                    .with_inner_size([300.0, 125.0]).with_resizable(false),
                |ctx, class| {
                    if !matches!(class, egui::ViewportClass::Embedded)
                        && ctx.input(|i| i.viewport().close_requested()) { cancel = true; }
                    egui::CentralPanel::default().show(ctx, |ui| self.render_math_content(ui));
                },
            );
            if cancel { let _ = self.resolve_math_dialog(false); }
        }
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

        if self.dialogs.duplicate.open {
            self.render_duplicate_dialog(ctx);
        }

        if self.dialogs.show_brightness_contrast {
            self.render_bc_palette(ctx);
        }

        if self.dialogs.show_threshold {
            self.render_threshold_palette(ctx);
        }

        if self.set_measurements.open {
            self.render_set_measurements_dialog(ctx);
        }

        if self.set_scale.open {
            self.render_set_scale_dialog(ctx);
        }

        if self.show_roi_manager_window {
            self.render_roi_manager_window(ctx);
        }

        self.render_histogram_window(ctx);

        self.render_plot_profile_window(ctx);

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

        if let Some(id) = self.pending_revert {
            let title = self
                .session
                .get_image(id)
                .map(|doc| doc.title.clone())
                .unwrap_or_else(|| format!("Image {}", id.0));
            let mut confirm = None;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_confirm_revert_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("Revert?")
                    .with_inner_size([360.0, 140.0])
                    .with_resizable(false),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        confirm = Some(false);
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.label(format!("Revert to saved version of\n\"{title}\"?"));
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Revert").clicked() {
                                confirm = Some(true);
                            }
                            if ui.button("Cancel").clicked() {
                                confirm = Some(false);
                            }
                        });
                    });
                },
            );
            if let Some(confirm) = confirm {
                self.resolve_revert_confirmation(confirm);
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

    /// Opens `Analyze > Set Measurements...` with the session's live options.
    pub fn open_set_measurements_dialog(&mut self) {
        let config = self.session.results.config;
        self.set_measurements = SetMeasurementsDialog {
            open: true,
            flags: config.flags,
            precision: config.precision.min(9),
        };
    }

    /// Applies the dialog's draft options as one `SetMeasurements` command and
    /// closes the window (Analyzer.doSetDialog only stores on OK).
    pub fn apply_set_measurements_dialog(&mut self) -> Result<(), SessionError> {
        let flags = self.set_measurements.flags;
        let precision =
            clamp_set_measurements_precision(i64::from(self.set_measurements.precision));
        match self
            .session
            .dispatch(Command::SetMeasurements { flags, precision })
        {
            Ok(_) => {
                self.set_measurements.open = false;
                self.set_measurements.flags = flags;
                self.set_measurements.precision = precision;
                Ok(())
            }
            Err(e) => {
                self.status_message = format!("Set Measurements failed: {e}");
                self.dialogs.last_error = Some(format!("Set Measurements failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn set_measurements_option_rect(ctx: &egui::Context, label: &str) -> Option<egui::Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new((OPTION_RECT_ID, label))))
    }

    pub fn set_measurements_button_rect(ctx: &egui::Context, label: &str) -> Option<egui::Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new((BUTTON_RECT_ID, label))))
    }

    /// The same "Set Measurements" content is used by the native window and by
    /// headless egui tests.
    pub fn render_set_measurements_content(&mut self, ui: &mut egui::Ui) {
        let mut dialog = self.set_measurements;

        render_option_group(
            ui,
            0,
            SET_MEASUREMENTS_FIRST_GROUP,
            SET_MEASUREMENTS_GROUP_ROWS,
            &mut dialog,
        );
        ui.add_space(10.0);
        render_option_group(
            ui,
            SET_MEASUREMENTS_FIRST_GROUP,
            SET_MEASUREMENTS_OPTIONS.len() - SET_MEASUREMENTS_FIRST_GROUP,
            SET_MEASUREMENTS_SECOND_ROWS,
            &mut dialog,
        );

        ui.add_space(12.0);
        // ImageJ offers a Redirect list here (Analyzer.java 204). The narrow
        // Rust session has no redirect target, so show the Java default
        // ("None") disabled rather than pretend to redirect.
        ui.horizontal(|ui| {
            ui.label("Redirect to:");
            ui.add_enabled(false, egui::Button::new("None"))
                .on_hover_text("Redirect targets are not supported yet");
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Decimal places (0-9):");
            let mut places = i32::from(dialog.precision);
            ui.add(egui::DragValue::new(&mut places).speed(1.0).range(0..=9));
            dialog.precision = clamp_set_measurements_precision(i64::from(places));
        });

        ui.separator();
        let mut action = None;
        ui.horizontal(|ui| {
            for label in ["OK", "Cancel"] {
                let response = ui.button(label);
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new((BUTTON_RECT_ID, label)), response.rect)
                });
                if response.clicked() {
                    action = Some(label);
                }
            }
        });

        self.set_measurements = dialog;
        match action {
            Some("OK") => {
                let _ = self.apply_set_measurements_dialog();
            }
            Some(_) => {
                self.set_measurements.open = false;
            }
            None => {}
        }
    }

    fn render_set_measurements_dialog(&mut self, ctx: &egui::Context) {
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_set_measurements_dialog"),
            egui::ViewportBuilder::default()
                .with_title("Set Measurements")
                .with_inner_size([430.0, 400.0])
                .with_resizable(false),
            |ctx, class| {
                let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    close = true;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| self.render_set_measurements_content(ui));
                });
            },
        );
        if close {
            self.set_measurements.open = false;
        }
    }
}

/// Initial values ScaleDialog.run derives from the active image
/// (original_imagej/ij/plugin/filter/ScaleDialog.java 24-70).
#[derive(Debug, Clone, PartialEq)]
pub struct SetScaleDefaults {
    pub distance: f64,
    pub known: f64,
    pub aspect: f64,
    pub unit: String,
    /// Uncalibrated length used by "Click to Remove Scale".
    pub raw_length: f64,
}

/// Reads the same initial dialog values as ScaleDialog: a line selection
/// contributes its raw length and a rectangular selection its larger side; an
/// already calibrated image converts those pixels back to physical units.
pub fn set_scale_defaults(doc: &ij_core::ImageDocument) -> SetScaleDefaults {
    let mut distance = 0.0;
    let mut known = 0.0;
    let mut aspect = 1.0;
    let mut unit = "pixel".to_string();
    let mut raw_length = 0.0;

    if let Some(roi) = &doc.roi {
        match roi {
            Roi::Line(line) => {
                distance = line.raw_length();
                raw_length = distance;
            }
            Roi::Rect(rect) => {
                distance = f64::from(rect.width.max(rect.height));
                raw_length = distance;
            }
            _ => {}
        }
    }

    let calibration = &doc.calibration;
    if calibration.scaled() {
        if distance != 0.0 {
            known = distance * calibration.pixel_width;
        } else {
            distance = 1.0 / calibration.pixel_width;
            known = 1.0;
        }
        unit = calibration.unit.clone();
        aspect = calibration.pixel_height / calibration.pixel_width;
    }

    SetScaleDefaults {
        distance,
        known,
        aspect,
        unit,
        raw_length,
    }
}

/// `Tools.getDecimalPlaces` (original_imagej/ij/util/Tools.java 209-222) for
/// the scale label: integer values need no decimals and more than four are
/// rounded back to four.
fn scale_decimal_places(value: f64) -> i32 {
    if (value as i32) as f64 == value || value.is_nan() {
        return 0;
    }
    if value != 0.0 && (value.abs() < 1.0e-3 || value.abs() >= 1.0e7) {
        // Java's Double.toString switches to exponent notation here.
        return -2;
    }
    let mut text = format!("{value}");
    if text.contains('E') {
        return -2;
    }
    while text.ends_with('0') {
        text.pop();
    }
    match text.find('.') {
        Some(index) => ((text.len() - index - 1) as i32).min(4),
        None => 0,
    }
}

/// `IJ.d2s(value, digits)` for the Set Scale label (IJ.java 1000-1029).
fn format_scale_label_number(value: f64, digits: i32) -> String {
    if digits < 0 {
        format!("{value:.2E}")
    } else {
        format!("{:.*}", digits as usize, value)
    }
}

/// Scale: label text from `SetScaleDialog.textValueChanged`
/// (ScaleDialog.java 152-180), including its singular "pixel/" form.
pub fn set_scale_label(distance: f64, known: f64, unit: &str) -> String {
    let no_unit = unit.is_empty() || unit.starts_with("pixel") || unit.starts_with("Pixel");
    if distance <= 0.0 || known <= 0.0 || no_unit {
        return "<no scale>".to_string();
    }
    let scale = distance / known;
    let digits = scale_decimal_places(scale);
    let pixels = if scale == 1.0 { "pixel" } else { "pixels" };
    format!(
        "{} {pixels}/{unit}",
        format_scale_label_number(scale, digits)
    )
}

/// Numeric fields must be real numbers before the dialog can apply them.
pub fn set_scale_input_is_valid(distance: f64, known: f64, aspect: f64) -> bool {
    distance.is_finite() && known.is_finite() && aspect.is_finite()
}

impl ImageJApp {
    /// Opens `Analyze > Set Scale...` with ScaleDialog's initial values.
    pub fn open_set_scale_dialog(&mut self) {
        let defaults = self
            .session
            .active_doc()
            .map(set_scale_defaults)
            .unwrap_or(SetScaleDefaults {
                distance: 0.0,
                known: 0.0,
                aspect: 1.0,
                unit: "pixel".to_string(),
                raw_length: 0.0,
            });
        self.set_scale = SetScaleDialog {
            open: true,
            distance: defaults.distance,
            known: defaults.known,
            aspect: defaults.aspect,
            unit: defaults.unit,
            raw_length: defaults.raw_length,
            removed: false,
        };
    }

    /// Applies the dialog fields as one `SetScale` command (ScaleDialog.java
    /// 80-112 only changes the image when OK is pressed).
    pub fn apply_set_scale_dialog(&mut self) -> Result<(), SessionError> {
        let dialog = self.set_scale.clone();
        if !set_scale_input_is_valid(dialog.distance, dialog.known, dialog.aspect) {
            let message = "Set Scale needs finite numbers".to_string();
            self.status_message = message.clone();
            self.dialogs.last_error = Some(message);
            return Err(SessionError::Core(ij_core::CoreError::InvalidOperation(
                "Set Scale needs finite numbers",
            )));
        }
        let removing = dialog.removed
            && dialog.distance == dialog.raw_length
            && dialog.known == 0.0
            && dialog.aspect == 1.0
            && dialog.unit == "pixel";
        let result = if removing {
            self.session.dispatch(Command::RemoveScale)
        } else {
            self.session.dispatch(Command::SetScale {
                distance: dialog.distance,
                known: dialog.known,
                aspect: dialog.aspect,
                unit: dialog.unit,
            })
        };
        match result {
            Ok(_) => {
                self.set_scale.open = false;
                self.set_scale.removed = false;
                Ok(())
            }
            Err(error) => {
                self.status_message = format!("Set Scale failed: {error}");
                self.dialogs.last_error = Some(format!("Set Scale failed: {error}"));
                Err(error)
            }
        }
    }

    /// "Click to Remove Scale" (ScaleDialog.java 182-203): reset the dialog
    /// fields. ScaleDialog.run applies them only if the user presses OK.
    pub fn remove_scale_from_dialog(&mut self) -> Result<(), SessionError> {
        self.set_scale.distance = self.set_scale.raw_length;
        self.set_scale.known = 0.0;
        self.set_scale.aspect = 1.0;
        self.set_scale.unit = "pixel".to_string();
        self.set_scale.removed = true;
        Ok(())
    }

    pub fn set_scale_button_rect(ctx: &egui::Context, label: &str) -> Option<egui::Rect> {
        ctx.data(|data| {
            data.get_temp(egui::Id::new(("ij_set_scale_button_rect", label)))
        })
    }

    /// The same "Set Scale" content is used by the native window and by
    /// headless egui tests.
    pub fn render_set_scale_content(&mut self, ui: &mut egui::Ui) {
        let mut dialog = self.set_scale.clone();

        ui.horizontal(|ui| {
            ui.label("Scale:");
            ui.label(set_scale_label(dialog.distance, dialog.known, &dialog.unit));
        });
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("Distance in pixels:");
            ui.add(egui::DragValue::new(&mut dialog.distance).speed(0.01));
        });
        ui.horizontal(|ui| {
            ui.label("Known distance:");
            ui.add(egui::DragValue::new(&mut dialog.known).speed(0.01));
        });
        ui.horizontal(|ui| {
            ui.label("Pixel aspect ratio:");
            ui.add(egui::DragValue::new(&mut dialog.aspect).speed(0.001));
        });
        ui.horizontal(|ui| {
            ui.label("Unit of length:");
            ui.add(egui::TextEdit::singleline(&mut dialog.unit).desired_width(90.0));
        });

        ui.add_space(6.0);
        let mut action = None;
        let remove = ui.button("Click to Remove Scale");
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new(("ij_set_scale_button_rect", "Click to Remove Scale")),
                remove.rect,
            )
        });
        if remove.clicked() {
            action = Some("Click to Remove Scale");
        }

        ui.separator();
        let valid = set_scale_input_is_valid(dialog.distance, dialog.known, dialog.aspect);
        ui.horizontal(|ui| {
            for label in ["OK", "Cancel"] {
                let response = ui.add_enabled(valid, egui::Button::new(label));
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new(("ij_set_scale_button_rect", label)), response.rect)
                });
                if response.clicked() {
                    action = Some(label);
                }
            }
        });

        self.set_scale = dialog;
        match action {
            Some("Click to Remove Scale") => {
                let _ = self.remove_scale_from_dialog();
            }
            Some("OK") => {
                let _ = self.apply_set_scale_dialog();
            }
            Some(_) => {
                self.set_scale.open = false;
            }
            None => {}
        }
    }

    fn render_set_scale_dialog(&mut self, ctx: &egui::Context) {
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_set_scale_dialog"),
            egui::ViewportBuilder::default()
                .with_title("Set Scale")
                .with_inner_size([360.0, 230.0])
                .with_resizable(false),
            |ctx, class| {
                let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    close = true;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.render_set_scale_content(ui);
                });
            },
        );
        if close {
            self.set_scale.open = false;
        }
    }
}

/// Parses Duplicator's Range field. `Tools.split(text, " -")` collapses runs of
/// spaces/hyphens, a missing last token means `stackSize`, and the Java
/// `(int)` casts truncate toward zero before the 1..stackSize clamp
/// (original_imagej/ij/plugin/Duplicator.java 511-521).
pub fn parse_duplicate_range(range: &str, stack_size: u32) -> (u32, u32) {
    let stack_size = stack_size.max(1);
    let tokens: Vec<&str> = range
        .split([' ', '-'])
        .filter(|token| !token.is_empty())
        .collect();
    let parse = |token: &str| token.trim().parse::<f64>().ok().unwrap_or(f64::NAN);
    let d1 = tokens.first().map(|token| parse(token)).unwrap_or(1.0);
    // Java only parses a second token when exactly two were produced; every
    // other shape leaves d2 at its `stackSize` default (Duplicator.java 512-518).
    let d2 = if tokens.len() == 2 {
        parse(tokens[1])
    } else {
        f64::from(stack_size)
    };
    let mut first = if d1.is_nan() { 1 } else { d1 as i64 };
    let mut last = if d2.is_nan() {
        i64::from(stack_size)
    } else {
        d2 as i64
    };
    if first < 1 {
        first = 1;
    }
    if last > i64::from(stack_size) {
        last = i64::from(stack_size);
    }
    if first > last {
        first = 1;
        last = i64::from(stack_size);
    }
    (first.max(1) as u32, last.max(1) as u32)
}

const DUPLICATE_BUTTON_RECT_ID: &str = "ij_duplicate_button_rect";

impl ImageJApp {
    /// Opens `Image > Duplicate...` for the image that is active right now and
    /// pins that `ImageId`; later focus changes must not retarget the dialog.
    pub fn open_duplicate_dialog(&mut self) {
        let Some(id) = self.session.active_image_id() else {
            self.status_message = "Duplicate requires an active image".into();
            return;
        };
        let (title, stack_size) = {
            let Some(doc) = self.session.get_image(id) else {
                self.status_message = "Duplicate requires an active image".into();
                return;
            };
            let existing: Vec<String> = self
                .session
                .image_ids()
                .into_iter()
                .filter_map(|image_id| self.session.get_image(image_id))
                .map(|image| image.title.clone())
                .collect();
            let existing: Vec<&str> = existing.iter().map(String::as_str).collect();
            (
                ij_core::default_duplicate_title(&existing, &doc.title),
                doc.stack_size() as u32,
            )
        };
        self.dialogs.duplicate = crate::dialogs::DuplicateDialog {
            open: true,
            source: Some(id),
            title,
            // Duplicator.java 56 and 485-486.
            duplicate_stack: stack_size > 1 && self.dialogs.duplicate_stack_default,
            range: format!("1-{stack_size}"),
            stack_size,
        };
    }

    /// Cancel leaves the session untouched (Duplicator.java 503-504, 527-529).
    pub fn cancel_duplicate_dialog(&mut self) {
        self.dialogs.duplicate.open = false;
    }

    /// Applies the dialog draft as exactly one `Command::Duplicate`. The pinned
    /// source is activated immediately before dispatch so a focus change while
    /// the dialog was open cannot duplicate a different image.
    pub fn apply_duplicate_dialog(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<Option<ImageId>, SessionError> {
        if !self.dialogs.duplicate.open {
            return Ok(None);
        }
        let dialog = self.dialogs.duplicate.clone();
        self.dialogs.duplicate.open = false;

        let Some(source) = dialog.source else {
            self.status_message = "Duplicate source is no longer open".into();
            return Ok(None);
        };
        if self.session.get_image(source).is_none() {
            self.status_message = "Duplicate source is no longer open".into();
            return Ok(None);
        }
        if let Some(err) = self.reject_while_closing("Duplicate") {
            return Err(err);
        }

        let previous_active = self.session.active_image_id();
        if previous_active != Some(source) {
            if let Err(error) = self.session.set_active_image(source) {
                self.status_message = format!("Duplicate failed: {error}");
                self.dialogs.last_error = Some(format!("Duplicate failed: {error}"));
                return Err(error);
            }
        }

        let stack_size = dialog.stack_size.max(1);
        let is_stack = stack_size > 1;
        let duplicate_stack = dialog.duplicate_stack && is_stack;
        let (first, last) = parse_duplicate_range(&dialog.range, stack_size);
        let command = Command::Duplicate {
            title: Some(dialog.title.clone()),
            duplicate_stack,
            first_slice: duplicate_stack.then_some(first),
            last_slice: duplicate_stack.then_some(last),
        };

        match self.session.dispatch(command) {
            Ok(CommandOutcome::ImageOpened(id)) => {
                if is_stack {
                    self.dialogs.duplicate_stack_default = dialog.duplicate_stack;
                }
                if let Some(doc) = self.session.get_image(id) {
                    let (width, height, title) = (doc.width, doc.height, doc.title.clone());
                    self.image_views.insert(id, ImageView::new(width, height));
                    self.status_message = format!("Duplicated \"{title}\"");
                }
                let _ = self.activate_image_window(id, ctx);
                Ok(Some(id))
            }
            Ok(_) => {
                if let Some(previous) = previous_active {
                    if previous != source {
                        let _ = self.session.set_active_image(previous);
                    }
                }
                Ok(None)
            }
            Err(error) => {
                if let Some(previous) = previous_active {
                    if previous != source {
                        let _ = self.session.set_active_image(previous);
                    }
                }
                self.status_message = format!("Duplicate failed: {error}");
                self.dialogs.last_error = Some(format!("Duplicate failed: {error}"));
                Err(error)
            }
        }
    }

    pub fn duplicate_button_rect(ctx: &egui::Context, label: &str) -> Option<egui::Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new((DUPLICATE_BUTTON_RECT_ID, label))))
    }

    /// Content shared by the immediate viewport and headless egui tests.
    pub fn render_duplicate_content(&mut self, ui: &mut egui::Ui) {
        let mut dialog = self.dialogs.duplicate.clone();
        ui.horizontal(|ui| {
            ui.label("Title: ");
            ui.add(egui::TextEdit::singleline(&mut dialog.title).desired_width(220.0));
        });
        if dialog.stack_size > 1 {
            // Duplicator.java 487-489.
            ui.checkbox(&mut dialog.duplicate_stack, "Duplicate stack");
            ui.horizontal(|ui| {
                ui.label("Range:");
                ui.add(egui::TextEdit::singleline(&mut dialog.range).desired_width(80.0));
            });
        }
        ui.separator();
        let mut action = None;
        ui.horizontal(|ui| {
            for label in ["OK", "Cancel"] {
                let response = ui.button(label);
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new((DUPLICATE_BUTTON_RECT_ID, label)),
                        response.rect,
                    )
                });
                if response.clicked() {
                    action = Some(label);
                }
            }
        });
        self.dialogs.duplicate = dialog;
        match action {
            Some("OK") => {
                let _ = self.apply_duplicate_dialog(ui.ctx());
            }
            Some(_) => self.cancel_duplicate_dialog(),
            None => {}
        }
    }

    fn render_duplicate_dialog(&mut self, ctx: &egui::Context) {
        let mut cancel = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_duplicate_dialog"),
            egui::ViewportBuilder::default()
                .with_title("Duplicate...")
                .with_inner_size([380.0, 200.0])
                .with_resizable(false),
            |ctx, class| {
                let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    cancel = true;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.render_duplicate_content(ui);
                });
            },
        );
        if cancel {
            self.cancel_duplicate_dialog();
        }
    }
}

fn render_option_group(
    ui: &mut egui::Ui,
    first: usize,
    count: usize,
    rows: usize,
    dialog: &mut SetMeasurementsDialog,
) {
    egui::Grid::new(("ij_set_measurements_group", first))
        .num_columns(SET_MEASUREMENTS_GROUP_COLUMNS)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for row in 0..rows {
                for column in 0..SET_MEASUREMENTS_GROUP_COLUMNS {
                    match set_measurements_slot(first, count, rows, row, column) {
                        Some(index) => {
                            let option = SET_MEASUREMENTS_OPTIONS[index];
                            render_option(ui, &option, dialog);
                        }
                        None => {
                            ui.label("");
                        }
                    }
                }
                ui.end_row();
            }
        });
}

fn render_option(
    ui: &mut egui::Ui,
    option: &SetMeasurementsOption,
    dialog: &mut SetMeasurementsDialog,
) {
    let mut checked = dialog.contains(option.flag);
    let response = ui.add_enabled(
        option.supported,
        egui::Checkbox::new(&mut checked, option.label),
    );
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new((OPTION_RECT_ID, option.label)), response.rect)
    });
    if !option.supported {
        response.on_hover_text("Not implemented in ImageJ-rs yet");
        return;
    }
    if response.changed() {
        dialog.toggle(option.flag, checked);
    }
}

// ---------------------------------------------------------------------------
// Analyze > Plot Profile
// ---------------------------------------------------------------------------

/// ImageJ's default y-axis label for uncalibrated images
/// (`Calibration.DEFAULT_VALUE_UNIT`, original_imagej/ij/measure/Calibration.java
/// 12). `ProfilePlot` passes `cal.getValueUnit()` (ProfilePlot.java 68), and
/// `ij_core::Calibration` does not model value units yet, so every supported
/// image shows the default.
pub const PLOT_PROFILE_Y_LABEL: &str = "Gray Value";

/// `ProfilePlot.getShortTitle` (original_imagej/ij/gui/ProfilePlot.java 143-149).
///
/// Java strips the extension only when the last dot is not the first character
/// and the dot plus extension is at most five characters, so `ramp.tif` becomes
/// `ramp` while `a.longext` is kept.
pub fn plot_profile_short_title(title: &str) -> String {
    let Some(index) = title.rfind('.') else {
        return title.to_string();
    };
    // Java compares UTF-16 lengths; counting chars keeps multi-byte titles
    // behaving like the reference instead of growing by the extra UTF-8 bytes.
    let total = title.chars().count();
    let prefix = title[..index].chars().count();
    if index > 0 && total - prefix <= 5 {
        title[..index].to_string()
    } else {
        title.to_string()
    }
}

/// State of one `Plot of <short title>` window.
///
/// A snapshot: it owns the sampled values and never borrows the document, so
/// rendering or closing it cannot mutate the image, the ROI or the recorder.
#[derive(Debug, Clone)]
pub struct PlotProfileWindowState {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub x_values: Vec<f64>,
    pub y_values: Vec<f64>,
    pub show_list: bool,
}

impl PlotProfileWindowState {
    pub fn new(image_title: &str, profile: &ProfileData) -> Self {
        Self {
            // ProfilePlot.java 125: new Plot("Plot of "+getShortTitle(imp), ...)
            title: format!("Plot of {}", plot_profile_short_title(image_title)),
            // ProfilePlot.java 117: xLabel = "Distance ("+units+")".
            x_label: format!("Distance ({})", profile.units),
            y_label: PLOT_PROFILE_Y_LABEL.to_string(),
            // ProfilePlot.java 120-122: xValues[i] = (float)(i*xInc). Keeping
            // the float cast lists the same digits Java stores.
            x_values: (0..profile.values.len())
                .map(|i| f64::from((i as f64 * profile.x_inc) as f32))
                .collect(),
            y_values: profile.values.clone(),
            show_list: false,
        }
    }

    pub fn x_range(&self) -> (f64, f64) {
        match (self.x_values.first(), self.x_values.last()) {
            (Some(&first), Some(&last)) if last > first => (first, last),
            (Some(&first), _) => (first, first + 1.0),
            _ => (0.0, 1.0),
        }
    }

    /// Autoscaled y range over the finite samples; `Plot` autoscale ignores
    /// non-finite data, and a flat profile is padded so the axis has a span.
    pub fn y_range(&self) -> (f64, f64) {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for &value in &self.y_values {
            if value.is_finite() {
                min = min.min(value);
                max = max.max(value);
            }
        }
        if !min.is_finite() || !max.is_finite() {
            return (0.0, 1.0);
        }
        if max > min {
            (min, max)
        } else {
            (min - 0.5, max + 0.5)
        }
    }

    /// Normalized polyline runs for this window's axes.
    pub fn runs(&self) -> Vec<Vec<(f32, f32)>> {
        plot_profile_runs(
            &self.x_values,
            &self.y_values,
            self.x_range(),
            self.y_range(),
        )
    }

    /// `PlotWindow.showList` (PlotWindow.java 663-668) shows
    /// `Plot.getResultsTable` (Plot.java 3852-3900): one `X`/`Y` row per sample,
    /// formatted like a ResultsTable column.
    pub fn values_tsv(&self) -> String {
        let mut out = String::from("X\tY\n");
        for (x, y) in self.x_values.iter().zip(&self.y_values) {
            out.push_str(&format!("{}\t{}\n", format_ij_num(*x), format_ij_num(*y)));
        }
        out
    }
}

/// Normalized (`0..1`, y up) plot coordinates split into runs at NaN samples.
///
/// A NaN sample has no drawable coordinate, so the polyline is broken there
/// instead of connecting through it. `ProfilePlot.getColumnAverageProfile`
/// leaves `0/0 = NaN` for a column with no in-bounds sample
/// (ProfilePlot.java 210-225).
pub fn plot_profile_runs(
    x_values: &[f64],
    y_values: &[f64],
    x_range: (f64, f64),
    y_range: (f64, f64),
) -> Vec<Vec<(f32, f32)>> {
    let (x_min, x_max) = x_range;
    let (y_min, y_max) = y_range;
    let x_span = x_max - x_min;
    let y_span = y_max - y_min;
    let mut runs: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    for (&x, &y) in x_values.iter().zip(y_values) {
        if !x.is_finite() || !y.is_finite() {
            if !current.is_empty() {
                runs.push(std::mem::take(&mut current));
            }
            continue;
        }
        let nx = if x_span > 0.0 {
            ((x - x_min) / x_span) as f32
        } else {
            0.0
        };
        let ny = if y_span > 0.0 {
            ((y - y_min) / y_span) as f32
        } else {
            0.0
        };
        current.push((nx.clamp(0.0, 1.0), ny.clamp(0.0, 1.0)));
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

impl ImageJApp {
    /// `Analyze > Plot Profile` window: line graph with calibrated
    /// distance/intensity axes plus the `List` button of PlotWindow.java
    /// 222-266.
    pub fn render_plot_profile_window(&mut self, ctx: &egui::Context) {
        let Some(window) = self.plot_profile_window.as_ref().cloned() else {
            return;
        };
        let mut close = false;
        let mut show_list = window.show_list;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_plot_profile_window"),
            egui::ViewportBuilder::default()
                .with_title(&window.title)
                .with_inner_size([420.0, 360.0])
                .with_min_inner_size([320.0, 280.0]),
            |ctx, class| {
                if !matches!(class, egui::ViewportClass::Embedded)
                    && ctx.input(|i| i.viewport().close_requested())
                {
                    close = true;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("List").clicked() {
                            show_list = !show_list;
                        }
                    });
                    ui.separator();
                    let (outer, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().max(220.0), 190.0),
                        Sense::hover(),
                    );
                    // Leave room for the axis labels outside the frame.
                    let rect = Rect::from_min_max(
                        outer.min + egui::vec2(40.0, 14.0),
                        outer.max - egui::vec2(8.0, 24.0),
                    );
                    let painter = ui.painter();
                    let (x_min, x_max) = window.x_range();
                    let (y_min, y_max) = window.y_range();
                    let to_screen = |(nx, ny): (f32, f32)| {
                        egui::pos2(
                            rect.left() + nx * rect.width(),
                            rect.bottom() - ny * rect.height(),
                        )
                    };
                    painter.rect_stroke(
                        rect,
                        CornerRadius::ZERO,
                        Stroke::new(1.0_f32, Color32::BLACK),
                        egui::StrokeKind::Inside,
                    );
                    let stroke = Stroke::new(1.0_f32, Color32::from_rgb(20, 20, 160));
                    for run in window.runs() {
                        if run.len() == 1 {
                            painter.circle_filled(to_screen(run[0]), 1.5, Color32::BLACK);
                        }
                        for pair in run.windows(2) {
                            painter.line_segment([to_screen(pair[0]), to_screen(pair[1])], stroke);
                        }
                    }
                    let tick_font = egui::FontId::proportional(10.0);
                    let label_font = egui::FontId::proportional(11.0);
                    painter.text(
                        egui::pos2(outer.left(), rect.top() - 2.0),
                        egui::Align2::LEFT_BOTTOM,
                        &window.y_label,
                        label_font.clone(),
                        Color32::BLACK,
                    );
                    painter.text(
                        egui::pos2(rect.left() - 4.0, rect.top()),
                        egui::Align2::RIGHT_CENTER,
                        format_ij_num(y_max),
                        tick_font.clone(),
                        Color32::BLACK,
                    );
                    painter.text(
                        egui::pos2(rect.left() - 4.0, rect.bottom()),
                        egui::Align2::RIGHT_CENTER,
                        format_ij_num(y_min),
                        tick_font.clone(),
                        Color32::BLACK,
                    );
                    painter.text(
                        egui::pos2(rect.center().x, rect.bottom() + 4.0),
                        egui::Align2::CENTER_TOP,
                        &window.x_label,
                        label_font,
                        Color32::BLACK,
                    );
                    painter.text(
                        egui::pos2(rect.left(), rect.bottom() + 4.0),
                        egui::Align2::LEFT_TOP,
                        format_ij_num(x_min),
                        tick_font.clone(),
                        Color32::BLACK,
                    );
                    painter.text(
                        egui::pos2(rect.right(), rect.bottom() + 4.0),
                        egui::Align2::RIGHT_TOP,
                        format_ij_num(x_max),
                        tick_font,
                        Color32::BLACK,
                    );
                    if show_list {
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .show(ui, |ui| {
                                ui.monospace(window.values_tsv());
                            });
                    }
                });
            },
        );
        if close {
            self.plot_profile_window = None;
        } else if let Some(state) = self.plot_profile_window.as_mut() {
            state.show_list = show_list;
        }
    }
}

impl ImageJApp {
    pub fn roi_manager_button_rect(ctx: &egui::Context, label: &str) -> Option<egui::Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new(("ij_roi_manager_button", label))))
    }

    pub fn render_roi_manager_content(&mut self, ui: &mut egui::Ui) {
        let can_add = self.session.active_doc().ok().is_some_and(|doc| doc.roi.is_some());
        let has_entries = !self.session.roi_manager.is_empty();
        let has_image = self.session.active_image_id().is_some();

        ui.horizontal(|ui| {
            let add = ui.add_enabled(can_add, egui::Button::new("Add"));
            ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(("ij_roi_manager_button", "Add")), add.rect));
            if add.clicked() {
                if let Err(error) = self.session.dispatch(Command::RoiManagerAdd) {
                    self.dialogs.last_error = Some(error.to_string());
                }
            }
            let delete = ui.add_enabled(has_entries, egui::Button::new("Delete"));
            ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(("ij_roi_manager_button", "Delete")), delete.rect));
            if delete.clicked() {
                if self.session.roi_manager_selected.is_empty() {
                    self.pending_roi_manager_delete_all = true;
                } else {
                    let indices = self.session.roi_manager_selected.clone();
                    let _ = self.session.dispatch(Command::RoiManagerDelete { indices });
                }
            }
            let measure = ui.add_enabled(has_entries && has_image, egui::Button::new("Measure"));
            ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(("ij_roi_manager_button", "Measure")), measure.rect));
            if measure.clicked() {
                let indices = self.session.roi_manager_selected.clone();
                match self.session.dispatch(Command::RoiManagerMeasure { indices }) {
                    Ok(_) => self.show_results_window = true,
                    Err(error) => self.dialogs.last_error = Some(error.to_string()),
                }
            }
        });

        if self.pending_roi_manager_delete_all {
            ui.separator();
            ui.label("Delete all items on the list?");
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    let _ = self.session.dispatch(Command::RoiManagerDelete { indices: Vec::new() });
                    self.pending_roi_manager_delete_all = false;
                }
                if ui.button("Cancel").clicked() {
                    self.pending_roi_manager_delete_all = false;
                }
            });
        }

        ui.separator();
        let entries = self
            .session
            .roi_manager
            .iter()
            .map(|entry| entry.name.clone())
            .collect::<Vec<_>>();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, name) in entries.iter().enumerate() {
                let selected = self.session.roi_manager_selected.contains(&index);
                if ui.selectable_label(selected, name).clicked() {
                    let multi = ui.input(|input| input.modifiers.command || input.modifiers.shift);
                    let mut selected_indices = self.session.roi_manager_selected.clone();
                    if multi {
                        if let Some(position) = selected_indices.iter().position(|&item| item == index) {
                            selected_indices.remove(position);
                        } else {
                            selected_indices.push(index);
                            selected_indices.sort_unstable();
                        }
                    } else {
                        selected_indices = vec![index];
                    }
                    if !selected_indices.is_empty() {
                        let _ = self.session.dispatch(Command::RoiManagerSelect { index });
                    }
                    self.session.roi_manager_selected = selected_indices;
                }
            }
        });
    }

    fn render_roi_manager_window(&mut self, ctx: &egui::Context) {
        let mut is_open = true;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_roi_manager_window"),
            egui::ViewportBuilder::default()
                .with_title("ROI Manager")
                .with_inner_size([300.0, 360.0])
                .with_min_inner_size([220.0, 200.0]),
            |ctx, class| {
                if !matches!(class, egui::ViewportClass::Embedded)
                    && ctx.input(|input| input.viewport().close_requested())
                {
                    is_open = false;
                }
                egui::CentralPanel::default().show(ctx, |ui| self.render_roi_manager_content(ui));
            },
        );
        if !is_open {
            self.show_roi_manager_window = false;
            self.pending_roi_manager_delete_all = false;
        }
    }
}

#[cfg(test)]
mod duplicate_dialog_tests {
    use super::*;
    use crate::action::AppAction;
    use egui::{
        CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2,
    };
    use ij_core::ImageDocument;

    fn stack_fixture() -> (ImageJApp, ImageId) {
        let planes: Vec<PixelPlane> = (1..=4u8)
            .map(|value| PixelPlane::Gray8(vec![value; 4]))
            .collect();
        let doc = ImageDocument::new_stack("stack.tif", 2, 2, planes).unwrap();
        let mut app = ImageJApp::new();
        let id = app.session.insert_doc(doc);
        app.image_views.insert(id, ImageView::new(2, 2));
        app.set_active_window(id).unwrap();
        (app, id)
    }

    fn render_duplicate(ctx: &Context, app: &mut ImageJApp, events: Vec<Event>) {
        let mut input = RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 220.0)));
        input.events = events;
        let _ = ctx.run(input, |ctx| {
            CentralPanel::default().show(ctx, |ui| app.render_duplicate_content(ui));
        });
    }

    fn click_duplicate_button(ctx: &Context, app: &mut ImageJApp, label: &str) {
        render_duplicate(ctx, app, vec![]);
        let pos = ImageJApp::duplicate_button_rect(ctx, label)
            .unwrap_or_else(|| panic!("missing duplicate dialog button rect for {label}"))
            .center();
        render_duplicate(ctx, app, vec![Event::PointerMoved(pos)]);
        render_duplicate(
            ctx,
            app,
            vec![Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
        );
        render_duplicate(
            ctx,
            app,
            vec![Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
    }

    #[test]
    fn dialog_defaults_to_unique_title_and_stack_controls() {
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        let id = app.open_ramp_8bit("blobs.tif", 4, 4).unwrap();

        app.perform(AppAction::Duplicate, &ctx);

        let dialog = &app.dialogs.duplicate;
        assert!(dialog.open);
        assert_eq!(dialog.source, Some(id));
        // WindowManager.getUniqueName (WindowManager.java 357-385) and
        // Duplicator.java 54-56.
        assert_eq!(dialog.title, "blobs-1.tif");
        assert!(!dialog.duplicate_stack);
        assert_eq!(dialog.stack_size, 1);
        assert_eq!(dialog.range, "1-1");

        let (mut app, id) = stack_fixture();
        app.perform(AppAction::Duplicate, &ctx);
        let dialog = &app.dialogs.duplicate;
        assert_eq!(dialog.source, Some(id));
        assert_eq!(dialog.title, "stack-1.tif");
        assert!(!dialog.duplicate_stack);
        assert_eq!(dialog.stack_size, 4);
        // Duplicator.java 488-489.
        assert_eq!(dialog.range, "1-4");
    }

    #[test]
    fn range_parsing_matches_duplicator_show_dialog() {
        // Duplicator.java 511-521.
        assert_eq!(parse_duplicate_range("2-3", 4), (2, 3));
        assert_eq!(parse_duplicate_range(" 2 - 3 ", 4), (2, 3));
        assert_eq!(parse_duplicate_range("", 4), (1, 4));
        assert_eq!(parse_duplicate_range("3", 4), (3, 4));
        assert_eq!(parse_duplicate_range("0-9", 4), (1, 4));
        assert_eq!(parse_duplicate_range("3-1", 4), (1, 4));
        assert_eq!(parse_duplicate_range("abc", 4), (1, 4));
    }

    #[test]
    fn cancel_keeps_session_unchanged() {
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        let id = app.open_ramp_8bit("blobs.tif", 4, 4).unwrap();
        app.session.macro_recorder.clear();
        app.perform(AppAction::Duplicate, &ctx);
        app.dialogs.duplicate.title = "changed".into();
        app.dialogs.duplicate.range = "2-3".into();

        app.cancel_duplicate_dialog();

        assert!(!app.dialogs.duplicate.open);
        assert_eq!(app.session.image_ids(), vec![id]);
        assert_eq!(app.session.active_image_id(), Some(id));
        assert!(app.session.macro_recorder.is_empty());
        assert_eq!(app.image_views.len(), 1);
    }

    #[test]
    fn single_duplicate_opens_registered_view_and_records_one_command() {
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        let source = app.open_ramp_8bit("blobs.tif", 4, 4).unwrap();
        app.perform(AppAction::Duplicate, &ctx);
        app.dialogs.duplicate.title = "copy".into();

        let new_id = app
            .apply_duplicate_dialog(&ctx)
            .unwrap()
            .expect("OK should open a duplicate");

        assert_ne!(new_id, source);
        assert_eq!(app.session.image_ids().len(), 2);
        assert_eq!(app.session.active_image_id(), Some(new_id));
        assert!(app.image_views.contains_key(&new_id));
        assert_eq!(app.image_views[&new_id].width, 4);
        assert_eq!(app.image_views[&new_id].height, 4);
        assert_eq!(app.session.get_image(new_id).unwrap().title, "copy");
        assert_eq!(
            app.session.get_image(source).unwrap().title,
            "blobs.tif",
            "the source document must survive the duplicate"
        );

        // One and only one macro entry, produced by Command::Duplicate.
        let records: Vec<_> = app
            .session
            .macro_recorder
            .iter()
            .filter(|record| record.starts_with("run(\"Duplicate...\""))
            .collect();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0],
            "run(\"Duplicate...\", \"title=copy\");"
        );
    }

    #[test]
    fn headless_dialog_ok_opens_duplicate() {
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        app.open_ramp_8bit("blobs.tif", 4, 4).unwrap();
        app.session.macro_recorder.clear();
        app.perform(AppAction::Duplicate, &ctx);

        click_duplicate_button(&ctx, &mut app, "OK");

        assert!(!app.dialogs.duplicate.open);
        assert_eq!(app.session.image_ids().len(), 2);
        assert_eq!(app.image_views.len(), 2);
        assert_eq!(app.session.macro_recorder.len(), 1);
    }

    #[test]
    fn duplicate_preserves_source_roi_calibration_and_display() {
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        let source = app.open_ramp_8bit("blobs.tif", 10, 8).unwrap();
        {
            let doc = app.session.get_image_mut(source).unwrap();
            doc.set_rect_roi(2, 1, 4, 3).unwrap();
            doc.calibration.pixel_width = 0.5;
            doc.calibration.pixel_height = 0.25;
            doc.calibration.unit = "mm".into();
            doc.display_range = ij_core::DisplayRange {
                min: 10.0,
                max: 200.0,
            };
        }
        let before = app.session.get_image(source).unwrap().clone();

        app.perform(AppAction::Duplicate, &ctx);
        app.apply_duplicate_dialog(&ctx).unwrap().unwrap();

        assert_eq!(app.session.get_image(source), Some(&before));
    }

    #[test]
    fn stack_range_two_to_three_copies_only_selected_slices() {
        let ctx = Context::default();
        let (mut app, source) = stack_fixture();
        let source_before = app.session.get_image(source).unwrap().clone();
        app.perform(AppAction::Duplicate, &ctx);
        {
            let dialog = &mut app.dialogs.duplicate;
            dialog.duplicate_stack = true;
            dialog.range = "2-3".into();
        }

        let new_id = app.apply_duplicate_dialog(&ctx).unwrap().unwrap();

        let new_doc = app.session.get_image(new_id).unwrap();
        assert_eq!(new_doc.stack_size(), 2);
        assert_eq!(
            new_doc.slices,
            vec![
                PixelPlane::Gray8(vec![2; 4]),
                PixelPlane::Gray8(vec![3; 4]),
            ]
        );
        assert_eq!(app.session.get_image(source), Some(&source_before));
        assert_eq!(
            app.session.macro_recorder.last().map(String::as_str),
            Some("run(\"Duplicate...\", \"title=stack-1.tif duplicate range=2-3\");")
        );
    }

    #[test]
    fn stack_checkbox_state_persists_like_static_duplicate_stack() {
        let ctx = Context::default();
        let (mut app, _) = stack_fixture();
        app.perform(AppAction::Duplicate, &ctx);
        app.dialogs.duplicate.duplicate_stack = true;
        app.apply_duplicate_dialog(&ctx).unwrap().unwrap();

        app.perform(AppAction::Duplicate, &ctx);
        assert!(app.dialogs.duplicate.duplicate_stack);
        // Duplicator.java 488-489 resets the Range field to 1-stackSize.
        assert_eq!(app.dialogs.duplicate.range, "1-4");
    }

    #[test]
    fn dialog_duplicates_pinned_source_when_focus_moves() {
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        let first = app
            .session
            .insert_doc(ImageDocument::new("first", 2, 1, PixelPlane::Gray8(vec![1, 2])).unwrap());
        app.image_views.insert(first, ImageView::new(2, 1));
        app.set_active_window(first).unwrap();
        let second = app
            .session
            .insert_doc(ImageDocument::new("second", 2, 1, PixelPlane::Gray8(vec![9, 9])).unwrap());
        app.image_views.insert(second, ImageView::new(2, 1));
        app.set_active_window(first).unwrap();

        app.perform(AppAction::Duplicate, &ctx);
        app.set_active_window(second).unwrap();
        let new_id = app.apply_duplicate_dialog(&ctx).unwrap().unwrap();

        let duplicated = app.session.get_image(new_id).unwrap();
        assert_eq!(duplicated.title, "first-1");
        assert_eq!(duplicated.slices, vec![PixelPlane::Gray8(vec![1, 2])]);
        assert_eq!(
            app.session.get_image(second).unwrap().slices,
            vec![PixelPlane::Gray8(vec![9, 9])],
            "the focused image must not be duplicated"
        );
    }

    #[test]
    fn closed_source_closes_dialog_without_dispatch() {
        let ctx = Context::default();
        let mut app = ImageJApp::new();
        let source = app.open_ramp_8bit("blobs.tif", 4, 4).unwrap();
        app.session.macro_recorder.clear();
        app.perform(AppAction::Duplicate, &ctx);

        app.session.close_image(source);
        app.image_views.remove(&source);
        let result = app.apply_duplicate_dialog(&ctx);

        assert!(result.unwrap().is_none());
        assert!(!app.dialogs.duplicate.open);
        assert!(app.session.image_ids().is_empty());
        assert!(app.session.macro_recorder.is_empty());
        assert!(app.status_message.contains("no longer open"));
    }
}

#[cfg(test)]
mod math_dialog_tests {
    use super::*;
    use crate::action::AppAction;
    use ij_core::{ImageDocument, math::MathOperation};

    fn fixture(stack: bool) -> ImageJApp {
        let mut app = ImageJApp::new();
        let doc = ImageDocument::new_stack(
            "math",
            2,
            2,
            vec![PixelPlane::Gray8(vec![1, 2, 3, 4]); if stack { 3 } else { 1 }],
        )
        .unwrap();
        app.session.insert_doc(doc);
        app
    }

    #[test]
    fn math_parameter_dialog_cancel_validation_and_stale_target() {
        let mut app = fixture(false);
        let ctx = egui::Context::default();
        app.perform(AppAction::MathAdd, &ctx);
        assert_eq!(
            app.dialogs.math.as_ref().unwrap().operation,
            MathOperation::Add(25.0)
        );
        let before = app.session.active_doc().unwrap().clone();
        app.resolve_math_dialog(false).unwrap();
        assert_eq!(app.session.active_doc().unwrap(), &before);
        app.perform(AppAction::MathGamma, &ctx);
        app.dialogs.math.as_mut().unwrap().operation = MathOperation::Gamma(0.01);
        assert!(app.resolve_math_dialog(true).is_err());
        assert!(app.dialogs.math.is_some());
        assert_eq!(app.session.active_doc().unwrap(), &before);
        app.resolve_math_dialog(false).unwrap();
        app.perform(AppAction::MathSet, &ctx);
        app.session
            .insert_doc(ImageDocument::new_ramp_8bit("other", 2, 2).unwrap());
        assert!(app.resolve_math_dialog(true).is_err());
        assert!(app.dialogs.math.is_none());
    }

    #[test]
    fn math_headless_enter_and_stack_choices_dispatch() {
        let ctx = egui::Context::default();
        let mut app = fixture(false);
        app.perform(AppAction::MathAdd, &ctx);
        app.dialogs.math.as_mut().unwrap().operation = MathOperation::Add(2.9);
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.render_math_content(ui));
        });
        assert!(app.dialogs.math.is_none());
        assert_eq!(
            app.session.active_doc().unwrap().plane(),
            &PixelPlane::Gray8(vec![3, 4, 5, 6])
        );
        app.undo_active().unwrap();
        assert_eq!(
            app.session.active_doc().unwrap().plane(),
            &PixelPlane::Gray8(vec![1, 2, 3, 4])
        );
        for choice in [
            StackProcessChoice::Cancel,
            StackProcessChoice::Current,
            StackProcessChoice::All,
        ] {
            let mut app = fixture(true);
            app.perform(AppAction::MathSquare, &ctx);
            assert!(app.pending_stack_process.is_some());
            app.resolve_stack_process(choice).unwrap();
            let expected = PixelPlane::Gray8(vec![1, 4, 9, 16]);
            let source = PixelPlane::Gray8(vec![1, 2, 3, 4]);
            let doc = app.session.active_doc().unwrap();
            for (index, plane) in doc.slices.iter().enumerate() {
                assert_eq!(
                    plane,
                    if choice == StackProcessChoice::All
                        || (choice == StackProcessChoice::Current && index == 0)
                    {
                        &expected
                    } else {
                        &source
                    }
                );
            }
        }
    }

    #[test]
    fn math_menu_has_all_operations_and_requires_an_image() {
        let mut app = ImageJApp::new();
        for action in [
            AppAction::MathAdd,
            AppAction::MathSubtract,
            AppAction::MathMultiply,
            AppAction::MathDivide,
            AppAction::MathMin,
            AppAction::MathMax,
            AppAction::MathGamma,
            AppAction::MathLog,
            AppAction::MathExp,
            AppAction::MathSquare,
            AppAction::MathSquareRoot,
            AppAction::MathReciprocal,
            AppAction::MathAbs,
            AppAction::MathSet,
        ] {
            assert!(app.action_enabled(action).is_err());
        }
        app.session
            .insert_doc(ImageDocument::new_ramp_8bit("math", 2, 2).unwrap());
        assert!(app.action_enabled(AppAction::MathAdd).is_ok());
    }
}

impl ImageJApp {
    fn render_math_content(&mut self, ui: &mut egui::Ui) {
        let Some(dialog) = self.dialogs.math.as_mut() else {
            return;
        };
        let mut value = dialog.operation.value().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Value:");
            ui.add(egui::DragValue::new(&mut value).speed(0.1));
        });
        dialog.operation = dialog.operation.with_value(value);
        if matches!(dialog.operation, ij_core::math::MathOperation::Gamma(_)) {
            ui.label("Gamma must be between 0.05 and 5.0");
        }
        let mut answer = None;
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            answer = Some(true);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            answer = Some(false);
        }
        if let Some(apply) = answer {
            if let Err(error) = self.resolve_math_dialog(apply) {
                self.dialogs.last_error = Some(error.to_string());
            }
        }
    }
}
