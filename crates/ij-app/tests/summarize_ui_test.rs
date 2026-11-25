//! P4.9b: `Analyze > Summarize` menu wiring, enablement and Results display.
//!
//! Expected behaviour comes from `Analyzer.summarize`
//! (original_imagej/ij/plugin/filter/Analyzer.java 888-927) and
//! `ResultsTable`'s label column (ResultsTable.java 291-300, 670-690,
//! 700-733).

use egui::Context;
use ij_app::action::{AppAction, MenuKind, MENU_TREE};
use ij_app::app::ImageJApp;

fn analyze_menu() -> &'static [ij_app::action::MenuNode] {
    let analyze = MENU_TREE
        .iter()
        .find(|node| node.label == "Analyze")
        .expect("Analyze menu");
    let MenuKind::Submenu(nodes) = analyze.kind else {
        panic!("Analyze must be a submenu");
    };
    nodes
}

#[test]
fn analyze_menu_wires_summarize_after_analyze_particles() {
    // Menus.java 211-212: Analyze Particles... then Summarize.
    let labels: Vec<_> = analyze_menu().iter().map(|node| node.label).collect();
    let particles = labels
        .iter()
        .position(|label| *label == "Analyze Particles...")
        .expect("Analyze Particles item");
    let summarize = labels
        .iter()
        .position(|label| *label == "Summarize")
        .expect("Summarize item");
    assert_eq!(summarize, particles + 1);

    let item = &analyze_menu()[summarize];
    assert!(matches!(item.kind, MenuKind::Item(AppAction::Summarize)));
    assert_eq!(item.shortcut, None, "Menus.java 212 registers no shortcut");
}

#[test]
fn summarize_is_disabled_without_a_results_table() {
    let app = ImageJApp::new();
    assert!(
        app.action_enabled(AppAction::Summarize).is_err(),
        "an empty Results table has nothing to summarize"
    );
}

#[test]
fn summarize_runs_without_an_active_image_when_results_exist() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("summarize", 16, 16).unwrap();
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Measure, &ctx);
    assert_eq!(app.session.results.rows.len(), 2);

    // Analyzer.summarize only reads the Results table, so closing the image
    // must not disable the menu item.
    assert!(app.close_window(id));
    assert!(app.session.active_image_id().is_none());
    assert!(
        app.action_enabled(AppAction::Summarize).is_ok(),
        "Summarize must not require an active image"
    );

    app.perform(AppAction::Summarize, &ctx);

    assert_eq!(app.session.results.rows.len(), 6);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Summarize\");")
    );
    assert!(app.show_results_window, "Analyzer.java 926 calls rt.show()");
    assert_eq!(app.status_message, "Summarized results");
}

#[test]
fn summarize_appends_labeled_rows_and_the_results_grid_shows_them() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("summarize", 16, 16).unwrap();
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Measure, &ctx);

    app.perform(AppAction::Summarize, &ctx);

    let labels: Vec<&str> = app
        .session
        .results
        .rows
        .iter()
        .map(|row| {
            row.summary
                .as_ref()
                .map_or("", |summary| summary.label.as_str())
        })
        .collect();
    assert_eq!(labels, ["", "", "Mean", "SD", "Min", "Max"]);

    // ResultsTable.getHeadings prepends the row-label heading once labels
    // exist (ResultsTable.java 670-690).
    let grid = app.results_grid_cells();
    assert_eq!(
        grid[0].iter().map(String::as_str).collect::<Vec<_>>(),
        [" ", "Label", "Area", "Mean", "Min", "Max"]
    );
    assert_eq!(grid.len(), 7, "header plus six rows");
    assert_eq!(grid[3][1], "Mean");
    assert_eq!(grid[6][1], "Max");
    assert_eq!(grid[1][1], "", "measurement rows keep an empty label cell");
    for row in &grid {
        assert_eq!(row.len(), grid[0].len(), "every row has the label column");
    }
}

#[test]
fn summarize_is_a_no_op_for_a_single_row() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("summarize", 16, 16).unwrap();
    app.perform(AppAction::Measure, &ctx);
    let before = app.session.results.to_tsv();

    app.perform(AppAction::Summarize, &ctx);

    assert_eq!(
        app.session.results.to_tsv(),
        before,
        "Analyzer.java 892-893"
    );
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Summarize\");"),
        "the menu command is recorded even when it does nothing"
    );
}

