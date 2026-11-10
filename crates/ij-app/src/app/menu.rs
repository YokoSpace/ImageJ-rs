use super::*;

impl ImageJApp {
    pub(super) fn render_menu_bar(&mut self, ui: &mut egui::Ui) {
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
