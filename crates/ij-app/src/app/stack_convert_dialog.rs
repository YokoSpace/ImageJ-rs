use super::*;
use crate::dialogs::ImagesToStackDialog;
use ij_core::stack_convert::{images_to_stack_plan, ImagesToStackOptions, IMAGES_TO_STACK_METHODS};

impl ImageJApp {
    /// `Image > Stacks > Images to Stack` (ImagesToStack.java 36-116): the
    /// command counts the open single-slice images, errors when fewer than two
    /// exist, then opens the GenericDialog seeded from the persisted statics.
    pub fn open_images_to_stack_dialog(&mut self) {
        let ids = self.session.image_ids();
        if ids.is_empty() {
            // IJ.error("No images are open.") (ImagesToStack.java 55-58).
            self.dialogs.last_error = Some("No images are open.".into());
            return;
        }
        let docs: Vec<&ij_core::ImageDocument> = ids
            .iter()
            .filter_map(|id| self.session.get_image(*id))
            .collect();
        // `filter = null` before the first findMinMaxSize call
        // (ImagesToStack.java 78), so the size message counts every 2D image.
        let options = ImagesToStackOptions {
            method: IMAGES_TO_STACK_METHODS[self
                .dialogs
                .images_to_stack_method
                .min(IMAGES_TO_STACK_METHODS.len() - 1)],
            method_default: IMAGES_TO_STACK_METHODS[self
                .dialogs
                .images_to_stack_method
                .min(IMAGES_TO_STACK_METHODS.len() - 1)],
            name: "Stack".into(),
            title_filter: String::new(),
            fill: String::new(),
            bicubic: self.dialogs.images_to_stack_bicubic,
            titles_as_labels: self.dialogs.images_to_stack_titles_as_labels,
            keep: self.dialogs.images_to_stack_keep,
        };
        let plan = match images_to_stack_plan(&docs, &options) {
            Ok(plan) => plan,
            Err(error) => {
                self.dialogs.last_error = Some(error.to_string());
                return;
            }
        };
        self.dialogs.images_to_stack = ImagesToStackDialog {
            open: true,
            sizes_differ: plan.sizes_differ(),
            count: plan.count(),
            min_width: plan.min_width,
            min_height: plan.min_height,
            // The message reports the largest image *by area*
            // (ImagesToStack.java 286-290), not the widest and tallest
            // independently; `plan.width`/`plan.height` are the copy target.
            max_width: plan.max_width,
            max_height: plan.max_height,
            method_index: self
                .dialogs
                .images_to_stack_method
                .min(IMAGES_TO_STACK_METHODS.len() - 1),
            // `name`, `filter` and `fillColor` are instance fields reset on
            // every run (ImagesToStack.java 34, 78, 84-86).
            name: "Stack".into(),
            title_filter: String::new(),
            fill: String::new(),
            bicubic: self.dialogs.images_to_stack_bicubic,
            titles_as_labels: self.dialogs.images_to_stack_titles_as_labels,
            keep: self.dialogs.images_to_stack_keep,
        };
    }

    /// `if (gd.wasCanceled()) return;` (ImagesToStack.java 96).
    pub fn cancel_images_to_stack_dialog(&mut self) {
        self.dialogs.images_to_stack.open = false;
    }

