use super::*;
use crate::action::AppAction;
use crate::roi_edit::{Arrow, nudge, nudge_corner};

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
            self.perform(AppAction::Quit, ctx);
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
            self.perform(AppAction::CloseAll, ctx);
            return;
        }

        // Close: W or Command/Ctrl+W (without Shift)
        let close = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::W)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::W)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::W)
        });
        if close {
            self.perform(AppAction::CloseImage(target_id), ctx);
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
            self.perform(AppAction::Undo, ctx);
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
            self.perform(AppAction::Invert, ctx);
            return;
        }

        // Crop: ImageJ's Image menu assigns Shift+X (Menus.java 181).
        let crop = ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::X));
        if crop {
            let _ = self.set_active_window(target_id);
            self.perform(AppAction::Crop, ctx);
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
            self.perform(AppAction::Measure, ctx);
            return;
        }

        // Histogram: ImageJ's Analyze menu assigns unmodified H (Menus.java 221-224).
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::H)) {
            let _ = self.set_active_window(target_id);
            self.perform(AppAction::Histogram, ctx);
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
            self.perform(AppAction::SelectNone, ctx);
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
            self.perform(AppAction::ZoomIn(target_id), ctx);
            return;
        }

        // Zoom Out: '-'
        let zoom_out = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Minus)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Minus)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Minus)
        });
        if zoom_out {
            self.perform(AppAction::ZoomOut(target_id), ctx);
            return;
        }

        // Comma and period navigate stacks regardless of selection.
        let next_slice = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Period));
        if next_slice {
            self.perform(AppAction::NextSlice(target_id), ctx);
            return;
        }
        let prev_slice = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Comma));
        if prev_slice {
            self.perform(AppAction::PreviousSlice(target_id), ctx);
            return;
        }

        let arrow = ctx.input_mut(|i| {
            let modifiers = i.modifiers;
            [
                (egui::Key::ArrowLeft, Arrow::Left),
                (egui::Key::ArrowRight, Arrow::Right),
                (egui::Key::ArrowUp, Arrow::Up),
                (egui::Key::ArrowDown, Arrow::Down),
            ]
            .into_iter()
            .find_map(|(key, arrow)| i.consume_key(modifiers, key).then_some((arrow, modifiers)))
        });
        if let Some((arrow, modifiers)) = arrow {
            let Some(doc) = self.session.get_image(target_id) else {
                return;
            };
            let stack_key = doc.stack_size() > 1 && (doc.roi.is_none() || modifiers.shift);
            if stack_key && matches!(arrow, Arrow::Right | Arrow::Left) {
                let action = if arrow == Arrow::Right {
                    AppAction::NextSlice(target_id)
                } else {
                    AppAction::PreviousSlice(target_id)
                };
                self.perform(action, ctx);
            } else if let Some(roi) = doc.roi.clone() {
                let updated = if modifiers.alt || modifiers.ctrl {
                    nudge_corner(&roi, arrow, doc.width, doc.height)
                } else {
                    nudge(&roi, arrow, doc.width, doc.height)
                };
                if updated != roi {
                    let _ = self.set_active_window(target_id);
                    self.session.get_image_mut(target_id).unwrap().roi = Some(updated);
                    self.session.set_dirty(target_id, true);
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
            self.perform(AppAction::Quit, ctx);
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
            self.perform(AppAction::CloseAll, ctx);
            return;
        }

        // Forward to active image if present
        if let Some(active_id) = self.active_image_id() {
            self.handle_image_shortcuts(ctx, active_id);
        }
    }
}