#[test]
fn repeated_summarize_does_not_stack_rows() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("summarize", 16, 16).unwrap();
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Summarize, &ctx);
    let once = app.session.results.to_tsv();

    app.perform(AppAction::Summarize, &ctx);

    assert_eq!(app.session.results.to_tsv(), once, "Analyzer.java 889");
    assert_eq!(app.session.results.rows.len(), 6);
}

#[test]
fn clear_results_after_summarize_drops_the_label_column() {
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("summarize", 16, 16).unwrap();
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Summarize, &ctx);
    assert!(app.session.results.to_tsv().starts_with(" \tLabel\t"));

    app.perform(AppAction::ClearResults, &ctx);
    assert!(app.session.results.rows.is_empty());
    assert!(
        app.action_enabled(AppAction::Summarize).is_err(),
        "Clear Results leaves nothing to summarize"
    );

    app.perform(AppAction::Measure, &ctx);
    assert!(
        !app.session.results.to_tsv().starts_with(" \tLabel\t"),
        "ResultsTable.reset drops rowLabels (ResultsTable.java 1019-1033)"
    );
}

#[test]
fn summarize_no_op_keeps_the_results_window_hidden_and_the_status_unchanged() {
    // Analyzer.summarize returns before rt.show when the table has fewer than
    // two rows (Analyzer.java 889-893), so the menu command must not open the
    // Results window or claim that anything was summarized.
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("summarize", 16, 16).unwrap();
    app.perform(AppAction::Measure, &ctx);
    // Measure shows Results (Analyzer.displayResults calls rt.show), so close
    // the window first: a no-op Summarize must leave it closed.
    assert!(app.show_results_window);
    app.show_results_window = false;
    let status_before = app.status_message.clone();

    app.perform(AppAction::Summarize, &ctx);

    assert_eq!(app.session.results.rows.len(), 1);
    assert!(
        !app.show_results_window,
        "a no-op Summarize must not call rt.show (Analyzer.java 892-893)"
    );
    assert_eq!(app.status_message, status_before);
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Summarize\");"),
        "the menu command is still recorded"
    );
}

#[test]
fn repeated_summarize_after_a_trailing_max_row_is_a_no_op() {
    // The second call sees the trailing "Max" label and returns before
    // rt.show (Analyzer.java 889), so the window state and status stay put.
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("summarize", 16, 16).unwrap();
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Summarize, &ctx);
    assert!(app.show_results_window);
    let status_after_first = app.status_message.clone();
    let rows_after_first = app.session.results.rows.len();

    app.show_results_window = false;
    app.status_message = "unchanged".to_string();
    app.perform(AppAction::Summarize, &ctx);

    assert_eq!(app.session.results.rows.len(), rows_after_first);
    assert!(
        !app.show_results_window,
        "the trailing Max row makes the second call a no-op"
    );
    assert_eq!(app.status_message, "unchanged");
    assert_ne!(status_after_first, "unchanged");
}

#[test]
fn summarize_with_an_empty_results_table_is_a_deliberate_ui_deviation() {
    // DELIBERATE DEVIATION: Java's Analyze > Summarize on an empty table
    // throws "Row out of range" from Analyzer.java 889 (ResultsTable.java
    // 542-545) and prints a stack trace. Rust disables the menu item instead
    // of reproducing the exception, so this is not perfect menu parity.
    let app = ImageJApp::new();
    assert!(app.session.results.rows.is_empty());
    assert!(
        app.action_enabled(AppAction::Summarize).is_err(),
        "the disabled menu item is the documented deviation"
    );
}

#[test]
fn set_measurements_disables_the_label_column_after_summarize() {
    // Analyzer.setOptions disables row labels whenever Display label is off
    // (Analyzer.java 253-254), so the Results grid loses its Label column even
    // though the summary rows are still in the table.
    let ctx = Context::default();
    let mut app = ImageJApp::new();
    app.open_ramp_8bit("summarize", 16, 16).unwrap();
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Measure, &ctx);
    app.perform(AppAction::Summarize, &ctx);
    assert_eq!(
        app.results_grid_cells()[0]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [" ", "Label", "Area", "Mean", "Min", "Max"]
    );

    // The Set Measurements dialog is not driven here; dispatch the options it
    // would apply (Area, Mean, Min & max, Display label off).
    app.session
        .dispatch(ij_session::Command::SetMeasurements {
            flags: 19,
            precision: 3,
        })
        .unwrap();

    let grid = app.results_grid_cells();
    assert_eq!(
        grid[0].iter().map(String::as_str).collect::<Vec<_>>(),
        [" ", "Area", "Mean", "Min", "Max"],
        "Display label off removes the Label column"
    );
    assert_eq!(grid.len(), 7, "header plus six rows");
}
