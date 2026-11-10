use super::*;

#[path = "bc_palette.rs"]
mod bc_palette;
#[path = "threshold_palette.rs"]
mod threshold_palette;

impl ImageJApp {
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
            self.render_bc_palette(ctx);
        }

        if self.dialogs.show_threshold {
            self.render_threshold_palette(ctx);
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
}
