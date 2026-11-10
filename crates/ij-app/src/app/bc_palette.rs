use super::*;
use ij_core::{
    auto_contrast_range, default_contrast_range, display_histogram, format_ij_num,
    slider_range_for, ContrastState, DisplayRange,
};

#[derive(Clone, Default)]
struct PaletteState {
    image_id: Option<ImageId>,
    auto_threshold: u32,
    slider_range: i32,
    preview_active: bool,
}

const PALETTE_STATE: &str = "ij_bc_palette_state";
const MAX_SLIDER_RECT: &str = "ij_bc_max_slider_rect";

impl ImageJApp {
    pub fn bc_button_rect(ctx: &egui::Context, label: &str) -> Option<egui::Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new((PALETTE_STATE, label))))
    }

    pub fn bc_max_slider_rect(ctx: &egui::Context) -> Option<egui::Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new(MAX_SLIDER_RECT)))
    }

    pub(super) fn render_bc_palette(&mut self, ctx: &egui::Context) {
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_bc_dialog"),
            egui::ViewportBuilder::default()
                .with_title("B&C")
                .with_inner_size([300.0, 330.0])
                .with_resizable(false),
            |ctx, class| {
                let embedded = matches!(class, egui::ViewportClass::Embedded);
                if (!embedded && ctx.input(|input| input.viewport().close_requested()))
                    || ctx.input(|input| input.key_pressed(egui::Key::Escape))
                {
                    close = true;
                }
                egui::CentralPanel::default().show(ctx, |ui| self.render_bc_palette_content(ui));
            },
        );
        if close {
            let palette_state =
                ctx.data(|data| data.get_temp::<PaletteState>(egui::Id::new(PALETTE_STATE)));
            if palette_state.is_some_and(|state| state.preview_active) {
                if let Ok(doc) = self.session.active_doc() {
                    let range = doc.display_range;
                    let _ = self.session.dispatch(Command::SetDisplayRange {
                        min: range.min,
                        max: range.max,
                    });
                }
            }
            self.dialogs.show_brightness_contrast = false;
            ctx.data_mut(|data| data.remove::<PaletteState>(egui::Id::new(PALETTE_STATE)));
        }
    }

    /// The same content is used by the native palette and headless egui tests.
    pub fn render_bc_palette_content(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.active_image_id() else {
            ui.label("No image");
            return;
        };
        let Some(doc) = self.session.get_image(id) else {
            return;
        };
        if matches!(doc.plane(), PixelPlane::ColorRgb(_)) {
            ui.colored_label(
                Color32::from_rgb(220, 80, 80),
                "B&C is not supported on RGB images.",
            );
            return;
        }
        let is_8bit = matches!(
            doc.plane(),
            PixelPlane::Gray8(_) | PixelPlane::Color256 { .. }
        );
        let default_range = default_contrast_range(doc);
        let defaults = (default_range.min, default_range.max);
        let hist = display_histogram(doc);
        let mut palette_state = ui.ctx().data(|data| {
            data.get_temp::<PaletteState>(egui::Id::new(PALETTE_STATE))
                .unwrap_or_default()
        });
        if palette_state.image_id != Some(id) {
            palette_state.image_id = Some(id);
            palette_state.auto_threshold = 0;
            palette_state.slider_range = slider_range_for(
                defaults.0,
                defaults.1,
                doc.display_range.min,
                doc.display_range.max,
            );
            palette_state.preview_active = false;
            self.dialogs.bc_min = doc.display_range.min;
            self.dialogs.bc_max = doc.display_range.max;
        }
        let mut contrast = ContrastState::new(
            defaults.0,
            defaults.1,
            doc.display_range.min,
            doc.display_range.max,
        )
        .with_slider_range(palette_state.slider_range);

        draw_histogram(ui, &hist.counts, contrast.min, contrast.max, defaults);
        ui.label(format!(
            "Min: {}    Max: {}",
            format_ij_num(contrast.min),
            format_ij_num(contrast.max)
        ));

        let mut preview = None;
        let mut commit = false;
        let positions = contrast.slider_positions();
        let slider_max = contrast.slider_range as i32 - 1;
        for (index, label) in ["Minimum", "Maximum", "Brightness", "Contrast"]
            .iter()
            .enumerate()
        {
            let mut value = positions[index].clamp(0, slider_max);
            let response = ui.add(egui::Slider::new(&mut value, 0..=slider_max).text(*label));
            if index == 1 {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new(MAX_SLIDER_RECT), response.rect)
                });
            }
            if response.changed() {
                match index {
                    0 => contrast.adjust_min(value as f64),
                    1 => contrast.adjust_max(value as f64),
                    2 => contrast.adjust_brightness(value as f64),
                    _ => contrast.adjust_contrast(value as f64),
                }
                preview = Some((contrast.min, contrast.max));
                self.dialogs.bc_min = contrast.min;
                self.dialogs.bc_max = contrast.max;
                palette_state.preview_active = true;
                if !response.dragged() {
                    commit = true;
                }
            }
            commit |= response.drag_stopped();
        }

        ui.horizontal(|ui| {
            ui.label("Minimum:");
            ui.add(egui::DragValue::new(&mut self.dialogs.bc_min).speed(1.0));
            ui.label("Maximum:");
            ui.add(egui::DragValue::new(&mut self.dialogs.bc_max).speed(1.0));
        });
        let mut action = None;
        ui.horizontal(|ui| {
            for label in ["Auto", "Reset", "Set", "Apply"] {
                let response =
                    ui.add_enabled(label != "Apply" || is_8bit, egui::Button::new(label));
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new((PALETTE_STATE, label)), response.rect)
                });
                if response.clicked() {
                    action = Some(label);
                }
            }
        });

        let auto_result = if action == Some("Auto") {
            Some(auto_contrast_range(doc, palette_state.auto_threshold))
        } else {
            None
        };
        if let Some((min, max)) = preview {
            let _ = self
                .session
                .dispatch(Command::PreviewDisplayRange { min, max });
        }
        if commit {
            let range = preview.unwrap_or((contrast.min, contrast.max));
            let _ = self.session.dispatch(Command::SetDisplayRange {
                min: range.0,
                max: range.1,
            });
            palette_state.preview_active = false;
        }
        match action {
            Some("Auto") => {
                let (range, threshold) = auto_result.unwrap();
                palette_state.auto_threshold = threshold;
                if let Some(range) = range {
                    self.dialogs.bc_min = range.min;
                    self.dialogs.bc_max = range.max;
                }
                let command = match range {
                    Some(DisplayRange { min, max }) => Command::AutoDisplayRange { min, max },
                    None => Command::ResetDisplayRange {
                        min: defaults.0,
                        max: defaults.1,
                    },
                };
                let _ = self.session.dispatch(command);
            }
            Some("Reset") => {
                palette_state.auto_threshold = 0;
                self.dialogs.bc_min = defaults.0;
                self.dialogs.bc_max = defaults.1;
                let _ = self.session.dispatch(Command::ResetDisplayRange {
                    min: defaults.0,
                    max: defaults.1,
                });
            }
            Some("Set") => {
                let _ = self.session.dispatch(Command::SetDisplayRange {
                    min: self.dialogs.bc_min,
                    max: self.dialogs.bc_max,
                });
            }
            Some("Apply") => {
                let _ = self.session.dispatch(Command::ApplyDisplayLut);
                palette_state.auto_threshold = 0;
                self.dialogs.bc_min = 0.0;
                self.dialogs.bc_max = 255.0;
            }
            _ => {}
        }
        if action.is_some() {
            palette_state.preview_active = false;
        }
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new(PALETTE_STATE), palette_state));
    }
}

fn draw_histogram(
    ui: &mut egui::Ui,
    counts: &[u64; 256],
    min: f64,
    max: f64,
    defaults: (f64, f64),
) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(128.0, 64.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let peak = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
    for (index, &count) in counts.iter().enumerate() {
        let x = rect.left() + (index as f32 * 0.5).floor();
        let y = rect.bottom() - (count as f32 / peak * 64.0);
        painter.line_segment(
            [egui::pos2(x, rect.bottom()), egui::pos2(x, y)],
            Stroke::new(1.0_f32, Color32::from_rgb(110, 110, 150)),
        );
    }
    let span = defaults.1 - defaults.0;
    if span > 0.0 {
        let x1 = rect.left() + (((min - defaults.0) / span) as f32 * 128.0).clamp(0.0, 128.0);
        let x2 = rect.left() + (((max - defaults.0) / span) as f32 * 128.0).clamp(0.0, 128.0);
        painter.line_segment(
            [egui::pos2(x1, rect.bottom()), egui::pos2(x2, rect.top())],
            Stroke::new(1.0_f32, Color32::BLACK),
        );
    }
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, Color32::BLACK),
        egui::StrokeKind::Inside,
    );
}
