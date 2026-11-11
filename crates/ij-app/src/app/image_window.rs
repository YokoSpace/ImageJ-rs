use super::*;
use crate::image_view::{visible_stack_axes, StackAxis};
use crate::roi_edit::{RoiHit, drag_handle, hit_test, move_by};
use crate::tool::{
    freehand_is_valid, freehand_push, polygon_closes_at_start, polygon_preview_segments,
    polyline_is_valid,
};
use ij_core::{Wand, WandMode};

#[derive(Clone)]
struct RoiDrag {
    original: Roi,
    hit: RoiHit,
    start_pixel: (i32, i32),
}

/// An in-progress Polygon tool selection. ImageJ keeps the ROI in
/// `CONSTRUCTING` state until the user double-clicks or clicks in the start
/// box (PolygonRoi.handleMouseUp 1073-1118); the Rust UI keeps it out of the
/// document until then so the selection stays transient.
#[derive(Clone, Default)]
struct PolygonBuild {
    vertices: Vec<(i32, i32)>,
    cursor: Option<(i32, i32)>,
}

fn polygon_build_id(image_id: ImageId) -> egui::Id {
    egui::Id::new(("ij_polygon_build", image_id.0))
}

fn angle_build_id(image_id: ImageId) -> egui::Id {
    egui::Id::new(("ij_angle_build", image_id.0))
}

/// The integer drag path of an in-progress Freehand selection.
///
/// ImageJ builds the polygon while the mouse is down (`FreehandRoi.grow`) and
/// only commits it on mouse-up (`handleMouseUp` 90-96), so the Rust UI keeps
/// the vertices transient until the drag stops.
#[derive(Clone, Default)]
struct FreehandBuild {
    points: Vec<(i32, i32)>,
}

fn freehand_build_id(image_id: ImageId) -> egui::Id {
    egui::Id::new(("ij_freehand_build", image_id.0))
}

pub fn animation_slider_axis(axes: &[StackAxis]) -> StackAxis {
    if axes.contains(&StackAxis::Frame) {
        StackAxis::Frame
    } else if axes.contains(&StackAxis::Slice) {
        StackAxis::Slice
    } else {
        StackAxis::FlatSlice
    }
}

fn render_play_stop_button(ui: &mut egui::Ui, is_animating: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&resp);
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let color = visuals.text_color();
        if is_animating {
            let square_rect = rect.shrink(4.0);
            ui.painter()
                .rect_filled(square_rect, CornerRadius::ZERO, color);
        } else {
            let pad = 4.0;
            let p1 = Pos2::new(rect.min.x + pad, rect.min.y + pad);
            let p2 = Pos2::new(rect.min.x + pad, rect.max.y - pad);
            let p3 = Pos2::new(rect.max.x - pad, rect.center().y);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![p1, p2, p3],
                color,
                egui::Stroke::NONE,
            ));
        }
    }
    let tip = if is_animating {
        "Stop Animation"
    } else {
        "Start Animation"
    };
    resp.on_hover_text(tip).clicked()
}

impl ImageJApp {
    pub fn image_canvas_rect(ctx: &egui::Context, id: ImageId) -> Option<Rect> {
        ctx.data(|data| data.get_temp(egui::Id::new(("ij_image_canvas", id.0))))
    }

