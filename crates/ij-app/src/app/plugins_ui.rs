use super::*;
use ij_bridge::plugin::{PluginDialogFieldKind, PluginDialogSchema};

pub struct PluginParameterDialog {
    pub schema: PluginDialogSchema,
    pub values: Vec<String>,
    pub error: Option<String>,
    prepared: ij_session::PreparedPluginFilter,
}

impl PluginParameterDialog {
    pub(super) fn new(
        schema: PluginDialogSchema,
        prepared: ij_session::PreparedPluginFilter,
    ) -> Self {
        let values = schema.default_values();
        Self {
            schema,
            values,
            error: None,
            prepared,
        }
    }

    pub fn target(&self) -> ImageId {
        self.prepared.ticket.target()
    }
}

impl ImageJApp {
    pub fn submit_plugin_parameters(&mut self) -> Result<(), String> {
        let state = self
            .plugin_parameters
            .as_mut()
            .ok_or("No plugin parameter dialog is open")?;
        let options = match state.schema.macro_options(&state.values) {
            Ok(options) => options,
            Err(error) => {
                state.error = Some(error.clone());
                return Err(error);
            }
        };
        let state = self.plugin_parameters.take().expect("parameter dialog");
        self.start_prepared_plugin_filter(state.prepared.with_macro_options(options))
            .map_err(|error| {
                self.dialogs.last_error = Some(error.to_string());
                error.to_string()
            })
    }

    fn render_plugin_parameters(&mut self, ctx: &egui::Context) {
        let Some(state) = self.plugin_parameters.as_mut() else {
            return;
        };
        let mut accept = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("ij_plugin_parameters")).show(ctx, |ui| {
            ui.heading(&state.schema.title);
            ui.label(format!("Target image ID {}", state.target().0));
            egui::ScrollArea::vertical()
                .max_height(ctx.screen_rect().height() * 0.6)
                .show(ui, |ui| {
                    egui::Grid::new("ij_plugin_parameter_fields").show(ui, |ui| {
                        for (index, field) in state.schema.fields.iter().enumerate() {
                            let label = field.label.replace('_', " ");
                            ui.label(label.trim_end_matches(':'));
                            let value = &mut state.values[index];
                            match &field.kind {
                                PluginDialogFieldKind::Checkbox => {
                                    let mut checked = value == "true";
                                    if ui.checkbox(&mut checked, "").changed() {
                                        *value = checked.to_string();
                                    }
                                }
                                PluginDialogFieldKind::Choice { choices } => {
                                    egui::ComboBox::from_id_salt(("plugin_choice", index))
                                        .selected_text(value.clone())
                                        .show_ui(ui, |ui| {
                                            for choice in choices {
                                                ui.selectable_value(value, choice.clone(), choice);
                                            }
                                        });
                                }
                                PluginDialogFieldKind::Numeric { units, .. } => {
                                    ui.horizontal(|ui| {
                                        ui.text_edit_singleline(value);
                                        if let Some(units) = units {
                                            ui.label(units);
                                        }
                                    });
                                }
                                PluginDialogFieldKind::String => {
                                    ui.text_edit_singleline(value);
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
            if let Some(error) = &state.error {
                ui.colored_label(Color32::RED, error);
            }
            ui.horizontal(|ui| {
                accept = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
            cancel |= ctx.input(|i| i.key_pressed(egui::Key::Escape));
        });
        if cancel {
            self.cancel_plugin_filter();
        } else if accept {
            let _ = self.submit_plugin_parameters();
        }
    }

    /// Independent, scrollable Installed Plugins window.
    ///
    /// The window only performs read-only discovery and explicit inspection.
    /// The panel states plainly that loading/inspecting does not sandbox code
    /// and that running a verified filter still uses the external JVM.
    pub fn render_plugin_browser_window(&mut self, ctx: &egui::Context) {
        self.render_plugin_parameters(ctx);
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
                        ui.label(egui::RichText::new("PlugInFilter target").strong());
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
                                    "Preflight: transferable 8/16/32-bit grayscale or RGB image; plain Z-stacks supported.",
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
}
