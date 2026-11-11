use super::*;
use ij_core::{
    ThresholdLevels, adjust_max_threshold, adjust_min_threshold, display_histogram,
    format_threshold_label, plot_threshold_bins, scale_up_and_set, threshold_to_sliders,
};

const PALETTE_STATE: &str = "ij_threshold_palette_state";
const LOWER_SLIDER_RECT: &str = "ij_threshold_lower_slider_rect";

#[derive(Clone, Default)]
struct PaletteState {
    image_id: Option<ImageId>,
    last_levels: Option<ThresholdLevels>,
    lower_input: f64,
    upper_input: f64,
}

impl ImageJApp {
    pub fn threshold_button_rect(ctx: &egui::Context, label: &str) -> Option<egui::Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new((PALETTE_STATE, label))))
    }

    pub fn threshold_lower_slider_rect(ctx: &egui::Context) -> Option<egui::Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new(LOWER_SLIDER_RECT)))
    }

    pub(super) fn render_threshold_palette(&mut self, ctx: &egui::Context) {
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_threshold_dialog"),
            egui::ViewportBuilder::default()
                .with_title("Threshold")
                .with_inner_size([320.0, 330.0])
                .with_resizable(false),
            |ctx, class| {
                let embedded = matches!(class, egui::ViewportClass::Embedded);
                if (!embedded && ctx.input(|input| input.viewport().close_requested()))
                    || ctx.input(|input| input.key_pressed(egui::Key::Escape))
                {
                    close = true;
                }
                egui::CentralPanel::default()
                    .show(ctx, |ui| self.render_threshold_palette_content(ui));
            },
        );
        if close {
            self.dialogs.show_threshold = false;
            ctx.data_mut(|data| data.remove::<PaletteState>(egui::Id::new(PALETTE_STATE)));
        }
    }

    pub fn render_threshold_palette_content(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.active_image_id() else {
            ui.label("No image");
            return;
        };
        let Some(doc) = self.session.get_image(id) else {
            return;
        };
        let bit_depth = match doc.plane() {
            PixelPlane::Gray8(_) => 8,
            PixelPlane::Gray16(_) => 16,
            PixelPlane::Gray32(_) => 32,
            _ => {
                ui.colored_label(
                    Color32::from_rgb(220, 80, 80),
                    "Requires a grayscale image.",
                );
                return;
            }
        };
        if doc.stack_size() != 1 {
            ui.colored_label(
                Color32::from_rgb(220, 80, 80),
                "Stack threshold is not yet supported.",
            );
            return;
        }
        let range = doc.display_range;
        let levels = doc.threshold;
        let has_threshold = levels.is_some();
        let counts = display_histogram(doc).counts;
        let can_auto = bit_depth != 32;
        if bit_depth != 16 {
            self.dialogs.threshold_16bit = false;
        }

        let mut state = ui.ctx().data(|data| {
            data.get_temp::<PaletteState>(egui::Id::new(PALETTE_STATE))
                .unwrap_or_default()
        });
        let auto_on_open = state.image_id != Some(id) && levels.is_none() && can_auto;
        if state.image_id != Some(id) || state.last_levels != levels {
            state.image_id = Some(id);
            state.last_levels = levels;
            state.lower_input = levels.map_or(0.0, |value| value.lower);
            state.upper_input = levels.map_or(255.0, |value| value.upper);
        }
        let mut sliders = threshold_to_sliders(levels, bit_depth, range.min, range.max);

        let (lower_bin, upper_bin) =
            plot_threshold_bins(levels.is_some(), sliders.min_slider, sliders.max_slider);
        draw_threshold_histogram(ui, &counts, lower_bin, upper_bin);
        ui.label("Red");
        ui.horizontal(|ui| {
            ui.label("Lower:");
            ui.label(levels.map_or(String::new(), |value| {
                format_threshold_label(value.lower, bit_depth == 16)
            }));
            ui.label("Upper:");
            ui.label(levels.map_or(String::new(), |value| {
                format_threshold_label(value.upper, bit_depth == 16)
            }));
        });

        let mut preview = None;
        let mut lower = sliders.min_slider.round() as i32;
        let response = ui.add(egui::Slider::new(&mut lower, 0..=255).text("Lower"));
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new(LOWER_SLIDER_RECT), response.rect));
        if response.changed() {
            let together = ui.input(|input| input.modifiers.shift || input.modifiers.alt);
            sliders = adjust_min_threshold(sliders, lower as f64, together);
            preview = Some(sliders);
        }
        let mut upper = sliders.max_slider.round() as i32;
        if ui
            .add(egui::Slider::new(&mut upper, 0..=255).text("Upper"))
            .changed()
        {
            sliders = adjust_max_threshold(sliders, upper as f64);
            preview = Some(sliders);
        }

        let mut auto_changed = false;
        ui.horizontal(|ui| {
            ui.label("Method:");
            egui::ComboBox::from_id_salt("thresh_method")
                .selected_text(&self.dialogs.threshold_method)
                .show_ui(ui, |ui| {
                    for method in ["IsoData", "Otsu"] {
                        auto_changed |= ui
                            .selectable_value(
                                &mut self.dialogs.threshold_method,
                                method.into(),
                                method,
                            )
                            .changed();
                    }
                    for method in [
                        "Huang",
                        "Intermodes",
                        "Li",
                        "MaxEntropy",
                        "Mean",
                        "MinError",
                        "Minimum",
                        "Moments",
                        "Percentile",
                        "RenyiEntropy",
                        "Shanbhag",
                        "Triangle",
                        "Yen",
                    ] {
                        ui.add_enabled(false, egui::Button::new(method))
                            .on_disabled_hover_text("This threshold method is not implemented yet");
                    }
                });
        });
        auto_changed |= ui
            .checkbox(&mut self.dialogs.threshold_dark_bg, "Dark background")
            .changed();
        auto_changed |= ui
            .add_enabled(
                bit_depth == 16,
                egui::Checkbox::new(&mut self.dialogs.threshold_16bit, "16-bit histogram"),
            )
            .on_disabled_hover_text("Requires a 16-bit image")
            .changed();
        ui.horizontal(|ui| {
            ui.label("Lower:");
            ui.add(egui::DragValue::new(&mut state.lower_input).speed(1.0));
            ui.label("Upper:");
            ui.add(egui::DragValue::new(&mut state.upper_input).speed(1.0));
        });
        let mut action = None;
        ui.horizontal(|ui| {
            for label in ["Auto", "Reset", "Set", "Apply"] {
                let enabled = label != "Auto" || can_auto;
                let response = ui.add_enabled(enabled, egui::Button::new(label));
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new((PALETTE_STATE, label)), response.rect)
                });
                if label == "Auto" && !can_auto {
                    response
                        .on_disabled_hover_text("Auto threshold is not supported on 32-bit images yet");
                } else if response.clicked() {
                    action = Some(label);
                }
            }
        });

        if let Some(sliders) = preview {
            let levels = scale_up_and_set(
                bit_depth,
                range.min,
                range.max,
                sliders.min_slider,
                sliders.max_slider,
            );
            if let Err(error) = self.session.dispatch(Command::PreviewThreshold {
                lower: levels.lower,
                upper: levels.upper,
            }) {
                self.dialogs.last_error = Some(error.to_string());
            }
        }
        match action {
            Some("Reset") => {
                let _ = self.reset_threshold_active();
            }
            Some("Set") => {
                let lower = state.lower_input;
                let upper = state.upper_input;
                if let Err(error) = self
                    .session
                    .dispatch(Command::SetThreshold { lower, upper })
                {
                    self.dialogs.last_error = Some(error.to_string());
                }
            }
            Some("Apply") => {
                // ThresholdAdjuster.apply rejects an image whose threshold is
                // not set (ThresholdAdjuster.java 776-782).
                if !has_threshold {
                    self.dialogs.last_error = Some("Threshold is not set".into());
                } else if let Err(error) =
                    self.convert_to_mask_active(self.dialogs.threshold_dark_bg)
                {
                    self.dialogs.last_error = Some(error.to_string());
                }
            }
            Some("Auto") => {
                let _ = self.auto_threshold_active_with_options(
                    &self.dialogs.threshold_method.clone(),
                    self.dialogs.threshold_dark_bg,
                    self.dialogs.threshold_16bit,
                );
            }
            _ if (auto_changed || auto_on_open) && can_auto => {
                let _ = self.auto_threshold_active_with_options(
                    &self.dialogs.threshold_method.clone(),
                    self.dialogs.threshold_dark_bg,
                    self.dialogs.threshold_16bit,
                );
            }
            _ => {}
        }
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new(PALETTE_STATE), state));
    }
}

fn draw_threshold_histogram(
    ui: &mut egui::Ui,
    counts: &[u64; 256],
    lower: Option<usize>,
    upper: usize,
) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(256.0, 48.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let peak = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
    for (index, &count) in counts.iter().enumerate() {
        let x = rect.left() + index as f32;
        let y = rect.bottom() - count as f32 / peak * rect.height();
        painter.line_segment(
            [egui::pos2(x, rect.bottom()), egui::pos2(x, y)],
            Stroke::new(1.0_f32, Color32::DARK_GRAY),
        );
    }
    if let Some(lower) = lower {
        let left = rect.left() + lower as f32;
        let right = rect.left() + upper as f32;
        painter.rect_stroke(
            Rect::from_min_max(
                egui::pos2(left, rect.top()),
                egui::pos2(right, rect.bottom()),
            ),
            0.0,
            Stroke::new(1.0_f32, Color32::RED),
            egui::StrokeKind::Inside,
        );
    }
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, Color32::BLACK),
        egui::StrokeKind::Inside,
    );
}