    /// OK on the Images to Stack GenericDialog (ImagesToStack.java 97-116):
    /// stores the static method/bicubic/keep/titles-as-labels preferences, then
    /// converts every open 2D image.
    pub fn apply_images_to_stack_dialog(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<Option<ImageId>, String> {
        if !self.dialogs.images_to_stack.open {
            return Ok(None);
        }
        let dialog = self.dialogs.images_to_stack.clone();
        self.dialogs.images_to_stack.open = false;
        if let Some(err) = self.reject_while_closing("Images to Stack") {
            return Err(err.to_string());
        }
        let method_index = dialog.method_index.min(IMAGES_TO_STACK_METHODS.len() - 1);
        // The statics are assigned before the conversion runs
        // (ImagesToStack.java 98-105); the previous value is the choice's
        // initial index, which decides whether the recorder writes it.
        let method_default = IMAGES_TO_STACK_METHODS[self
            .dialogs
            .images_to_stack_method
            .min(IMAGES_TO_STACK_METHODS.len() - 1)];
        // `method` is an instance field initialised to COPY_CENTER
        // (ImagesToStack.java 21) and only read from the dialog when the sizes
        // differ (107), so an equal-size run stores COPY_CENTER back into
        // `staticMethod` (124-129).
        let method = if dialog.sizes_differ {
            IMAGES_TO_STACK_METHODS[method_index]
        } else {
            ij_core::stack_convert::ImagesToStackMethod::CopyCenter
        };
        self.dialogs.images_to_stack_method = IMAGES_TO_STACK_METHODS
            .iter()
            .position(|candidate| *candidate == method)
            .unwrap_or(0);
        // The hidden checkbox is never read on an equal-size run, leaving
        // the instance default false (ImagesToStack.java 22, 113, 126).
        let bicubic = dialog.sizes_differ && dialog.bicubic;
        self.dialogs.images_to_stack_bicubic = bicubic;
        self.dialogs.images_to_stack_titles_as_labels = dialog.titles_as_labels;
        self.dialogs.images_to_stack_keep = dialog.keep;
        let options = ImagesToStackOptions {
            method,
            method_default,
            name: dialog.name,
            title_filter: dialog.title_filter,
            fill: dialog.fill,
            bicubic,
            titles_as_labels: dialog.titles_as_labels,
            keep: dialog.keep,
        };
        match self.session.dispatch(Command::ImagesToStack { options }) {
            Ok(CommandOutcome::ImageOpened(id)) => {
                self.prune_closed_image_views();
                if let Some(doc) = self.session.get_image(id) {
                    let (width, height, title) = (doc.width, doc.height, doc.title.clone());
                    self.image_views.insert(id, ImageView::new(width, height));
                    self.status_message = format!("Created stack \"{title}\"");
                }
                let _ = self.activate_image_window(id, ctx);
                Ok(Some(id))
            }
            Ok(other) => {
                self.status_message = format!("Images to Stack failed: {other:?}");
                Ok(None)
            }
            Err(error) => {
                self.status_message = format!("Images to Stack failed: {error}");
                self.dialogs.last_error = Some(format!("Images to Stack failed: {error}"));
                Err(error.to_string())
            }
        }
    }

    /// Content shared by the immediate viewport and headless egui tests, in
    /// `convertImagesToStack` field order (ImagesToStack.java 87-95).
    pub fn render_images_to_stack_content(&mut self, ui: &mut egui::Ui) {
        let mut dialog = self.dialogs.images_to_stack.clone();
        if dialog.sizes_differ {
            ui.label(images_to_stack_size_message(&dialog));
            egui::ComboBox::from_label("Method:")
                .selected_text(
                    IMAGES_TO_STACK_METHODS
                        [dialog.method_index.min(IMAGES_TO_STACK_METHODS.len() - 1)]
                    .label(),
                )
                .show_ui(ui, |ui| {
                    for (index, method) in IMAGES_TO_STACK_METHODS.iter().enumerate() {
                        ui.selectable_value(&mut dialog.method_index, index, method.label());
                    }
                });
        }
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(egui::TextEdit::singleline(&mut dialog.name).desired_width(120.0));
        });
        ui.horizontal(|ui| {
            ui.label("Title contains:");
            ui.add(egui::TextEdit::singleline(&mut dialog.title_filter).desired_width(120.0));
        });
        ui.horizontal(|ui| {
            ui.label("Fill color:");
            ui.add(egui::TextEdit::singleline(&mut dialog.fill).desired_width(120.0));
        });
        if dialog.sizes_differ {
            ui.checkbox(&mut dialog.bicubic, "Bicubic interpolation");
        }
        ui.checkbox(&mut dialog.titles_as_labels, "Use titles as labels");
        ui.checkbox(&mut dialog.keep, "Keep source images");
        self.dialogs.images_to_stack = dialog;
    }

    pub(super) fn render_images_to_stack_dialog(&mut self, ctx: &egui::Context) {
        if !self.dialogs.images_to_stack.open {
            return;
        }
        // The Java dialog title is "Images to Stack"
        // (ImagesToStack.java 88).
        let mut apply = false;
        let mut cancel = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_images_to_stack_dialog"),
            egui::ViewportBuilder::default()
                .with_title("Images to Stack")
                .with_inner_size([360.0, 240.0])
                .with_resizable(false),
            |ctx, class| {
                if !matches!(class, egui::ViewportClass::Embedded)
                    && ctx.input(|i| i.viewport().close_requested())
                {
                    cancel = true;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.render_images_to_stack_content(ui);
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
            self.cancel_images_to_stack_dialog();
        } else if apply {
            if let Err(error) = self.apply_images_to_stack_dialog(ctx) {
                self.dialogs.last_error = Some(error);
            }
        }
    }

    /// `Image > Stacks > Stack to Images`
    /// (`StackEditor.convertStackToImages`, StackEditor.java 240-310). Stacks
    /// with more than 30 slices ask for confirmation first
    /// (StackEditor.java 254-262).
    pub fn run_stack_to_images(&mut self, ctx: &egui::Context) {
        let Some(id) = self.active_image_id() else {
            return;
        };
        let Some(doc) = self.session.get_image(id) else {
            return;
        };
        let size = doc.stack_size();
        if size < 2 {
            self.dialogs.last_error = Some(format!(
                "\"Convert Stack to Images\" requires a stack\n{}",
                doc.title
            ));
            return;
        }
        if size > 30 {
            self.dialogs.stack_to_images_pending = Some(id);
            return;
        }
        self.apply_stack_to_images(ctx);
    }

    /// Answer to the `IJ.showMessageWithCancel("Convert to Images?", ...)`
    /// prompt (StackEditor.java 255-262).
    pub fn resolve_stack_to_images_confirmation(&mut self, proceed: bool, ctx: &egui::Context) {
        let pending = self.dialogs.stack_to_images_pending.take();
        if proceed && pending.is_some() {
            self.apply_stack_to_images(ctx);
        }
    }

    fn apply_stack_to_images(&mut self, ctx: &egui::Context) {
        if let Some(err) = self.reject_while_closing("Stack to Images") {
            self.dialogs.last_error = Some(err.to_string());
            return;
        }
        match self.session.dispatch(Command::StackToImages) {
            Ok(CommandOutcome::ImagesOpened(ids)) => {
                self.prune_closed_image_views();
                for id in &ids {
                    if let Some(doc) = self.session.get_image(*id) {
                        let (width, height) = (doc.width, doc.height);
                        self.image_views.insert(*id, ImageView::new(width, height));
                    }
                }
                // Every slice window is shown in order, so the last one ends up
                // active (StackEditor.java 288-290).
                if let Some(last) = ids.last() {
                    let _ = self.activate_image_window(*last, ctx);
                }
                self.status_message = format!("Converted stack to {} images", ids.len());
            }
            Ok(other) => {
                self.status_message = format!("Stack to Images failed: {other:?}");
            }
            Err(error) => {
                self.status_message = format!("Stack to Images failed: {error}");
                self.dialogs.last_error = Some(format!("Stack to Images failed: {error}"));
            }
        }
    }

    pub(super) fn render_stack_to_images_dialog(&mut self, ctx: &egui::Context) {
        if self.dialogs.stack_to_images_pending.is_none() {
            return;
        }
        let mut resolved = None;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_stack_to_images_dialog"),
            egui::ViewportBuilder::default()
                .with_title("Convert to Images?")
                .with_inner_size([420.0, 140.0])
                .with_resizable(false),
            |ctx, class| {
                if !matches!(class, egui::ViewportClass::Embedded)
                    && ctx.input(|i| i.viewport().close_requested())
                {
                    resolved = Some(false);
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    let size = self
                        .dialogs
                        .stack_to_images_pending
                        .and_then(|id| self.session.get_image(id))
                        .map(|doc| doc.stack_size())
                        .unwrap_or(0);
                    ui.label(format!(
                        "Are you sure you want to convert this\nstack to {size} separate windows?"
                    ));
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
            self.resolve_stack_to_images_confirmation(proceed, ctx);
        }
    }

    /// Drops view state for images the session no longer holds. The conversion
    /// commands close their sources inside `Session::dispatch`, so the app has
    /// to reconcile `image_views`/`texture_cache` afterwards.
    pub(super) fn prune_closed_image_views(&mut self) {
        let open: std::collections::BTreeSet<ImageId> =
            self.session.image_ids().into_iter().collect();
        self.image_views.retain(|id, _| open.contains(id));
        self.texture_cache.retain(|id, _| open.contains(id));
    }
}

/// The `sizesDiffer` message (ImagesToStack.java 90-91). `max_width` and
/// `max_height` come from the largest image *by area*, not from the widest and
/// tallest images independently, so 4x3 + 5x2 reports `smallest=5x2,
/// largest=4x3` even though the copy target is 5x3.
pub fn images_to_stack_size_message(dialog: &ImagesToStackDialog) -> String {
    format!(
        "The {} images differ in size (smallest={}x{},\nlargest={}x{}). They will be converted\nto a stack using the specified method.",
        dialog.count, dialog.min_width, dialog.min_height, dialog.max_width, dialog.max_height
    )
}
