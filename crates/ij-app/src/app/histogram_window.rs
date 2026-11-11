use super::*;
use ij_core::format_ij_num;

#[derive(Debug, Clone)]
pub struct HistogramWindowState {
    pub title: String,
    pub summary: HistogramSummary,
    pub show_list: bool,
}

impl HistogramWindowState {
    pub fn new(image_title: String, summary: HistogramSummary) -> Self {
        Self {
            title: format!("Histogram of {image_title}"),
            summary,
            show_list: false,
        }
    }

    pub fn stats_labels(&self) -> [String; 6] {
        let stats = &self.summary;
        [
            format!("N: {}", stats.count),
            format!("Mean: {}", format_ij_num(stats.mean)),
            format!("StdDev: {}", format_ij_num(stats.std_dev)),
            format!("Min: {}", format_ij_num(stats.min)),
            format!("Max: {}", format_ij_num(stats.max)),
            format!("Mode: {}", format_ij_num(stats.mode)),
        ]
    }

    pub fn bin_tsv(&self) -> String {
        let mut out = String::new();
        for (index, count) in self.summary.counts.iter().enumerate() {
            let value = self.summary.hmin + index as f64 * self.summary.bin_size;
            out.push_str(&format!("{}\t{count}\n", format_ij_num(value)));
        }
        out
    }
}

impl ImageJApp {
    pub fn render_histogram_window(&mut self, ctx: &egui::Context) {
        let Some(window) = self.histogram_window.as_ref().cloned() else {
            return;
        };
        let mut close = false;
        let mut show_list = window.show_list;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_histogram_window"),
            egui::ViewportBuilder::default()
                .with_title(&window.title)
                .with_inner_size([360.0, 340.0])
                .with_min_inner_size([300.0, 280.0]),
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
                        if ui.button("Copy").clicked() {
                            ctx.copy_text(window.bin_tsv());
                        }
                    });
                    ui.separator();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(256.0, 150.0), Sense::hover());
                    let highest = window.summary.counts.iter().copied().max().unwrap_or(0).max(1);
                    let painter = ui.painter();
                    for (index, count) in window.summary.counts.iter().enumerate() {
                        let x = rect.left() + index as f32;
                        let height = (*count as f32 / highest as f32) * rect.height();
                        painter.line_segment(
                            [egui::pos2(x, rect.bottom()), egui::pos2(x, rect.bottom() - height)],
                            Stroke::new(1.0_f32, Color32::BLACK),
                        );
                    }
                    ui.separator();
                    let labels = window.stats_labels();
                    ui.columns(2, |columns| {
                        for label in &labels[..3] {
                            columns[0].label(label);
                        }
                        for label in &labels[3..] {
                            columns[1].label(label);
                        }
                    });
                    if show_list {
                        ui.separator();
                        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                            ui.monospace(window.bin_tsv());
                        });
                    }
                });
            },
        );
        if close {
            self.histogram_window = None;
        } else if let Some(state) = self.histogram_window.as_mut() {
            state.show_list = show_list;
        }
    }
}
