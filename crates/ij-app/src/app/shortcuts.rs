use super::*;

impl ImageJApp {
    /// Handles keyboard shortcuts targeted at a specific image window.
    /// Uses key consumption (`consume_key`) so shortcuts execute exactly once
    /// without duplicate dispatches across viewports or frames.
    pub fn handle_image_shortcuts(&mut self, ctx: &egui::Context, target_id: ImageId) {
        // Respect text input focus: do not capture typing intended for TextEdit / DragValue
        if ctx.wants_keyboard_input() {
            return;
        }

        // Respect modal close/save confirmation, stack process prompts, and error dialogs
        if self.pending_close.is_some()
            || self.pending_stack_process.is_some()
            || self.dialogs.last_error.is_some()
        {
            return;
        }

        // Quit: Cmd+Q or Ctrl+Q
        let quit = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Q)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Q)
        });
        if quit {
            self.request_quit();
            return;
        }

        // Close All: Shift+W or Command/Ctrl+Shift+W
        let close_all = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::W,
            ) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::W)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::W)
        });
        if close_all {
            if self.open_window_count() > 0 {
                self.request_close_all();
            }
            return;
        }

        // Close: W or Command/Ctrl+W (without Shift)
        let close = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::W)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::W)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::W)
        });
        if close {
            self.request_close_window(target_id);
            return;
        }

        // Undo: Z or Command/Ctrl+Z (without Shift)
        let undo = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Z)
        });
        if undo {
            let _ = self.set_active_window(target_id);
            let _ = self.undo_active();
            return;
        }

        // Invert: Shift+I or Command/Ctrl+Shift+I
        let invert = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::I,
            ) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::I)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::I)
        });
        if invert {
            let _ = self.set_active_window(target_id);
            let _ = self.invert_active();
            return;
        }

        // Measure: M or Command/Ctrl+M (without Shift)
        let measure = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::M)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::M)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::M)
        });
        if measure {
            let _ = self.set_active_window(target_id);
            let _ = self.measure_active();
            return;
        }

        // Select None: Shift+A or Command/Ctrl+Shift+A
        let select_none = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::A,
            ) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::A)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::A)
        });
        if select_none {
            let _ = self.set_active_window(target_id);
            let _ = self.clear_roi();
            return;
        }

        // Zoom In: '+' or '='
        let zoom_in = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Plus)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Plus)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Plus)
                || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Equals)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Equals)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Equals)
        });
        if zoom_in {
            if let Some(view) = self.image_views.get_mut(&target_id) {
                view.zoom_in();
            }
            return;
        }

        // Zoom Out: '-'
        let zoom_out = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Minus)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Minus)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Minus)
        });
        if zoom_out {
            if let Some(view) = self.image_views.get_mut(&target_id) {
                view.zoom_out();
            }
            return;
        }

        // Stack Navigation
        let has_roi = self
            .session
            .get_image(target_id)
            .map(|doc| doc.roi.is_some())
            .unwrap_or(false);

        let next_slice = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Period)
                || (!has_roi && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight))
        });
        if next_slice {
            if let Some(doc) = self.session.get_image(target_id) {
                let sz = doc.stack_size();
                let cur = doc.current_slice;
                if sz > 1 && (cur as usize) < sz {
                    let _ = self.set_active_window(target_id);
                    let _ = self.set_slice_active((cur + 1) as i32);
                }
            }
            return;
        }

        let prev_slice = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Comma)
                || (!has_roi && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft))
        });
        if prev_slice {
            if let Some(doc) = self.session.get_image(target_id) {
                let sz = doc.stack_size();
                let cur = doc.current_slice;
                if sz > 1 && cur > 1 {
                    let _ = self.set_active_window(target_id);
                    let _ = self.set_slice_active((cur - 1) as i32);
                }
            }
        }
    }

    /// Handles advertised keyboard shortcuts for supported actions per ImageJ keyPressed and Menus.
    /// When invoked on the root toolbar viewport, acts on the active image window (if any).
    pub fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input()
            || self.pending_close.is_some()
            || self.pending_stack_process.is_some()
            || self.dialogs.last_error.is_some()
        {
            return;
        }

        // Quit: Cmd+Q or Ctrl+Q
        let quit = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Q)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Q)
        });
        if quit {
            self.request_quit();
            return;
        }

        // Close All: Shift+W or Command/Ctrl+Shift+W
        let close_all = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::W,
            ) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::W)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::W)
        });
        if close_all {
            if self.open_window_count() > 0 {
                self.request_close_all();
            }
            return;
        }

        // Forward to active image if present
        if let Some(active_id) = self.active_image_id() {
            self.handle_image_shortcuts(ctx, active_id);
        }
    }

}
