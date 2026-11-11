use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use ij_app::action::{AppAction, MenuKind, MENU_TREE};
use ij_app::app::ImageJApp;
use ij_session::Command;

fn render(ctx: &Context, app: &mut ImageJApp, events: Vec<Event>) {
    let mut input = RawInput::default();
    input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 330.0)));
    input.events = events;
    let _ = ctx.run(input, |ctx| {
        CentralPanel::default().show(ctx, |ui| app.render_roi_manager_content(ui));
    });
}

fn click(ctx: &Context, app: &mut ImageJApp, label: &str) {
    render(ctx, app, Vec::new());
    let pos = ImageJApp::roi_manager_button_rect(ctx, label)
        .unwrap_or_else(|| panic!("missing ROI Manager {label} button"))
        .center();
    for event in [
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ] {
        render(ctx, app, vec![event]);
    }
}

#[test]
fn menus_expose_roi_manager_and_add_to_manager() {
    let edit = MENU_TREE.iter().find(|node| node.label == "Edit").unwrap();
    let MenuKind::Submenu(edit_nodes) = edit.kind else { panic!("Edit submenu") };
    let selection = edit_nodes.iter().find(|node| node.label == "Selection").unwrap();
    let MenuKind::Submenu(selection_nodes) = selection.kind else { panic!("Selection submenu") };
    let add = selection_nodes.iter().find(|node| node.label == "Add to Manager").unwrap();
    assert!(matches!(add.kind, MenuKind::Item(AppAction::AddToRoiManager)));
    assert_eq!(add.shortcut, Some("T"));

    let analyze = MENU_TREE.iter().find(|node| node.label == "Analyze").unwrap();
    let MenuKind::Submenu(analyze_nodes) = analyze.kind else { panic!("Analyze submenu") };
    let tools = analyze_nodes.iter().find(|node| node.label == "Tools").unwrap();
    let MenuKind::Submenu(tool_nodes) = tools.kind else { panic!("Tools submenu") };
    let manager = tool_nodes.iter().find(|node| node.label == "ROI Manager...").unwrap();
    assert!(matches!(manager.kind, MenuKind::Item(AppAction::RoiManager)));
}

#[test]
fn roi_manager_content_adds_measures_and_confirms_delete_all() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.perform(AppAction::RoiManager, &ctx);
    assert!(app.show_roi_manager_window);
    app.open_ramp_8bit("roi-manager", 4, 4).unwrap();
    app.session.dispatch(Command::MakeRectRoi { x: 1, y: 1, width: 2, height: 2 }).unwrap();

    click(&ctx, &mut app, "Add");
    assert_eq!(app.session.roi_manager.len(), 1);
    assert_eq!(app.session.roi_manager[0].name, "0002-0002");

    app.session.dispatch(Command::MakeRectRoi { x: 0, y: 0, width: 1, height: 1 }).unwrap();
    app.perform(AppAction::AddToRoiManager, &ctx);
    assert_eq!(app.session.roi_manager.len(), 2);
    click(&ctx, &mut app, "Measure");
    assert!(app.show_results_window);
    assert_eq!(app.session.results.rows.len(), 2);
    assert_eq!(app.session.results.rows[0].area, 4.0);
    assert_eq!(app.session.results.rows[1].area, 1.0);

    click(&ctx, &mut app, "Delete");
    assert!(app.pending_roi_manager_delete_all);
    assert_eq!(app.session.roi_manager.len(), 2);
}

#[test]
fn particle_dialog_opens_and_applies() {
 let mut app = ImageJApp::new();
 app.open_ramp_8bit("particles", 8, 8).unwrap();
 app.session.dispatch(ij_session::Command::SetThreshold { lower: 1.0, upper: 255.0 }).unwrap();
 app.particles.add_to_manager = true;
 app.particles.summarize = true;
 app.perform(AppAction::AnalyzeParticles, &egui::Context::default());
 assert!(app.particles.open);
 app.apply_particle_dialog().unwrap();
 assert!(!app.particles.open);
 assert!(!app.session.roi_manager.is_empty());
 assert!(app.show_particle_summary_window);
 assert_eq!(app.session.particle_summaries.len(), 1);
 assert_eq!(app.session.particle_summaries[0].count, app.session.roi_manager.len());
 app.session.dispatch(ij_session::Command::RoiManagerSelect { index: 0 }).unwrap();
 assert!(app.session.active_doc().unwrap().roi.is_some());
}
