use super::*;

impl ImageJApp {
    pub fn update_ui(&mut self, ctx: &egui::Context) {
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
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.render_menu_bar(ui);
        });

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
                        } else if self.pending_close.is_some() {
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
            self.request_close_window(id);
        }

        // Render modal / secondary dialogs
        self.render_dialogs(ctx);

        // Results Table Window if opened
        if self.show_results_window {
            self.render_results_window(ctx);
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
            .to_tsv_basic()
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

    /// Renders an image window with canvas, pixel display, zoom, pan, ROI tools, and slice slider.
    pub fn render_image_canvas_content(&mut self, ui: &mut egui::Ui, image_id: ImageId) {
        let (doc_w, doc_h, subtitle, stack_size, current_slice) = {
            let Some(doc) = self.session.get_image(image_id) else {
                ui.label("Image not found");
                return;
            };
            (
                doc.width,
                doc.height,
                ImageView::create_subtitle(doc),
                doc.stack_size(),
                doc.current_slice,
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

        // Stack slice slider if stack has more than 1 slice
        let mut new_slice = None;
        if stack_size > 1 {
            let mut slice_idx = current_slice as i32;
            ui.horizontal(|ui| {
                ui.label(format!("Slice {}/{}", current_slice, stack_size));
                if ui
                    .add(egui::Slider::new(&mut slice_idx, 1..=stack_size as i32))
                    .changed()
                {
                    new_slice = Some(slice_idx);
                }
            });
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
        }

        enum DragAction {
            Rect(i32, i32, i32, i32),
            Oval(i32, i32, i32, i32),
            Line(i32, i32, i32, i32),
        }

        let mut drag_action = None;
        if response.drag_stopped() {
            view.is_dragging = false;
            if let (Some(start), Some(curr)) = (view.drag_start_pixel, view.drag_current_pixel) {
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
            view.drag_start_pixel = None;
            view.drag_current_pixel = None;
        }

        let mut was_clicked = false;
        if response.clicked() {
            was_clicked = true;
            if tool == Tool::Magnifier {
                let is_zoom_out = ui.input(|i| i.modifiers.alt);
                if is_zoom_out {
                    view.zoom_out();
                } else {
                    view.zoom_in();
                }
            }
        }

        // Draw active ROI overlay (classic ImageJ yellow outline)
        let stroke = Stroke::new(1.0_f32, Color32::from_rgb(255, 255, 0));
        let z = view.zoom as f32;

        // Draw in-progress dragging preview
        if view.is_dragging {
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
                }
            }
        }

        // Put the view back into self.image_views
        self.image_views.insert(image_id, view);

        // Dispatch deferred actions
        if let Some(slice_idx) = new_slice {
            let _ = self.set_active_window(image_id);
            let _ = self.set_slice_active(slice_idx);
        }

        if let Some((ix, iy)) = hover_pixel {
            self.update_status_for_pixel(image_id, ix, iy);
        }

        if was_clicked {
            let _ = self.set_active_window(image_id);
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
            }
        }
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
