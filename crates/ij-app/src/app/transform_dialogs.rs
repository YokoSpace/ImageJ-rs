use super::*;
use crate::dialogs::{RotateDialog, SizeDialog};
use ij_core::transform::Interpolation;

impl ImageJApp {
    pub fn open_rotate_dialog(&mut self) {
        let Some(source) = self.active_image_id() else {
            return;
        };
        let Some(doc) = self.session.get_image(source) else {
            return;
        };
        let can_enlarge = doc
            .roi
            .as_ref()
            .filter(|roi| roi.is_area())
            .and_then(|roi| roi.clipped_bounds(doc.width, doc.height).ok())
            .is_none_or(|bounds| bounds == (0, 0, doc.width, doc.height));
        let mut options = self.dialogs.rotate_defaults;
        if !can_enlarge {
            options.enlarge = false;
        }
        self.dialogs.rotate = Some(RotateDialog {
            source,
            options,
            can_enlarge,
        });
    }

    pub fn cancel_rotate_dialog(&mut self) {
        self.dialogs.rotate = None;
    }

    pub fn apply_rotate_dialog(&mut self) -> Result<(), String> {
        let dialog = self
            .dialogs
            .rotate
            .clone()
            .ok_or("Rotate dialog is closed")?;
        if self.active_image_id() != Some(dialog.source) {
            return Err("Activate the source image".into());
        }
        self.session
            .dispatch(Command::Rotate {
                options: dialog.options,
            })
            .map_err(|e| e.to_string())?;
        self.dialogs.rotate_defaults = dialog.options;
        self.dialogs.rotate = None;
        self.status_message = "Rotated image".into();
        Ok(())
    }

    pub fn open_size_dialog(&mut self) {
        let Some(source) = self.active_image_id() else {
            return;
        };
        let Some(doc) = self.session.get_image(source) else {
            return;
        };
        let (rw, rh) = doc
            .roi
            .as_ref()
            .filter(|roi| roi.is_area())
            .and_then(|roi| roi.clipped_bounds(doc.width, doc.height).ok())
            .map(|(_, _, w, h)| (w, h))
            .unwrap_or((doc.width, doc.height));
        let mut dialog = self.dialogs.size_defaults.clone().unwrap_or(SizeDialog {
            source,
            width: (rw / 2).max(1),
            height: (rh / 2).max(1),
            depth: None,
            constrain: true,
            average_when_downsizing: true,
            interpolation: Interpolation::Bilinear,
        });
        dialog.source = source;
        if doc.stack_size() > 1 {
            dialog.width = rw;
            dialog.height = rh;
            dialog.depth = Some(doc.stack_size() as u32);
        } else {
            dialog.depth = None;
        }
        self.dialogs.size = Some(dialog);
    }

    pub fn cancel_size_dialog(&mut self) {
        self.dialogs.size = None;
    }

    pub fn apply_size_dialog(&mut self) -> Result<(), String> {
        let dialog = self.dialogs.size.clone().ok_or("Size dialog is closed")?;
        if self.active_image_id() != Some(dialog.source) {
            return Err("Activate the source image".into());
        }
        self.session
            .dispatch(Command::Size {
                width: dialog.width,
                height: dialog.height,
                depth: dialog.depth,
                constrain: dialog.constrain,
                average_when_downsizing: dialog.average_when_downsizing,
                interpolation: dialog.interpolation,
            })
            .map_err(|e| e.to_string())?;
        self.dialogs.size_defaults = Some(dialog);
        self.dialogs.size = None;
        self.status_message = "Resized image".into();
        Ok(())
    }

    pub(super) fn render_transform_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut dialog) = self.dialogs.rotate.clone() {
            let mut apply = false;
            let mut cancel = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_rotate_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("Rotate")
                    .with_inner_size([340.0, 235.0])
                    .with_resizable(false),
                |ctx, class| {
                    if !matches!(class, egui::ViewportClass::Embedded)
                        && ctx.input(|i| i.viewport().close_requested())
                    {
                        cancel = true;
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Angle:");
                            ui.add(egui::DragValue::new(&mut dialog.options.angle).speed(0.1));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Grid lines:");
                            ui.add(egui::DragValue::new(&mut dialog.options.grid_lines));
                        });
                        interpolation_choice(ui, &mut dialog.options.interpolation);
                        ui.checkbox(
                            &mut dialog.options.fill_with_background,
                            "Fill with background color",
                        );
                        if dialog.can_enlarge {
                            ui.checkbox(&mut dialog.options.enlarge, "Enlarge image");
                        }
                        if self
                            .session
                            .get_image(dialog.source)
                            .is_some_and(|doc| doc.stack_size() > 1)
                        {
                            ui.checkbox(&mut dialog.options.whole_stack, "Process stack");
                        }
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
            self.dialogs.rotate = Some(dialog);
            if cancel {
                self.cancel_rotate_dialog();
            } else if apply {
                if let Err(e) = self.apply_rotate_dialog() {
                    self.dialogs.last_error = Some(e);
                }
            }
        }
        if let Some(mut dialog) = self.dialogs.size.clone() {
            let mut apply = false;
            let mut cancel = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("ij_size_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("Resize")
                    .with_inner_size([340.0, 255.0])
                    .with_resizable(false),
                |ctx, class| {
                    if !matches!(class, egui::ViewportClass::Embedded)
                        && ctx.input(|i| i.viewport().close_requested())
                    {
                        cancel = true;
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Width (pixels):");
                            ui.add(egui::DragValue::new(&mut dialog.width));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Height (pixels):");
                            ui.add(egui::DragValue::new(&mut dialog.height));
                        });
                        if let Some(depth) = dialog.depth.as_mut() {
                            ui.horizontal(|ui| {
                                ui.label("Depth (images):");
                                ui.add(egui::DragValue::new(depth));
                            });
                        }
                        ui.checkbox(&mut dialog.constrain, "Constrain aspect ratio");
                        ui.checkbox(
                            &mut dialog.average_when_downsizing,
                            "Average when downsizing",
                        );
                        interpolation_choice(ui, &mut dialog.interpolation);
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
            self.dialogs.size = Some(dialog);
            if cancel {
                self.cancel_size_dialog();
            } else if apply {
                if let Err(e) = self.apply_size_dialog() {
                    self.dialogs.last_error = Some(e);
                }
            }
        }
    }
}

fn interpolation_choice(ui: &mut egui::Ui, method: &mut Interpolation) {
    egui::ComboBox::from_label("Interpolation:")
        .selected_text(method.java_name())
        .show_ui(ui, |ui| {
            for value in [
                Interpolation::None,
                Interpolation::Bilinear,
                Interpolation::Bicubic,
            ] {
                ui.selectable_value(method, value, value.java_name());
            }
        });
}
