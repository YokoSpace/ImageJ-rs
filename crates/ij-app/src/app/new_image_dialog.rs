//! `File > New > Image...` dialog, mirroring `NewImage.showDialog`
//! (original_imagej/ij/gui/NewImage.java 393-442) and the recorder format
//! emitted by `Recorder.appendNewImage`
//! (original_imagej/ij/plugin/frame/Recorder.java 726-753).

use super::ImageJApp;
use crate::dialogs::{NEW_IMAGE_FILLS, NEW_IMAGE_TYPES, NewImageDialog};
use ij_session::{Command, CommandOutcome};

const NEW_IMAGE_BUTTON_RECT_ID: &str = "ij_new_image_button_rect";

impl ImageJApp {
    /// Button rectangle helper for headless tests.
    pub fn new_image_button_rect(ctx: &egui::Context, label: &str) -> Option<egui::Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new((NEW_IMAGE_BUTTON_RECT_ID, label))))
    }

    /// Opens the dialog seeded with NewImage's `static*` defaults
    /// (NewImage.java 26-31, 395-403).
    pub fn open_new_image_dialog(&mut self) {
        let defaults = self.dialogs.new_image_defaults.clone();
        self.dialogs.new_image = NewImageDialog {
            open: true,
            name: defaults.name,
            type_index: defaults.type_index.min(NEW_IMAGE_TYPES.len() - 1),
            // staticFillWith is an `options` fill code; getFill maps the
            // legacy codes 5-7 to Black (NewImage.java 106-113).
            fill_index: defaults.fill_index.min(NEW_IMAGE_FILLS.len() - 1),
            width: defaults.width,
            height: defaults.height,
            slices: defaults.slices,
        };
    }

    /// Cancel returns false and creates nothing (NewImage.java 403-405).
    pub fn cancel_new_image_dialog(&mut self) {
        self.dialogs.new_image.open = false;
    }

    /// Applies the dialog as exactly one `Command::NewImage`. The recorded
    /// type string is `"<type> <fill lowercase>"`, matching
    /// `Recorder.appendNewImage` where the "Fill with" choice is appended to
    /// the "Type" choice (Recorder.java 731-734).
    pub fn apply_new_image_dialog(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<Option<ij_session::ImageId>, ij_session::SessionError> {
        if !self.dialogs.new_image.open {
            return Ok(None);
        }
        let dialog = self.dialogs.new_image.clone();
        let parse = |field: &str| -> Option<i64> { field.trim().parse::<f64>().ok().map(|v| v as i64) };
        let (Some(width), Some(height), Some(slices)) = (
            parse(&dialog.width),
            parse(&dialog.height),
            parse(&dialog.slices),
        ) else {
            // GenericDialog marks the numeric field invalid and re-shows the
            // dialog (GenericDialog.getNextNumber + invalidNumber).
            self.dialogs.last_error = Some("Invalid numeric field".into());
            return Ok(None);
        };
        self.dialogs.new_image.open = false;
        if let Some(err) = self.reject_while_closing("New") {
            return Err(err);
        }
        let type_name = format!(
            "{} {}",
            NEW_IMAGE_TYPES[dialog.type_index],
            NEW_IMAGE_FILLS[dialog.fill_index].to_lowercase()
        );
        let outcome = self.session.dispatch(Command::NewImage {
            title: dialog.name.clone(),
            type_name,
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            slices: slices.max(0) as u32,
        });
        match outcome {
            Ok(CommandOutcome::ImageOpened(id)) => {
                // Prefs update only on success outside macro mode
                // (NewImage.java 435-441); the session has no macro mode.
                self.dialogs.new_image_defaults = crate::dialogs::NewImageDefaults {
                    name: dialog.name,
                    type_index: dialog.type_index,
                    fill_index: dialog.fill_index,
                    width: dialog.width,
                    height: dialog.height,
                    slices: dialog.slices,
                };
                if let Some(doc) = self.session.get_image(id) {
                    let (width, height, title) = (doc.width, doc.height, doc.title.clone());
                    self.image_views.insert(id, super::ImageView::new(width, height));
                    self.status_message = format!("Created image \"{title}\"");
                }
                let _ = self.activate_image_window(id, ctx);
                Ok(Some(id))
            }
            Ok(other) => {
                self.status_message = format!("New Image failed: {other:?}");
                Ok(None)
            }
            Err(error) => {
                self.status_message = format!("New Image failed: {error}");
                self.dialogs.last_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    /// Dialog fields in `showDialog` order: Name, Type, Fill with, Width,
    /// Height, Slices (NewImage.java 398-402).
    pub fn render_new_image_content(&mut self, ui: &mut egui::Ui) {
        let mut dialog = self.dialogs.new_image.clone();
        egui::Grid::new("ij_new_image_grid").num_columns(2).show(ui, |ui| {
            ui.label("Name:");
            ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(200.0));
            ui.end_row();
            ui.label("Type:");
            egui::ComboBox::from_id_salt("ij_new_image_type").selected_text(NEW_IMAGE_TYPES[dialog.type_index]).show_ui(ui, |ui| {
                for (index, label) in NEW_IMAGE_TYPES.iter().enumerate() {
                    ui.selectable_value(&mut dialog.type_index, index, *label);
                }
            });
            ui.end_row();
            ui.label("Fill with:");
            egui::ComboBox::from_id_salt("ij_new_image_fill").selected_text(NEW_IMAGE_FILLS[dialog.fill_index]).show_ui(ui, |ui| {
                for (index, label) in NEW_IMAGE_FILLS.iter().enumerate() {
                    ui.selectable_value(&mut dialog.fill_index, index, *label);
                }
            });
            ui.end_row();
            ui.label("Width:");
            ui.add(egui::TextEdit::singleline(&mut dialog.width).desired_width(90.0));
            ui.end_row();
            ui.label("Height:");
            ui.add(egui::TextEdit::singleline(&mut dialog.height).desired_width(90.0));
            ui.end_row();
            ui.label("Slices:");
            ui.add(egui::TextEdit::singleline(&mut dialog.slices).desired_width(90.0));
            ui.end_row();
        });
        ui.separator();
        let mut action = None;
        ui.horizontal(|ui| {
            for label in ["OK", "Cancel"] {
                let response = ui.button(label);
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new((NEW_IMAGE_BUTTON_RECT_ID, label)),
                        response.rect,
                    )
                });
                if response.clicked() {
                    action = Some(label);
                }
            }
        });
        self.dialogs.new_image = dialog;
        match action {
            Some("OK") => {
                let _ = self.apply_new_image_dialog(ui.ctx());
            }
            Some(_) => self.cancel_new_image_dialog(),
            None => {}
        }
    }

    pub(crate) fn render_new_image_dialog(&mut self, ctx: &egui::Context) {
        let mut cancel = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_new_image_dialog"),
            egui::ViewportBuilder::default()
                .with_title("New Image...")
                .with_inner_size([380.0, 260.0])
                .with_resizable(false),
            |ctx, class| {
                let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                if (!is_embedded && ctx.input(|i| i.viewport().close_requested()))
                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    cancel = true;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.render_new_image_content(ui);
                });
            },
        );
        if cancel {
            self.cancel_new_image_dialog();
        }
    }
}
