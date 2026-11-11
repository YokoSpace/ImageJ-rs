use ij_app::action::{MENU_TREE, MenuKind, MenuNode};
use ij_session::generated_commands::{COMMAND_SPECS, ExecutionTier};
use std::collections::BTreeSet;

const KNOWN_UNWIRED: &[&str] = &[
    "RGB Color",
    "Duplicate...",
    "Histogram",
    "Set Measurements...",
    "Save",
    "Tiff...",
    "Results...",
    "Results... ",
];

fn collect_wired(nodes: &[MenuNode], labels: &mut BTreeSet<&'static str>) {
    for node in nodes {
        match node.kind {
            MenuKind::Item(_) => {
                labels.insert(node.label);
            }
            MenuKind::Submenu(children) => collect_wired(children, labels),
            MenuKind::Separator | MenuKind::Unsupported | MenuKind::WindowList => {}
        }
    }
}

#[test]
fn tier_one_commands_have_menu_actions_or_an_explicit_gap() {
    let mut wired = BTreeSet::new();
    collect_wired(MENU_TREE, &mut wired);

    let missing: BTreeSet<_> = COMMAND_SPECS
        .iter()
        .filter(|spec| spec.tier == ExecutionTier::RustTier1)
        .map(|spec| spec.label)
        .filter(|label| !wired.contains(label))
        .collect();
    let known: BTreeSet<_> = KNOWN_UNWIRED.iter().copied().collect();
    assert_eq!(missing, known, "update KNOWN_UNWIRED when menu wiring changes");
}
