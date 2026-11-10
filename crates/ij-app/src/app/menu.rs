use super::*;
use crate::action::{AppAction, MENU_TREE, MenuKind, MenuNode};

impl ImageJApp {
    pub(super) fn render_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            for node in MENU_TREE {
                if let MenuKind::Submenu(children) = node.kind {
                    ui.menu_button(node.label, |ui| {
                        scrollable_menu(ui, |ui| self.render_menu_nodes(ui, children));
                    });
                }
            }
        });
    }

    fn render_menu_nodes(&mut self, ui: &mut egui::Ui, nodes: &[MenuNode]) {
        for node in nodes {
            match node.kind {
                MenuKind::Item(action) => {
                    let enabled = self.action_enabled(action);
                    let shortcut = match action {
                        AppAction::Quit if !cfg!(target_os = "macos") => Some("Ctrl+Q"),
                        _ => node.shortcut,
                    };
                    let label = match shortcut {
                        Some(shortcut) => format!("{}\t{shortcut}", node.label),
                        None => node.label.to_string(),
                    };
                    let response = ui.add_enabled(enabled.is_ok(), egui::Button::new(label));
                    let response = match enabled {
                        Ok(()) => response,
                        Err(reason) => response.on_hover_text(reason),
                    };
                    if response.clicked() {
                        self.perform(action, ui.ctx());
                        ui.close_menu();
                    }
                }
                MenuKind::Submenu(children) => {
                    ui.menu_button(node.label, |ui| {
                        scrollable_menu(ui, |ui| self.render_menu_nodes(ui, children));
                    });
                }
                MenuKind::Separator => {
                    ui.separator();
                }
                MenuKind::Unsupported => {
                    let label = match node.shortcut {
                        Some(shortcut) => format!("{}\t{shortcut} (unsupported)", node.label),
                        None => format!("{} (unsupported)", node.label),
                    };
                    let _ = ui.add_enabled(false, egui::Button::new(label));
                }
                MenuKind::WindowList => {
                    for id in self.session.image_ids() {
                        let title = self
                            .session
                            .get_image(id)
                            .map(|doc| doc.title.clone())
                            .unwrap_or_else(|| format!("Image {}", id.0));
                        let prefix = if self.active_image_id() == Some(id) {
                            "✓ "
                        } else {
                            "  "
                        };
                        if ui.button(format!("{prefix}{title}")).clicked() {
                            self.perform(AppAction::ActivateImage(id), ui.ctx());
                            ui.close_menu();
                        }
                    }
                }
            }
        }
    }
}

/// Wraps popup menu contents in a vertical ScrollArea constrained to the available
/// vertical space in the root window viewport (such as the default 680x200 root window).
/// This prevents long menus, submenus, or open window lists from being clipped
/// by viewport bounds, keeping all items accessible and interactable via scroll.
pub fn scrollable_menu<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let screen_h = ui.ctx().screen_rect().height();
    let cursor_y = ui.cursor().top();
    let max_h = (screen_h - cursor_y - 8.0).max(60.0);
    egui::ScrollArea::vertical()
        .max_height(max_h)
        .auto_shrink([true, true])
        .show(ui, add_contents)
        .inner
}
