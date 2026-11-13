use super::*;
use crate::dialogs::ZProjectDialog;
use ij_core::zproject::{
    Z_PROJECT_INVERTED_LUT_MESSAGE, Z_PROJECT_METHODS, Z_PROJECT_STACK_REQUIRED,
};

impl ImageJApp {
    /// `Image > Stacks > Z Project...` (ZProjector.run2,
    /// original_imagej/ij/plugin/ZProjector.java 140-155): a single-slice
    /// image errors before the dialog, an inverting LUT warns via
    /// `IJ.showMessageWithCancel`, then the dialog opens.
    pub fn open_zproject_dialog(&mut self) {
        let Some(source) = self.active_image_id() else {
            return;
        };
        let Some(doc) = self.session.get_image(source) else {
            return;
        };
        if doc.stack_size() == 1 {
            // IJ.error("Z Project", "Stack required") (ZProjector.java
            // 144-147).
            self.dialogs.last_error = Some(Z_PROJECT_STACK_REQUIRED.into());
            return;
        }
        if crate::image_view::is_inverted_lut(doc) {
            // IJ.showMessageWithCancel precedes the dialog
            // (ZProjector.java 149-153).
            self.dialogs.zproject_lut_pending = Some(source);
            return;
        }
        self.show_zproject_dialog(source);
    }

    /// Answer to the inverting-LUT `IJ.showMessageWithCancel`: OK continues
    /// into the projection dialog, Cancel aborts the command
    /// (ZProjector.java 150-153).
    pub fn resolve_zproject_lut_warning(&mut self, proceed: bool) {
        let source = self.dialogs.zproject_lut_pending.take();
        if proceed {
            if let Some(source) = source {
                self.show_zproject_dialog(source);
            }
        }
    }

    fn show_zproject_dialog(&mut self, source: ImageId) {
        let Some(doc) = self.session.get_image(source) else {
            return;
        };
        self.dialogs.zproject = ZProjectDialog {
            open: true,
            source: Some(source),
            // setDefaultBounds resets the slice range on every run
            // (ZProjector.java 202-213, 221-229).
            start: "1".into(),
            stop: doc.stack_size().to_string(),
            method_index: self
                .dialogs
                .zproject_method
                .min(Z_PROJECT_METHODS.len() - 1),
        };
    }

    /// Cancel returns without projecting (ZProjector.java 160-162).
    pub fn cancel_zproject_dialog(&mut self) {
        self.dialogs.zproject.open = false;
    }

    /// OK on the ZProjection GenericDialog: parses the numeric fields, runs
    /// `Command::ZProject`, and stores the chosen method like
    /// `Prefs.set(METHOD_KEY, method)` (ZProjector.java 164-177).
    pub fn apply_zproject_dialog(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<Option<ImageId>, String> {
        if !self.dialogs.zproject.open {
            return Ok(None);
        }
        let dialog = self.dialogs.zproject.clone();
        if dialog.source.is_none() || self.active_image_id() != dialog.source {
            // The projection always targets the image the dialog opened on.
            self.dialogs.last_error = Some("Activate the source image".into());
            return Ok(None);
        }
        self.dialogs.zproject.open = false;
        if let Some(err) = self.reject_while_closing("Z Project") {
            return Err(err.to_string());
        }
        let method = Z_PROJECT_METHODS[dialog.method_index.min(Z_PROJECT_METHODS.len() - 1)];
        // Prefs.set precedes doProjection, even when projection fails.
        self.dialogs.zproject_method = dialog.method_index.min(Z_PROJECT_METHODS.len() - 1);
        match self.session.dispatch(Command::ZProjectFromDialog {
            method,
            start: dialog.start,
            stop: dialog.stop,
        }) {
            Ok(CommandOutcome::ImageOpened(id)) => {
                if let Some(doc) = self.session.get_image(id) {
                    let (width, height, title) = (doc.width, doc.height, doc.title.clone());
                    self.image_views.insert(id, ImageView::new(width, height));
                    self.status_message = format!("Projected \"{title}\"");
                }
                let _ = self.activate_image_window(id, ctx);
                Ok(Some(id))
            }
            Ok(other) => {
                self.status_message = format!("Z Project failed: {other:?}");
                Ok(None)
            }
            Err(error) => {
                self.status_message = format!("Z Project failed: {error}");
                self.dialogs.last_error = Some(format!("Z Project failed: {error}"));
                Err(error.to_string())
            }
        }
    }

    /// Content shared by the immediate viewport and headless egui tests, in
    /// `buildControlDialog` field order (ZProjector.java 293-301).
    pub fn render_zproject_content(&mut self, ui: &mut egui::Ui) {
        let mut dialog = self.dialogs.zproject.clone();
        ui.horizontal(|ui| {
            ui.label("Start slice:");
            ui.add(egui::TextEdit::singleline(&mut dialog.start).desired_width(80.0));
        });
        ui.horizontal(|ui| {
            ui.label("Stop slice:");
            ui.add(egui::TextEdit::singleline(&mut dialog.stop).desired_width(80.0));
        });
        egui::ComboBox::from_label("Projection type")
            .selected_text(
                Z_PROJECT_METHODS[dialog.method_index.min(Z_PROJECT_METHODS.len() - 1)].label(),
            )
            .show_ui(ui, |ui| {
                for (index, method) in Z_PROJECT_METHODS.iter().enumerate() {
                    ui.selectable_value(&mut dialog.method_index, index, method.label());
                }
            });
        self.dialogs.zproject = dialog;
    }

    pub(super) fn render_zproject_dialog(&mut self, ctx: &egui::Context) {
        if self.dialogs.zproject_lut_pending.is_some() {
            // GenericDialog title for the LUT warning is "ZProjection"
            // (IJ.showMessageWithCancel call at ZProjector.java 150-151).
            let mut resolved = None;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_zproject_lut_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("ZProjection")
                    .with_inner_size([420.0, 140.0])
                    .with_resizable(false),
                |ctx, class| {
                    if !matches!(class, egui::ViewportClass::Embedded)
                        && ctx.input(|i| i.viewport().close_requested())
                    {
                        resolved = Some(false);
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.label(Z_PROJECT_INVERTED_LUT_MESSAGE);
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                resolved = Some(true);
                            }
                            if ui.button("Cancel").clicked() {
                                resolved = Some(false);
                            }
                        });
                    });
                },
            );
            if let Some(proceed) = resolved {
                self.resolve_zproject_lut_warning(proceed);
            }
        }
        if self.dialogs.zproject.open {
            // The Java dialog title is "ZProjection"
            // (ZProjector.java 293).
            let mut apply = false;
            let mut cancel = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_zproject_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("ZProjection")
                    .with_inner_size([280.0, 160.0])
                    .with_resizable(false),
                |ctx, class| {
                    if !matches!(class, egui::ViewportClass::Embedded)
                        && ctx.input(|i| i.viewport().close_requested())
                    {
                        cancel = true;
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        self.render_zproject_content(ui);
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                apply = true;
                            }
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                        });
                    });
                },
            );
            if cancel {
                self.cancel_zproject_dialog();
            } else if apply {
                if let Err(error) = self.apply_zproject_dialog(ctx) {
                    self.dialogs.last_error = Some(error);
                }
            }
        }
    }
}
