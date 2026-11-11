#!/usr/bin/env python3
"""
Deterministic parser for original_imagej/IJ_Props.txt and original_imagej/ij/Menus.java.
Generates docs/command_ledger.md and crates/ij-session/src/generated_commands.rs.
Re-run via: python3 tools/ledger/sync_ledger.py [--check]
"""
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
PROPS_PATH = REPO_ROOT / "original_imagej" / "IJ_Props.txt"
MENUS_JAVA_PATH = REPO_ROOT / "original_imagej" / "ij" / "Menus.java"
LEDGER_MD_PATH = REPO_ROOT / "docs" / "command_ledger.md"
GENERATED_RS_PATH = REPO_ROOT / "crates" / "ij-session" / "src" / "generated_commands.rs"

ENTRY_RE = re.compile(r'^([a-zA-Z0-9_]+)\s*=\s*"([^"]+)"\s*,\s*(.+)$')
SHORTCUT_RE = re.compile(r'^(.*)\[([^\]]+)\]\s*$')
CLASS_ARG_RE = re.compile(r'^([a-zA-Z0-9_.$]+)(?:\("([^"]*)"\))?$')
MENUS_ADD_PLUGIN_RE = re.compile(
    r'addPlugInItem\(\s*([a-zA-Z0-9_]+)\s*,\s*"([^"]+)"\s*,\s*"((?:\\"|[^"])+)"\s*,\s*([^,]+)\s*,\s*(true|false)\s*\)'
)
MENUS_ADD_CHECKBOX_RE = re.compile(
    r'addCheckboxItem\(\s*([a-zA-Z0-9_]+)\s*,\s*"([^"]+)"\s*,\s*"((?:\\"|[^"])+)"\s*\)'
)

RUST_SLICE_1_COMMANDS = {
    "Open...",
    "Save",
    "Tiff...",
    "Results...",
    "Undo",
    "Invert",
    "Measure",
    "Set Measurements...",
    "Clear Results",
    "Brightness/Contrast...",
    "8-bit",
    "16-bit",
    "32-bit",
    "RGB Color",
    "Duplicate...",
    "Histogram",
}


def classify_target(target: str):
    target_unescaped = target.replace('\\"', '"').strip()
    cm = CLASS_ARG_RE.match(target_unescaped)
    if cm:
        java_class, java_arg = cm.groups()
        return java_class, java_arg or ""
    return target_unescaped, ""


def parse_menus_java(text: str):
    entries = []
    seen_labels = set()
    idx = 0
    for raw_line in text.splitlines():
        line = raw_line.strip()
        m = MENUS_ADD_PLUGIN_RE.search(line)
        if m:
            menu_var, label, target, key_ev, shift = m.groups()
            if label in seen_labels or "+" in label:
                continue
            seen_labels.add(label)
            idx += 1
            java_class, java_arg = classify_target(target)
            shortcut = ""
            if "VK_" in key_ev:
                key_char = key_ev.split("VK_")[-1]
                shortcut = f"Shift+{key_char}" if shift == "true" else key_char
            tier = "RustTier1" if label in RUST_SLICE_1_COMMANDS else "JavaBridge"
            entries.append({
                "key": f"menus_{idx:02d}",
                "section": f"Menus.java ({menu_var})",
                "label": label,
                "shortcut": shortcut,
                "java_class": java_class,
                "java_arg": java_arg,
                "tier": tier,
            })
            continue
        cm = MENUS_ADD_CHECKBOX_RE.search(line)
        if cm:
            menu_var, label, target = cm.groups()
            if label in seen_labels:
                continue
            seen_labels.add(label)
            idx += 1
            java_class, java_arg = classify_target(target)
            tier = "RustTier1" if label in RUST_SLICE_1_COMMANDS else "JavaBridge"
            entries.append({
                "key": f"menus_{idx:02d}",
                "section": f"Menus.java ({menu_var})",
                "label": label,
                "shortcut": "",
                "java_class": java_class,
                "java_arg": java_arg,
                "tier": tier,
            })
    return entries, seen_labels


def parse_props(text: str, seen_labels: set):
    entries = []
    current_section = "General"
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith("#"):
            comment = line.lstrip("#").strip()
            if comment.startswith("Plugins installed in the "):
                current_section = comment.replace("Plugins installed in the ", "").rstrip(" submenu").rstrip(" menu")
            continue
        m = ENTRY_RE.match(line)
        if not m:
            continue
        key, raw_label, target = m.groups()
        shortcut = ""
        label = raw_label
        sm = SHORTCUT_RE.match(raw_label)
        if sm:
            label, shortcut = sm.groups()
        if label in seen_labels:
            continue
        seen_labels.add(label)
        java_class, java_arg = classify_target(target)
        # IJ_Props.txt uses "Results... " for import and "Results..." for
        # export. Whitespace is part of the registered command identity.
        tier = "RustTier1" if label in RUST_SLICE_1_COMMANDS else "JavaBridge"

        entries.append({
            "key": key,
            "section": current_section,
            "label": label,
            "shortcut": shortcut,
            "java_class": java_class,
            "java_arg": java_arg,
            "tier": tier,
        })
    return entries