    pub fn update_ui(&mut self, ctx: &egui::Context) {
        let now_secs = ctx.input(|i| i.time);
        self.advance_animations(now_secs);
        if self.is_any_animating() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                1.0 / DEFAULT_ANIMATION_FPS,
            ));
        }

        self.poll_plugin_filter();
        if self.is_plugin_filter_running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let catalog_changed = self.poll_plugin_catalog_job();
        if catalog_changed {
            ctx.request_repaint();
        }
        if self.is_plugin_catalog_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        crate::macos_bridge::install_macos_termination_bridge(Some(ctx));
        crate::macos_bridge::set_dirty_state(self.any_dirty());
        self.dispatch_native_menu_actions(ctx);
        let mut native_spec = self.build_native_menu_spec();
        if self.pending_close.is_some()
            || self.pending_revert.is_some()
            || self.pending_stack_process.is_some()
            || self.dialogs.last_error.is_some()
            || self.is_plugin_filter_running()
        {
            for menu in &mut native_spec {
                menu.enabled = false;
            }
        }
        let native_menu_active =
            crate::macos_bridge::native_menu::sync_menus(&native_spec, ctx);

        // Native OS termination requested (macOS Cmd-Q / App Menu Quit / Dock Quit)
        if crate::macos_bridge::poll_native_quit_requested() {
            if self.quit_app {
                crate::macos_bridge::set_quit_authorized(true);
            } else {
                let can_quit = self.request_quit();
                if !can_quit {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
            }
        }

        // Root window close requested (close route 5)
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.quit_app {
                // Quit already resolved: do not send CancelClose!
            } else {
                let can_quit = self.request_quit();
                if !can_quit {
                    // Deferred: dirty documents require user prompt.
                    // Must send CancelClose so eframe does not exit before user decides.
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                // When can_quit is true, quit is resolved immediately; do NOT send CancelClose!
            }
        }

        // Global keyboard shortcuts for supported actions on root viewport
        self.handle_keyboard_shortcuts(ctx);

        // Main Toolbar Window
        if !native_menu_active {
            egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
                self.render_menu_bar(ui);
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_toolbar_tools(ui);
            ui.separator();
            self.render_status_bar(ui);
        });

        // Independent Image Windows via multi-viewport
        self.prune_texture_cache();
        let image_ids: Vec<ImageId> = self.session.image_ids();
        let mut close_requests = Vec::new();
        let mut focused_id = None;
        for &id in &image_ids {
            let doc_exists = self.session.get_image(id).is_some();
            if !doc_exists {
                continue;
            }

            let title = self
                .session
                .get_image(id)
                .map(|d| d.title.clone())
                .unwrap_or_else(|| format!("Image {}", id.0));

            let zoom = self.image_views.get(&id).map(|v| v.zoom).unwrap_or(1.0);
            let zoom_pct = (zoom * 100.0).round() as i32;

            let viewport_id = egui::ViewportId::from_hash_of(&("ij_image_window", id.0));

            let win_w = self
                .session
                .get_image(id)
                .map(|d| ((d.width as f32 * zoom as f32).max(200.0) + 30.0).min(1200.0))
                .unwrap_or(400.0);
            let win_h = self
                .session
                .get_image(id)
                .map(|d| ((d.height as f32 * zoom as f32).max(100.0) + 80.0).min(900.0))
                .unwrap_or(300.0);

            let mut close_this = false;
            ctx.show_viewport_immediate(
                viewport_id,
                egui::ViewportBuilder::default()
                    .with_title(format!("{title} ({zoom_pct}%)"))
                    .with_inner_size([win_w, win_h]),
                |ctx, class| {
                    let is_embedded = matches!(class, egui::ViewportClass::Embedded);
                    if ctx.input(|i| i.viewport().focused == Some(true)) {
                        focused_id = Some(id);
                    }
                    self.handle_image_shortcuts(ctx, id);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        self.render_image_canvas_content(ui, id);
                    });
                    if !is_embedded && ctx.input(|i| i.viewport().close_requested()) {
                        if self.is_plugin_filter_running_for(id) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                            self.status_message =
                                "Cannot close window while Java PlugInFilter is running; cancel it first".into();
                        } else if self.pending_close.is_some() || self.pending_revert.is_some() {
                            // While a close/quit prompt is open, ignore other
                            // window closes so the prompt's snapshot stays valid.
                            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        } else {
                            if self.is_dirty(id) {
                                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                            }
                            close_this = true;
                        }
                    }
                },
            );

            if close_this {
                close_requests.push(id);
            }
        }

        if let Some(f_id) = focused_id {
            let _ = self.set_active_window(f_id);
        }

        for id in close_requests {
            self.stop_animation(id);
            self.request_close_window(id);
        }

        // Render modal / secondary dialogs
        self.render_dialogs(ctx);

        // Results Table Window if opened
        if self.show_results_window {
            self.render_results_window(ctx);
        }
        if self.show_particle_summary_window {
            self.render_particle_summary_window(ctx);
        }

        if self.quit_app {
            self.cancel_plugin_catalog_job();
            crate::macos_bridge::set_quit_authorized(true);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn render_toolbar_tools(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
            for &tool in Tool::ALL {
                let is_selected = self.active_tool == tool;
                let is_supp = tool.is_supported();

                let text = format!(" {} ", tool.symbol());
                let btn = egui::Button::new(text).min_size(Vec2::new(28.0, 24.0));

                let resp = if is_supp {
                    if is_selected {
                        ui.add(btn.fill(Color32::from_rgb(180, 205, 240)))
                    } else {
                        ui.add(btn)
                    }
                } else {
                    ui.add_enabled(false, btn)
                };

                if is_supp && resp.clicked() {
                    self.active_tool = tool;
                    self.status_message = format!("Selected tool: {}", tool.name());
                }

                if !is_supp {
                    resp.on_hover_text(format!("{} (not yet implemented)", tool.name()));
                } else {
                    resp.on_hover_text(tool.name());
                }
            }
        });
    }

    fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.status_message);
            if let Some(prog) = self.progress {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::ProgressBar::new(prog)
                            .desired_width(120.0)
                            .show_percentage(),
                    );
                });
            }
        });
    }

    pub fn results_grid_cells(&self) -> Vec<Vec<String>> {
        self.session
            .results
            .to_tsv()
            .lines()
            .map(|line| line.split('\t').map(str::to_string).collect())
            .collect()
    }

    pub fn render_results_window(&mut self, ctx: &egui::Context) {
        if !self.show_results_window {
            return;
        }

        let mut is_open = true;
        let mut save_results_path = None;
        let mut clear_results = false;
        let grid_cells = self.results_grid_cells();

        let viewport_id = egui::ViewportId::from_hash_of("ij_results_window");
        ctx.show_viewport_immediate(
            viewport_id,
            egui::ViewportBuilder::default()
                .with_title("Results")
                .with_inner_size([450.0, 250.0])
                .with_min_inner_size([250.0, 150.0])
                .with_resizable(true),
            |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    is_open = false;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Clear").clicked() {
                            clear_results = true;
                        }
                        if ui.button("Save As...").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("TSV Table", &["tsv", "txt"])
                                .save_file()
                            {
                                save_results_path = Some(path);
                            }
                        }
                    });
                    ui.separator();

                    egui::ScrollArea::both().show(ui, |ui| {
                        egui::Grid::new("results_grid")
                            .striped(true)
                            .min_col_width(60.0)
                            .show(ui, |ui| {
                                for row in &grid_cells {
                                    for cell in row {
                                        ui.label(cell);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                });
            },
        );

        if clear_results {
            let _ = self.session.dispatch(Command::ClearResults);
        }
        if let Some(path) = save_results_path {
            let _ = self.save_results_tsv(path);
        }
        if !is_open {
            self.show_results_window = false;
        }
    }

    pub fn render_particle_summary_window(&mut self, ctx: &egui::Context) {
        let mut is_open = true;
        let rows = self.session.particle_summaries.clone();
        let headings = ["Slice", "Count", "Total Area", "Average Size", "%Area",
            "Mean", "Mode", "Perim.", "Major", "Minor", "Angle", "Circ.",
            "Solidity", "IntDen", "Median", "Skew", "Kurt"];
        let extra: Vec<&str> = headings[5..].iter().copied()
            .filter(|heading| rows.iter().any(|row| row.means.contains_key(*heading)))
            .collect();
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ij_particle_summary_window"),
            egui::ViewportBuilder::default()
                .with_title("Summary")
                .with_inner_size([600.0, 230.0])
                .with_resizable(true),
            |ctx, _class| {
                if ctx.input(|input| input.viewport().close_requested()) {
                    is_open = false;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::both().show(ui, |ui| {
                        egui::Grid::new("particle_summary_grid").striped(true).show(ui, |ui| {
                            for heading in &headings[..5] { ui.label(*heading); }
                            for heading in &extra { ui.label(*heading); }
                            ui.end_row();
                            for row in &rows {
                                ui.label(&row.slice);
                                ui.label(row.count.to_string());
                                for value in [row.total_area, row.average_size, row.area_percent] {
                                    ui.label(ij_core::format_ij_num(value));
                                }
                                for heading in &extra {
                                    ui.label(row.means.get(*heading)
                                        .map(|value| ij_core::format_ij_num(*value))
                                        .unwrap_or_default());
                                }
                                ui.end_row();
                            }
                        });
                    });
                });
            },
        );
        if !is_open { self.show_particle_summary_window = false; }
    }

    /// Renders an image window with canvas, pixel display, zoom, pan, ROI tools, and slice slider.
    pub fn render_image_canvas_content(&mut self, ui: &mut egui::Ui, image_id: ImageId) {
        let (doc_w, doc_h, subtitle, stack_axes, current_pos, dim_sizes) = {
            let Some(doc) = self.session.get_image(image_id) else {
                ui.label("Image not found");
                return;
            };
            (
                doc.width,
                doc.height,
                ImageView::create_subtitle(doc),
                visible_stack_axes(doc),
                (
                    doc.get_channel(),
                    doc.get_slice(),
                    doc.get_frame(),
                    doc.current_slice,
                ),
                (
                    doc.get_n_channels(),
                    doc.get_n_slices(),
                    doc.get_n_frames(),
                    doc.stack_size(),
                ),
            )
        };

        let mut view = self
            .image_views
            .remove(&image_id)
            .unwrap_or_else(|| ImageView::new(doc_w, doc_h));

        // Subtitle bar
        ui.horizontal(|ui| {
            ui.label(&subtitle);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+").clicked() {
                    view.zoom_in();
                }
                if ui.button("-").clicked() {
                    view.zoom_out();
                }
                if ui.button("100%").clicked() {
                    view.set_zoom(1.0);
                    view.pan = (0.0, 0.0);
                }
                ui.label(format!("{}%", (view.zoom * 100.0).round() as i32));
            });
        });

        ui.separator();

        // Stack sliders (ImageJ StackWindow.java lines 58-114, Animator.java 109-171)
        let mut new_slice = None;
        let mut new_position = None;
        let mut toggle_anim_image = None;
        let is_animating = self.is_animating(image_id);
        let anim_axis = animation_slider_axis(&stack_axes);
        for axis in &stack_axes {
            let is_anim_target = *axis == anim_axis;
            match axis {
                StackAxis::Channel => {
                    let mut c_idx = current_pos.0 as i32;
                    let max_c = dim_sizes.0 as i32;
                    ui.horizontal(|ui| {
                        if is_anim_target && render_play_stop_button(ui, is_animating) {
                            toggle_anim_image = Some(image_id);
                        }
                        ui.label(format!("c {}/{}", c_idx, max_c));
                        if ui.add(egui::Slider::new(&mut c_idx, 1..=max_c)).changed() {
                            new_position =
                                Some((c_idx, current_pos.1 as i32, current_pos.2 as i32));
                        }
                    });
                }
                StackAxis::Slice => {
                    let mut z_idx = current_pos.1 as i32;
                    let max_z = dim_sizes.1 as i32;
                    ui.horizontal(|ui| {
                        if is_anim_target && render_play_stop_button(ui, is_animating) {
                            toggle_anim_image = Some(image_id);
                        }
                        ui.label(format!("z {}/{}", z_idx, max_z));
                        if ui.add(egui::Slider::new(&mut z_idx, 1..=max_z)).changed() {
                            new_position =
                                Some((current_pos.0 as i32, z_idx, current_pos.2 as i32));
                        }
                    });
                }
                StackAxis::Frame => {
                    let mut t_idx = current_pos.2 as i32;
                    let max_t = dim_sizes.2 as i32;
                    ui.horizontal(|ui| {
                        if is_anim_target && render_play_stop_button(ui, is_animating) {
                            toggle_anim_image = Some(image_id);
                        }
                        ui.label(format!("t {}/{}", t_idx, max_t));
                        if ui.add(egui::Slider::new(&mut t_idx, 1..=max_t)).changed() {
                            new_position =
                                Some((current_pos.0 as i32, current_pos.1 as i32, t_idx));
                        }
                    });
                }
                StackAxis::FlatSlice => {
                    let mut slice_idx = current_pos.3 as i32;
                    let max_s = dim_sizes.3 as i32;
                    ui.horizontal(|ui| {
                        if is_anim_target && render_play_stop_button(ui, is_animating) {
                            toggle_anim_image = Some(image_id);
                        }
                        ui.label(format!("Slice {}/{}", slice_idx, max_s));
                        if ui.add(egui::Slider::new(&mut slice_idx, 1..=max_s)).changed() {
                            new_slice = Some(slice_idx);
                        }
                    });
                }
            }
        }

        // Per-image texture caching keyed by reliable Session revision
        let current_revision = self.session.image_revision(image_id);
        let needs_upload = match self.texture_cache.get(&image_id) {
            Some((cached_rev, _)) => *cached_rev != current_revision,
            None => true,
        };

        if needs_upload {
            let rgba = {
                let doc = self.session.get_image(image_id).unwrap();
                plane_to_rgba(
                    doc.plane(),
                    doc.width,
                    doc.height,
                    doc.display_range,
                    doc.threshold,
                )
            };

            let color_image =
                ColorImage::from_rgba_unmultiplied([doc_w as usize, doc_h as usize], &rgba);

            // Nearest-neighbor texture so pixels remain crisp squares when zoomed
            let texture = ui.ctx().load_texture(
                format!("doc_tex_{}", image_id.0),
                color_image,
                TextureOptions::NEAREST,
            );
            self.texture_cache
                .insert(image_id, (current_revision, texture));
            self.texture_upload_count += 1;
        } else {
            self.texture_cache_hit_count += 1;
        }

        let texture_id = self.texture_cache.get(&image_id).unwrap().1.id();

        // The interaction area is the visible viewport. Panning can move the
        // image beyond its original bounds while keeping visible pixels usable.
        let canvas_size = Vec2::new(
            doc_w as f32 * view.zoom as f32,
            doc_h as f32 * view.zoom as f32,
        );

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        let tool = self.active_tool;
        if tool == Tool::Hand && response.dragged() {
            let delta = ui.input(|i| i.pointer.delta());
            view.pan.0 += delta.x;
            view.pan.1 += delta.y;
        }

        // Draw image texture with pan
        let img_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + view.pan.0, rect.min.y + view.pan.1),
            canvas_size,
        );
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new(("ij_image_canvas", image_id.0)), img_rect)
        });
        painter.image(
            texture_id,
            img_rect,
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        if response.clicked() || response.is_pointer_button_down_on() {
            let _ = self.set_active_window(image_id);
        }

        // Mouse hover: update status bar with pixel coordinate and value
        let mut hover_pixel = None;
        if let Some(hover_pos) = response.hover_pos().filter(|pos| img_rect.contains(*pos)) {
            let (ix, iy) = view.screen_to_pixel(hover_pos.x - rect.min.x, hover_pos.y - rect.min.y);
            hover_pixel = Some((ix, iy));
            if matches!(tool, Tool::Rectangle | Tool::Oval | Tool::Line | Tool::Point | Tool::Angle) {
                if let Some(roi) = self
                    .session
                    .get_image(image_id)
                    .and_then(|doc| doc.roi.as_ref())
                {
                    let screen = (
                        (hover_pos.x - img_rect.min.x) as f64,
                        (hover_pos.y - img_rect.min.y) as f64,
                    );
                    let icon = match hit_test(roi, screen, view.zoom, (0.0, 0.0)) {
                        RoiHit::Handle(0 | 4) => egui::CursorIcon::ResizeNwSe,
                        RoiHit::Handle(2 | 6) => egui::CursorIcon::ResizeNeSw,
                        RoiHit::Handle(1 | 5) => egui::CursorIcon::ResizeVertical,
                        RoiHit::Handle(3 | 7) => egui::CursorIcon::ResizeHorizontal,
                        RoiHit::Handle(_) | RoiHit::Inside => egui::CursorIcon::Move,
                        RoiHit::Outside => egui::CursorIcon::Crosshair,
                    };
                    ui.ctx().set_cursor_icon(icon);
                }
            }
        }

        // Active Tool Interactions
        if response.drag_started() {
            let _ = self.set_active_window(image_id);
            let start_pos = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos());
            if let Some(pos) = start_pos.filter(|pos| tool == Tool::Hand || img_rect.contains(*pos))
            {
                let (px, py) = view.screen_to_pixel(pos.x - rect.min.x, pos.y - rect.min.y);
                view.is_dragging = true;
                view.drag_start_pixel = Some((px, py));
                view.drag_current_pixel = Some((px, py));
                if matches!(tool, Tool::Rectangle | Tool::Oval | Tool::Line | Tool::Point | Tool::Angle) {
                    if let Some(roi) = self
                        .session
                        .get_image(image_id)
                        .and_then(|doc| doc.roi.as_ref())
                    {
                        let screen = (
                            (pos.x - img_rect.min.x) as f64,
                            (pos.y - img_rect.min.y) as f64,
                        );
                        let hit = hit_test(roi, screen, view.zoom, (0.0, 0.0));
                        if hit != RoiHit::Outside {
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(
                                    egui::Id::new(("ij_roi_drag", image_id.0)),
                                    RoiDrag {
                                        original: roi.clone(),
                                        hit,
                                        start_pixel: (px, py),
                                    },
                                )
                            });
                        }
                    }
                }
                if matches!(tool, Tool::Freehand | Tool::Freeline) {
                    // FreehandRoi's constructor seeds the polygon with the
                    // press point and nPoints=1 (FreehandRoi.java 10-17);
                    // the FREELINE tool uses the same class
                    // (ImagePlus.createNewRoi 2133-2135).
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            freehand_build_id(image_id),
                            FreehandBuild {
                                points: vec![(px, py)],
                            },
                        )
                    });
                }
            }
        }

        if view.is_dragging {
            let _ = self.set_active_window(image_id);
            if let Some(pos) = response.interact_pointer_pos() {
                if tool != Tool::Hand {
                    let (px, py) = view.screen_to_pixel(pos.x - rect.min.x, pos.y - rect.min.y);
                    view.drag_current_pixel = Some((px, py));
                }
            }
            if matches!(tool, Tool::Freehand | Tool::Freeline) {
                if let Some(curr) = view.drag_current_pixel {
                    let mut build = ui
                        .ctx()
                        .data(|data| data.get_temp::<FreehandBuild>(freehand_build_id(image_id)))
                        .unwrap_or_default();
                    freehand_push(&mut build.points, curr, doc_w, doc_h);
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(freehand_build_id(image_id), build));
                }
            }
            let edit = ui
                .ctx()
                .data(|data| data.get_temp::<RoiDrag>(egui::Id::new(("ij_roi_drag", image_id.0))));
            if let (Some(edit), Some(curr)) = (edit, view.drag_current_pixel) {
                let next = match edit.hit {
                    RoiHit::Inside => move_by(
                        &edit.original,
                        curr.0 - edit.start_pixel.0,
                        curr.1 - edit.start_pixel.1,
                    ),
                    RoiHit::Handle(handle) => drag_handle(&edit.original, handle, curr),
                    RoiHit::Outside => edit.original.clone(),
                };
                if self
                    .session
                    .get_image(image_id)
                    .and_then(|doc| doc.roi.as_ref())
                    != Some(&next)
                {
                    self.session.get_image_mut(image_id).unwrap().roi = Some(next);
                }
            }
        }

        enum DragAction {
            Rect(i32, i32, i32, i32),
            Oval(i32, i32, i32, i32),
            Line(i32, i32, i32, i32),
            Point(Vec<(i32, i32)>),
            Angle([(i32, i32); 3]),
        }

        let mut drag_action = None;
        let mut freehand_finish: Option<Vec<(i32, i32)>> = None;
        let mut freeline_finish: Option<Vec<(i32, i32)>> = None;
        if response.drag_stopped() {
            view.is_dragging = false;
            let edit = ui.ctx().data_mut(|data| {
                let key = egui::Id::new(("ij_roi_drag", image_id.0));
                let edit = data.get_temp::<RoiDrag>(key);
                data.remove::<RoiDrag>(key);
                edit
            });
            if let Some(edit) = edit {
                if let Some(roi) = self
                    .session
                    .get_image(image_id)
                    .and_then(|doc| doc.roi.as_ref())
                {
                    if roi != &edit.original {
                        drag_action = match roi {
                            Roi::Rect(r) => Some(DragAction::Rect(r.x, r.y, r.width, r.height)),
                            Roi::Oval(r) => Some(DragAction::Oval(r.x, r.y, r.width, r.height)),
                            Roi::Line(l) => Some(DragAction::Line(l.x1, l.y1, l.x2, l.y2)),
                            Roi::Polygon(_) | Roi::Polyline(_) => None,
                            Roi::Point(p) => Some(DragAction::Point(p.points.clone())),
                            Roi::Angle(a) => Some(DragAction::Angle(a.points)),
                        };
                        self.session.set_dirty(image_id, true);
                    }
                }
            } else if let (Some(start), Some(curr)) =
                (view.drag_start_pixel, view.drag_current_pixel)
            {
                let x = start.0.min(curr.0);
                let y = start.1.min(curr.1);
                let w = (start.0 - curr.0).abs();
                let h = (start.1 - curr.1).abs();

                match tool {
                    Tool::Rectangle => {
                        if w > 0 && h > 0 {
                            drag_action = Some(DragAction::Rect(x, y, w, h));
                        }
                    }
                    Tool::Oval => {
                        if w > 0 && h > 0 {
                            drag_action = Some(DragAction::Oval(x, y, w, h));
                        }
                    }
                    Tool::Line => {
                        drag_action = Some(DragAction::Line(start.0, start.1, curr.0, curr.1));
                    }
                    _ => {}
                }
            }
            if matches!(tool, Tool::Freehand | Tool::Freeline) {
                let build = ui.ctx().data_mut(|data| {
                    data.remove_temp::<FreehandBuild>(freehand_build_id(image_id))
                });
                if let Some(mut build) = build {
                    if let Some(curr) = view.drag_current_pixel {
                        freehand_push(&mut build.points, curr, doc_w, doc_h);
                    }
                    // handleMouseUp finishes via finishPolygon, which keeps a
                    // freehand area with three-plus vertices and a non-zero
                    // span, and an open line with two-plus vertices
                    // (FreehandRoi.java 90-96, PolygonRoi.java 591-593).
                    if tool == Tool::Freeline {
                        if polyline_is_valid(&build.points) {
                            freeline_finish = Some(build.points);
                        }
                    } else if freehand_is_valid(&build.points) {
                        freehand_finish = Some(build.points);
                    }
                }
            }
            view.drag_start_pixel = None;
            view.drag_current_pixel = None;
        }
        if !matches!(tool, Tool::Freehand | Tool::Freeline) {
            ui.ctx()
                .data_mut(|data| data.remove::<FreehandBuild>(freehand_build_id(image_id)));
        }

        let mut was_clicked = false;
        let mut wand_click = None;
        let mut point_click = None;
        if response.clicked() {
            was_clicked = true;
            if tool == Tool::Magnifier {
                let is_zoom_out = ui.input(|i| i.modifiers.alt);
                if is_zoom_out {
                    view.zoom_out();
                } else {
                    view.zoom_in();
                }
            } else if tool == Tool::Wand {
                if let Some(pos) = response
                    .interact_pointer_pos()
                    .filter(|pos| img_rect.contains(*pos))
                {
                    wand_click = Some(
                        view.screen_to_pixel(pos.x - img_rect.min.x, pos.y - img_rect.min.y),
                    );
                }
            } else if tool == Tool::Point {
                if let Some(pos) = response.interact_pointer_pos().filter(|pos| img_rect.contains(*pos)) {
                    point_click = Some(view.screen_to_pixel(pos.x - img_rect.min.x, pos.y - img_rect.min.y));
                }
            }
        }

        if matches!(tool, Tool::Dropper | Tool::Text) && response.clicked() {
            if let Some(pos) = response.interact_pointer_pos().filter(|p| img_rect.contains(*p)) {
                let (x, y) = view.screen_to_pixel(pos.x - img_rect.min.x, pos.y - img_rect.min.y);
                if tool == Tool::Dropper {
                    let doc = self.session.get_image(image_id).unwrap();
                    let rgba = plane_to_rgba(doc.plane(), doc.width, doc.height, doc.display_range, doc.threshold);
                    if x >= 0 && y >= 0 && x < doc.width as i32 && y < doc.height as i32 {
                        let index = (y as usize * doc.width as usize + x as usize) * 4;
                        let color = [rgba[index], rgba[index+1], rgba[index+2]];
                        let background = ui.input(|i| i.modifiers.alt);
                        let command = if background { Command::SetBackgroundColor { color } } else { Command::SetForegroundColor { color } };
                        let _ = self.session.dispatch(command);
                        self.status_message = format!("{} color: ({}, {}, {})", if background { "Background" } else { "Foreground" }, color[0], color[1], color[2]);
                    }
                } else {
                    let rect = ij_core::RectRoi::new(x, y, 8, 8).unwrap();
                    self.session.get_image_mut(image_id).unwrap().roi = Some(ij_core::Roi::Rect(rect));
                    self.text_selection = Some((image_id, x, y, String::new(), rect));
                }
            }
        }
        if self.text_selection.as_ref().is_some_and(|(id, _, _, _, rect)| {
            *id == image_id
                && self.session.get_image(*id).and_then(|doc| doc.roi.as_ref())
                    != Some(&ij_core::Roi::Rect(*rect))
        }) {
            self.text_selection = None;
        }
        if let Some((id, x, y, text, last_rect)) = &mut self.text_selection {
            if *id == image_id && tool == Tool::Text {
                egui::Window::new("Text selection").show(ui.ctx(), |ui| {
                    ui.text_edit_multiline(text);
                });
                let rect = ij_core::RectRoi::new(*x, *y, (text.lines().map(str::len).max().unwrap_or(1).max(1) * 8) as i32, (text.lines().count().max(1) * 8) as i32).unwrap();
                self.session.get_image_mut(image_id).unwrap().roi = Some(ij_core::Roi::Rect(rect));
                *last_rect = rect;
                painter.text(Pos2::new(img_rect.min.x + *x as f32 * view.zoom as f32, img_rect.min.y + *y as f32 * view.zoom as f32), egui::Align2::LEFT_TOP, text.as_str(), egui::FontId::monospace(8.0 * view.zoom as f32), Color32::YELLOW);
            }
        }
        let angle_key = angle_build_id(image_id);
        let mut angle_finish = None;
        if tool == Tool::Angle {
            let mut build = ui.ctx().data(|data| data.get_temp::<PolygonBuild>(angle_key)).unwrap_or_default();
            if let Some(pos) = response.hover_pos().filter(|pos| img_rect.contains(*pos)) {
                build.cursor = Some(view.screen_to_pixel(pos.x - img_rect.min.x, pos.y - img_rect.min.y));
            }
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos().filter(|pos| img_rect.contains(*pos)) {
                    build.vertices.push(view.screen_to_pixel(pos.x - img_rect.min.x, pos.y - img_rect.min.y));
                    if build.vertices.len() == 3 {
                        angle_finish = Some([build.vertices[0], build.vertices[1], build.vertices[2]]);
                    }
                }
            }
            if angle_finish.is_some() {
                ui.ctx().data_mut(|data| data.remove::<PolygonBuild>(angle_key));
            } else {
                ui.ctx().data_mut(|data| data.insert_temp(angle_key, build));
            }
        } else {
            ui.ctx().data_mut(|data| data.remove::<PolygonBuild>(angle_key));
        }

        // Polygon and Segmented Line tools: every click appends a vertex; a
        // double-click or a click in the 10x10 screen-pixel start box finishes
        // the construction (PolygonRoi.handleMouseUp 1100-1118). Nothing is
        // written to the document until the selection completes: as a closed
        // polygon for the Polygon tool, an open polyline for the Segmented
        // Line tool.
        let build_key = polygon_build_id(image_id);
        let mut polygon_finish: Option<Vec<(i32, i32)>> = None;
        let mut polyline_finish: Option<Vec<(i32, i32)>> = None;
        if matches!(tool, Tool::Polygon | Tool::Polyline) {
            let mut build = ui
                .ctx()
                .data(|data| data.get_temp::<PolygonBuild>(build_key))
                .unwrap_or_default();
            if let Some(pos) = response.hover_pos().filter(|pos| img_rect.contains(*pos)) {
                let (px, py) =
                    view.screen_to_pixel(pos.x - img_rect.min.x, pos.y - img_rect.min.y);
                build.cursor = Some((px, py));
            }
            if response.clicked() {
                if let Some(pos) = response
                    .interact_pointer_pos()
                    .filter(|pos| img_rect.contains(*pos))
                {
                    let rel = (pos.x - img_rect.min.x, pos.y - img_rect.min.y);
                    let (px, py) = view.screen_to_pixel(rel.0, rel.1);
                    // handleMouseUp completes a segment construction once
                    // nPoints>2 - committed vertices plus the rubber-band
                    // point - and the click lands in the start box or
                    // doubles the previous one; a polygon needs three
                    // committed vertices, a polyline two.
                    let min_vertices = if tool == Tool::Polyline { 2 } else { 3 };
                    let closes = build.vertices.len() >= min_vertices
                        && (response.double_clicked()
                            || polygon_closes_at_start(
                                build.vertices[0],
                                rel,
                                view.zoom as f32,
                            ));
                    if closes {
                        if tool == Tool::Polyline {
                            polyline_finish = Some(build.vertices.clone());
                        } else {
                            polygon_finish = Some(build.vertices.clone());
                        }
                    } else if !(tool == Tool::Polyline
                        && response.double_clicked()
                        && build.vertices.last() == Some(&(px, py)))
                    {
                        // handleMouseUp appends a repeated double-click only
                        // when samePoint is false (PolygonRoi.java 1121).
                        build.vertices.push((px, py));
                    }
                    let _ = self.set_active_window(image_id);
                }
            }
            if polygon_finish.is_some() || polyline_finish.is_some() {
                ui.ctx()
                    .data_mut(|data| data.remove::<PolygonBuild>(build_key));
            } else {
                ui.ctx().data_mut(|data| data.insert_temp(build_key, build));
            }
        } else {
            ui.ctx()
                .data_mut(|data| data.remove::<PolygonBuild>(build_key));
        }

        // Draw active ROI overlay (classic ImageJ yellow outline)
        let stroke = Stroke::new(1.0_f32, Color32::from_rgb(255, 255, 0));
        let z = view.zoom as f32;

        // Draw in-progress dragging preview
        let editing_existing = ui.ctx().data(|data| {
            data.get_temp::<RoiDrag>(egui::Id::new(("ij_roi_drag", image_id.0)))
                .is_some()
        });
        if view.is_dragging && !editing_existing {
            if let (Some(start), Some(curr)) = (view.drag_start_pixel, view.drag_current_pixel) {
                match tool {
                    Tool::Rectangle => {
                        let x = start.0.min(curr.0) as f32 * z + img_rect.min.x;
                        let y = start.1.min(curr.1) as f32 * z + img_rect.min.y;
                        let w = (start.0 - curr.0).abs() as f32 * z;
                        let h = (start.1 - curr.1).abs() as f32 * z;
                        painter.rect_stroke(
                            Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h)),
                            CornerRadius::ZERO,
                            stroke,
                            egui::StrokeKind::Outside,
                        );
                    }
                    Tool::Oval => {
                        let x = start.0.min(curr.0) as f32 * z + img_rect.min.x;
                        let y = start.1.min(curr.1) as f32 * z + img_rect.min.y;
                        let w = (start.0 - curr.0).abs() as f32 * z;
                        let h = (start.1 - curr.1).abs() as f32 * z;
                        draw_ellipse_stroke(
                            &painter,
                            Pos2::new(x + w / 2.0, y + h / 2.0),
                            w / 2.0,
                            h / 2.0,
                            stroke,
                        );
                    }
                    Tool::Line => {
                        let p1 = Pos2::new(
                            start.0 as f32 * z + img_rect.min.x,
                            start.1 as f32 * z + img_rect.min.y,
                        );
                        let p2 = Pos2::new(
                            curr.0 as f32 * z + img_rect.min.x,
                            curr.1 as f32 * z + img_rect.min.y,
                        );
                        painter.line_segment([p1, p2], stroke);
                    }
                    Tool::Freehand | Tool::Freeline => {
                        // FreehandRoi.drawLine repaints the growing path while
                        // the mouse is down (FreehandRoi.java 61-88); the
                        // FREELINE tool uses the same construction path.
                        if let Some(build) = ui.ctx().data(|data| {
                            data.get_temp::<FreehandBuild>(freehand_build_id(image_id))
                        }) {
                            let pts: Vec<Pos2> = build
                                .points
                                .iter()
                                .map(|&(x, y)| {
                                    Pos2::new(
                                        x as f32 * z + img_rect.min.x,
                                        y as f32 * z + img_rect.min.y,
                                    )
                                })
                                .collect();
                            for pair in pts.windows(2) {
                                painter.line_segment([pair[0], pair[1]], stroke);
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else if let Some(doc) = self.session.get_image(image_id) {
            // Draw committed ROI from document
            if let Some(roi) = &doc.roi {
                match roi {
                    Roi::Rect(r) => {
                        let rx = r.x as f32 * z + img_rect.min.x;
                        let ry = r.y as f32 * z + img_rect.min.y;
                        let rw = r.width as f32 * z;
                        let rh = r.height as f32 * z;
                        painter.rect_stroke(
                            Rect::from_min_size(Pos2::new(rx, ry), Vec2::new(rw, rh)),
                            CornerRadius::ZERO,
                            stroke,
                            egui::StrokeKind::Outside,
                        );
                    }
                    Roi::Oval(r) => {
                        let rx = r.x as f32 * z + img_rect.min.x;
                        let ry = r.y as f32 * z + img_rect.min.y;
                        let rw = r.width as f32 * z;
                        let rh = r.height as f32 * z;
                        draw_ellipse_stroke(
                            &painter,
                            Pos2::new(rx + rw / 2.0, ry + rh / 2.0),
                            rw / 2.0,
                            rh / 2.0,
                            stroke,
                        );
                    }
                    Roi::Line(l) => {
                        let p1 = Pos2::new(
                            l.x1 as f32 * z + img_rect.min.x,
                            l.y1 as f32 * z + img_rect.min.y,
                        );
                        let p2 = Pos2::new(
                            l.x2 as f32 * z + img_rect.min.x,
                            l.y2 as f32 * z + img_rect.min.y,
                        );
                        painter.line_segment([p1, p2], stroke);
                    }
                    Roi::Polygon(p) => {
                        let pts: Vec<Pos2> = p
                            .xpoints
                            .iter()
                            .zip(&p.ypoints)
                            .map(|(&x, &y)| {
                                Pos2::new(
                                    x as f32 * z + img_rect.min.x,
                                    y as f32 * z + img_rect.min.y,
                                )
                            })
                            .collect();
                        if pts.len() >= 3 {
                            for i in 0..pts.len() {
                                let next = if i + 1 == pts.len() { 0 } else { i + 1 };
                                painter.line_segment([pts[i], pts[next]], stroke);
                            }
                        }
                    }
                    Roi::Point(p) => {
                        for &(x, y) in &p.points {
                            let center = Pos2::new(x as f32 * z + img_rect.min.x, y as f32 * z + img_rect.min.y);
                            painter.line_segment([center + Vec2::new(-5.0, 0.0), center + Vec2::new(5.0, 0.0)], stroke);
                            painter.line_segment([center + Vec2::new(0.0, -5.0), center + Vec2::new(0.0, 5.0)], stroke);
                        }
                    }
                    Roi::Angle(a) => {
                        let points = a.points.map(|(x, y)| Pos2::new(x as f32 * z + img_rect.min.x, y as f32 * z + img_rect.min.y));
                        for pair in points.windows(2) {
                            painter.line_segment([pair[0], pair[1]], stroke);
                        }
                        for point in points {
                            painter.circle_filled(point, 3.0, Color32::from_rgb(255, 255, 0));
                        }
                    }
                    Roi::Polyline(p) => {
                        // PolygonRoi.draw paints open lines with
                        // g.drawPolyline: no closing segment back to the
                        // first vertex (PolygonRoi.java 228).
                        let pts: Vec<Pos2> = p
                            .points
                            .iter()
                            .map(|&(x, y)| {
                                Pos2::new(
                                    x as f32 * z + img_rect.min.x,
                                    y as f32 * z + img_rect.min.y,
                                )
                            })
                            .collect();
                        for pair in pts.windows(2) {
                            painter.line_segment([pair[0], pair[1]], stroke);
                        }
                    }
                }
            }
        }

        // Transient Polygon/Segmented Line tool rubber band: open edges plus
        // the live edge to the cursor, and the start box that completes the
        // selection (PolygonRoi.draw 228, drawStartBox 190-193).
        if matches!(tool, Tool::Polygon | Tool::Polyline) {
            if let Some(build) = ui
                .ctx()
                .data(|data| data.get_temp::<PolygonBuild>(build_key))
            {
                let to_screen = |p: (i32, i32)| {
                    Pos2::new(
                        p.0 as f32 * z + img_rect.min.x,
                        p.1 as f32 * z + img_rect.min.y,
                    )
                };
                for (from, to) in polygon_preview_segments(&build.vertices, build.cursor) {
                    painter.line_segment([to_screen(from), to_screen(to)], stroke);
                }
                if let Some(&start) = build.vertices.first() {
                    painter.rect_stroke(
                        Rect::from_center_size(to_screen(start), Vec2::splat(8.0)),
                        CornerRadius::ZERO,
                        stroke,
                        egui::StrokeKind::Outside,
                    );
                }
            }
        }

        if tool == Tool::Angle {
            if let Some(build) = ui.ctx().data(|data| data.get_temp::<PolygonBuild>(angle_key)) {
                let to_screen = |(x, y): (i32, i32)| Pos2::new(x as f32 * z + img_rect.min.x, y as f32 * z + img_rect.min.y);
                for (from, to) in polygon_preview_segments(&build.vertices, build.cursor) {
                    painter.line_segment([to_screen(from), to_screen(to)], stroke);
                }
            }
        }

        // Put the view back into self.image_views
        self.image_views.insert(image_id, view);

        // Dispatch deferred actions
        if let Some(id) = toggle_anim_image {
            self.toggle_animation(id);
        }

        if let Some(slice_idx) = new_slice {
            self.stop_animation(image_id);
            let _ = self.set_active_window(image_id);
            let _ = self.set_slice_active(slice_idx);
        }

        if let Some((c, z, t)) = new_position {
            self.stop_animation(image_id);
            let _ = self.set_active_window(image_id);
            let _ = self.navigate_position_active(c, z, t);
        }

        if let Some((ix, iy)) = hover_pixel {
            self.update_status_for_pixel(image_id, ix, iy);
        }

        if was_clicked {
            let _ = self.set_active_window(image_id);
        }

        if let Some((x, y)) = wand_click {
            if let Some(doc) = self.session.get_image(image_id) {
                let points = Wand::auto_outline(doc, x, y, 0.0, WandMode::Legacy);
                if !points.is_empty() {
                    match self.session.dispatch(Command::MakeWandRoi { x, y, points }) {
                        Ok(_) => self.status_message = "Wand ROI created".into(),
                        Err(error) => {
                            self.status_message = format!("Wand ROI failed: {error}");
                            self.dialogs.last_error = Some(self.status_message.clone());
                        }
                    }
                }
            }
        }

        if let Some(point) = point_click {
            let mut points = match self.session.get_image(image_id).and_then(|doc| doc.roi.as_ref()) {
                Some(Roi::Point(existing)) => existing.points.clone(),
                _ => Vec::new(),
            };
            points.push(point);
            if let Err(error) = self.session.dispatch(Command::MakePointRoi { points }) {
                self.dialogs.last_error = Some(error.to_string());
            }
        }

        if let Some(points) = angle_finish {
            if let Err(error) = self.session.dispatch(Command::MakeAngleRoi { points }) {
                self.dialogs.last_error = Some(error.to_string());
            }
        }

        if let Some(action) = drag_action {
            let _ = self.set_active_window(image_id);
            match action {
                DragAction::Rect(x, y, w, h) => {
                    let _ = self.dispatch_rect_roi(x, y, w, h);
                }
                DragAction::Oval(x, y, w, h) => {
                    let _ = self.dispatch_oval_roi(x, y, w, h);
                }
                DragAction::Line(x1, y1, x2, y2) => {
                    let _ = self.dispatch_line_roi(x1, y1, x2, y2);
                }
                DragAction::Point(points) => {
                    let _ = self.session.dispatch(Command::MakePointRoi { points });
                }
                DragAction::Angle(points) => {
                    let _ = self.session.dispatch(Command::MakeAngleRoi { points });
                }
            }
        }

        if let Some(points) = polygon_finish {
            let _ = self.set_active_window(image_id);
            let _ = self.dispatch_polygon_roi(points);
        }

        if let Some(points) = polyline_finish {
            let _ = self.set_active_window(image_id);
            match self.session.dispatch(Command::MakePolylineRoi { points }) {
                Ok(_) => self.status_message = "Segmented line ROI created".into(),
                Err(error) => {
                    self.status_message = format!("Segmented line ROI failed: {error}");
                    self.dialogs.last_error = Some(self.status_message.clone());
                }
            }
        }

        if let Some(points) = freehand_finish {
            let _ = self.set_active_window(image_id);
            match self.session.dispatch(Command::MakeFreehandRoi { points }) {
                Ok(_) => self.status_message = "Freehand ROI created".into(),
                Err(error) => {
                    self.status_message = format!("Freehand ROI failed: {error}");
                    self.dialogs.last_error = Some(self.status_message.clone());
                }
            }
        }

        if let Some(points) = freeline_finish {
            let _ = self.set_active_window(image_id);
            match self.session.dispatch(Command::MakeFreelineRoi { points }) {
                Ok(_) => self.status_message = "Freehand line ROI created".into(),
                Err(error) => {
                    self.status_message = format!("Freehand line ROI failed: {error}");
                    self.dialogs.last_error = Some(self.status_message.clone());
                }
            }
        }
    }

    /// The open rubber-band segments of the polygon currently being
    /// constructed for `image_id`, or `None` when no polygon is in progress.
    /// The image window paints exactly these segments while the tool is active.
    pub fn polygon_construction_preview(
        ctx: &egui::Context,
        image_id: ImageId,
    ) -> Option<Vec<((i32, i32), (i32, i32))>> {
        ctx.data(|data| data.get_temp::<PolygonBuild>(polygon_build_id(image_id)))
            .map(|build| polygon_preview_segments(&build.vertices, build.cursor))
    }
}

pub fn draw_ellipse_stroke(
    painter: &egui::Painter,
    center: Pos2,
    rad_x: f32,
    rad_y: f32,
    stroke: Stroke,
) {
    if rad_x <= 0.0 || rad_y <= 0.0 {
        return;
    }
    let n = 64;
    let pts: Vec<Pos2> = (0..n)
        .map(|i| {
            let a = i as f32 * std::f32::consts::TAU / n as f32;
            Pos2::new(center.x + rad_x * a.cos(), center.y + rad_y * a.sin())
        })
        .collect();
    for i in 0..pts.len() {
        let next = if i + 1 == pts.len() { 0 } else { i + 1 };
        painter.line_segment([pts[i], pts[next]], stroke);
    }
}