def render_markdown(entries):
    tier1_count = sum(1 for e in entries if e["tier"] == "RustTier1")
    bridge_count = sum(1 for e in entries if e["tier"] == "JavaBridge")
    lines = [
        "# ImageJ 1.54u command ledger",
        "",
        "Generated from `original_imagej/ij/Menus.java` and `original_imagej/IJ_Props.txt` by `tools/ledger/sync_ledger.py`. Do not edit by hand.",
        "",
        f"- Total registered menu commands: **{len(entries)}**",
        f"- Rust Vertical Slice 1 target (`RustTier1`): **{tier1_count}**",
        f"- Delegated to Java compatibility session (`JavaBridge`): **{bridge_count}**",
        "",
        "| Key | Menu section | Command label | Shortcut | Java class | Arg | Target tier |",
        "| :--- | :--- | :--- | :--- | :--- | :--- | :--- |",
    ]
    for e in entries:
        label_esc = e["label"].replace("|", "\\|")
        arg_esc = e["java_arg"].replace("|", "\\|")
        lines.append(
            f"| `{e['key']}` | {e['section']} | `{label_esc}` | `{e['shortcut']}` | `{e['java_class']}` | `{arg_esc}` | `{e['tier']}` |"
        )
    lines.append("")
    return "\n".join(lines)


def escape_rs(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def render_rust(entries):
    lines = [
        "// Generated from original_imagej/ij/Menus.java and original_imagej/IJ_Props.txt by tools/ledger/sync_ledger.py.",
        "// Do not edit by hand.",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub enum ExecutionTier {",
        "    RustTier1,",
        "    JavaBridge,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct CommandSpec {",
        "    pub prop_key: &'static str,",
        "    pub section: &'static str,",
        "    pub label: &'static str,",
        "    pub shortcut: &'static str,",
        "    pub java_class: &'static str,",
        "    pub java_arg: &'static str,",
        "    pub tier: ExecutionTier,",
        "}",
        "",
        f"pub const COMMAND_SPECS: [CommandSpec; {len(entries)}] = [",
    ]
    for e in entries:
        lines.append("    CommandSpec {")
        lines.append(f'        prop_key: "{escape_rs(e["key"])}",')
        lines.append(f'        section: "{escape_rs(e["section"])}",')
        lines.append(f'        label: "{escape_rs(e["label"])}",')
        lines.append(f'        shortcut: "{escape_rs(e["shortcut"])}",')
        lines.append(f'        java_class: "{escape_rs(e["java_class"])}",')
        lines.append(f'        java_arg: "{escape_rs(e["java_arg"])}",')
        lines.append(f'        tier: ExecutionTier::{e["tier"]},')
        lines.append("    },")
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


def main():
    check_only = "--check" in sys.argv
    menus_text = MENUS_JAVA_PATH.read_text(encoding="utf-8", errors="replace")
    props_text = PROPS_PATH.read_text(encoding="utf-8", errors="replace")
    menus_entries, seen_labels = parse_menus_java(menus_text)
    props_entries = parse_props(props_text, seen_labels)
    entries = menus_entries + props_entries
    md_out = render_markdown(entries)
    rs_out = render_rust(entries)

    if check_only:
        if not LEDGER_MD_PATH.exists() or LEDGER_MD_PATH.read_text(encoding="utf-8") != md_out:
            print(f"Out of sync: {LEDGER_MD_PATH}", file=sys.stderr)
            sys.exit(1)
        if not GENERATED_RS_PATH.exists() or GENERATED_RS_PATH.read_text(encoding="utf-8") != rs_out:
            print(f"Out of sync: {GENERATED_RS_PATH}", file=sys.stderr)
            sys.exit(1)
        print(f"Ledger check passed ({len(entries)} commands synchronized).")
        return

    LEDGER_MD_PATH.parent.mkdir(parents=True, exist_ok=True)
    GENERATED_RS_PATH.parent.mkdir(parents=True, exist_ok=True)
    LEDGER_MD_PATH.write_text(md_out, encoding="utf-8")
    GENERATED_RS_PATH.write_text(rs_out, encoding="utf-8")
    print(f"Generated {LEDGER_MD_PATH} and {GENERATED_RS_PATH} ({len(entries)} commands).")


if __name__ == "__main__":
    main()
