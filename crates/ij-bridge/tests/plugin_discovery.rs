//! Discovery tests for the read-only classic ImageJ plugin catalog scanner.
//!
//! The expectations in these tests are derived from ImageJ 1.54u
//! `original_imagej/ij/Menus.java` (`getPlugins`, `checkSubdirectory`,
//! `getConfigurationFile`, `autoGenerateConfigFile`, `installJarPlugin`) and,
//! for the differential test, from running those unmodified Java methods in a
//! throwaway `ij.DiscoveryHarness` (see `java_discovery_harness_source`).

use ij_bridge::plugin::{
    PluginBridge, PluginInspectionKind, PluginInspectionRequest, PluginInspectionStatus,
};
use ij_bridge::plugin_discovery::{
    ConfigOrigin, DiscoveredPlugin, DiscoveryWarning, InspectionOptions, PluginCatalog, PluginKind,
    PluginInspectionFailure, PluginSource, ScanOptions,
};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn touch(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, b"").unwrap();
}

fn write_config(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

fn plugin<'a>(catalog: &'a PluginCatalog, label: &str) -> &'a DiscoveredPlugin {
    catalog
        .plugins
        .iter()
        .find(|p| p.label == label)
        .unwrap_or_else(|| {
            panic!(
                "no discovered plugin labelled {label:?}: {:#?}",
                catalog.plugins
            )
        })
}

fn labels(catalog: &PluginCatalog) -> Vec<&str> {
    catalog.plugins.iter().map(|p| p.label.as_str()).collect()
}

fn class_path(p: &DiscoveredPlugin) -> &Path {
    match &p.source {
        PluginSource::ClassFile { path } => path,
        other => panic!("expected a class file source, got {other:?}"),
    }
}

fn archive_path(p: &DiscoveredPlugin) -> &Path {
    match &p.source {
        PluginSource::Archive { archive, .. } => archive,
        other => panic!("expected an archive source, got {other:?}"),
    }
}

fn menu(p: &DiscoveredPlugin) -> Option<Vec<&str>> {
    p.menu_path
        .as_ref()
        .map(|path| path.iter().map(String::as_str).collect())
}

fn archive_report<'a>(
    catalog: &'a PluginCatalog,
    name: &str,
) -> &'a ij_bridge::plugin_discovery::ArchiveReport {
    catalog
        .archives
        .iter()
        .find(|report| report.path.file_name().unwrap() == name)
        .unwrap_or_else(|| panic!("no archive report for {name}: {:#?}", catalog.archives))
}

// ---------------------------------------------------------------------------
// Class file discovery
// ---------------------------------------------------------------------------

#[test]
fn discovers_top_level_and_one_level_subdir_class_files() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    touch(&plugins.join("Top_Plugin.class"));
    touch(&plugins.join("_Leading.class"));
    touch(&plugins.join("Sub").join("Tool_A.class"));
    touch(&plugins.join("Sub").join("Another_Thing.class"));

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    assert_eq!(
        labels(&catalog),
        vec!["Another Thing", "Tool A", "Top Plugin", " Leading"]
    );
    // `Menus.getPlugins` sorts the class list with `StringSorter.sort`
    // (String.compareTo) before `installUserPlugin` runs.
    assert_eq!(
        catalog
            .plugins
            .iter()
            .map(|p| p.class_spec.as_str())
            .collect::<Vec<_>>(),
        vec!["Sub.Another_Thing", "Sub.Tool_A", "Top_Plugin", "_Leading"]
    );

    let top = plugin(&catalog, "Top Plugin");
    assert_eq!(menu(top), Some(vec!["Plugins"]));
    assert_eq!(class_path(top), plugins.join("Top_Plugin.class"));
    assert_eq!(top.kind, PluginKind::Uninspected);

    let sub = plugin(&catalog, "Tool A");
    // `installUserPlugin` uses the raw subdirectory name for the submenu
    // (underscores are only replaced in auto-generated JAR submenus).
    assert_eq!(menu(sub), Some(vec!["Plugins", "Sub"]));
    assert_eq!(class_path(sub), plugins.join("Sub").join("Tool_A.class"));
    assert_eq!(sub.kind, PluginKind::Uninspected);

    assert!(catalog.warnings.is_empty(), "{:#?}", catalog.warnings);
}

#[test]
fn ignores_non_plugin_files_and_nested_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    touch(&plugins.join("Good_Plugin.class"));
    touch(&plugins.join("plain.class"));
    touch(&plugins.join("With$Inner_Thing.class"));
    touch(&plugins.join("Upper_Thing.CLASS"));
    touch(&plugins.join("notes.ijm"));
    touch(&plugins.join("readme.txt"));
    touch(&plugins.join("No_Underscore.jar"));
    touch(
        &plugins
            .join("Deep")
            .join("Inner")
            .join("Nested_Thing.class"),
    );

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    assert_eq!(labels(&catalog), vec!["Good Plugin"]);
    assert!(catalog.archives.is_empty());
}

#[test]
fn all_entries_are_uninspected_in_phase_one() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    touch(&plugins.join("One_Plugin.class"));
    write_zip(
        &plugins.join("Jar_Plugins.jar"),
        &[("Two_Plugin.class", b"")],
    );

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    assert_eq!(catalog.plugins.len(), 2);
    assert!(
        catalog
            .plugins
            .iter()
            .all(|p| p.kind == PluginKind::Uninspected)
    );
}

// ---------------------------------------------------------------------------
// Archive discovery
// ---------------------------------------------------------------------------

#[test]
fn auto_generates_config_entries_when_archive_has_no_config() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(
        &plugins.join("My_Plugins.jar"),
        &[
            ("My_Plugin.class", b""),
            ("Sub_Dir/Third_Thing.class", b""),
            ("pack/Other_Thing.class", b""),
            ("_Hidden.class", b""),
            ("Sub/_Private_Thing.class", b""),
            ("Has$Inner_X.class", b""),
            ("Note.txt", b""),
        ],
    );

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    assert_eq!(labels(&catalog), vec!["My Plugin", "Third Thing"]);
    let my = plugin(&catalog, "My Plugin");
    assert_eq!(menu(my), Some(vec!["Plugins"]));
    assert_eq!(my.class_spec, "My_Plugin");
    assert_eq!(archive_path(my), plugins.join("My_Plugins.jar"));

    let third = plugin(&catalog, "Third Thing");
    // `autoGenerateConfigFile` turns path separators and underscores in the
    // submenu part into '>' and ' ' (unlike `installUserPlugin`).
    assert_eq!(menu(third), Some(vec!["Plugins", "Sub Dir"]));
    assert_eq!(third.class_spec, "Sub_Dir.Third_Thing");

    let report = archive_report(&catalog, "My_Plugins.jar");
    assert_eq!(report.config_origin, ConfigOrigin::AutoGenerated);
    assert_eq!(report.config_entry, None);
    assert_eq!(
        report.config_text,
        "Plugins, \"My Plugin\", My_Plugin\nPlugins>Sub Dir, \"Third Thing\", Sub_Dir.Third_Thing\n"
    );
    assert_eq!(report.plugins, vec![0, 1]);
}

#[test]
fn embedded_plugins_config_wins_over_auto_generation() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(
        &plugins.join("Config_Plugins.jar"),
        &[
            (
                "foo/plugins.config",
                b"# a comment\nPlugins>My Tools, \"Do Thing\", com.example.Do_Thing\n\"Top Level\", com.example.Top\nFile>Import, \"Import Widget\", com.example.Widget\n",
            ),
            ("Ignored_Auto.class", b""),
        ],
    );

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    assert_eq!(
        labels(&catalog),
        vec!["Do Thing", "Top Level", "Import Widget"]
    );
    assert_eq!(
        menu(plugin(&catalog, "Do Thing")),
        Some(vec!["Plugins", "My Tools"])
    );
    assert_eq!(menu(plugin(&catalog, "Top Level")), Some(vec!["Plugins"]));
    assert_eq!(
        menu(plugin(&catalog, "Import Widget")),
        Some(vec!["File", "Import"])
    );
    assert_eq!(
        plugin(&catalog, "Do Thing").class_spec,
        "com.example.Do_Thing"
    );

    let report = archive_report(&catalog, "Config_Plugins.jar");
    assert_eq!(report.config_origin, ConfigOrigin::Embedded);
    assert_eq!(report.config_entry.as_deref(), Some("foo/plugins.config"));
    assert_eq!(report.plugins, vec![0, 1, 2]);
    // No `Ignored_Auto` entry: ImageJ never auto-generates when a config exists.
    assert!(!labels(&catalog).contains(&"Ignored Auto"));
}

#[test]
fn archive_without_config_or_plugins_is_reported_as_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(
        &plugins.join("Resources_Only.jar"),
        &[("data/table.txt", b"payload"), ("NoUnderscore.class", b"")],
    );

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    assert!(catalog.plugins.is_empty());
    let report = archive_report(&catalog, "Resources_Only.jar");
    assert_eq!(report.config_origin, ConfigOrigin::Absent);
    assert_eq!(report.config_entry, None);
    assert!(report.config_text.is_empty());
    assert!(report.plugins.is_empty());
    // The archive still belongs on the classpath, like PluginClassLoader.
    assert!(
        catalog
            .classpath
            .contains(&plugins.join("Resources_Only.jar"))
    );
}

#[test]
fn unqualified_config_entries_use_the_archive_submenu() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(
        &plugins.join("Sub").join("Widget_Tool.jar"),
        &[(
            "plugins.config",
            b"\"Local Thing\", com.example.Local\nPlugins>Explicit, \"Explicit Thing\", com.example.Explicit\n",
        )],
    );

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    // `getSubmenuName` derives the submenu from the JAR's parent directory.
    assert_eq!(
        menu(plugin(&catalog, "Local Thing")),
        Some(vec!["Plugins", "Sub"])
    );
    assert_eq!(
        menu(plugin(&catalog, "Explicit Thing")),
        Some(vec!["Plugins", "Explicit"])
    );
    assert_eq!(
        archive_path(plugin(&catalog, "Local Thing")),
        plugins.join("Sub").join("Widget_Tool.jar")
    );
}

#[test]
fn malformed_archive_is_reported_without_aborting_the_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    touch(&plugins.join("Good_Plugin.class"));
    write_config(
        &plugins.join("Broken_Thing.jar"),
        "this is not a zip archive",
    );

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    assert_eq!(labels(&catalog), vec!["Good Plugin"]);
    assert!(catalog.archives.is_empty());
    assert_eq!(catalog.warnings.len(), 1);
    match &catalog.warnings[0] {
        DiscoveryWarning::ArchiveUnreadable { path, .. } => {
            assert_eq!(path, &plugins.join("Broken_Thing.jar"));
        }
        other => panic!("unexpected warning: {other:?}"),
    }
}

#[test]
fn malformed_config_lines_are_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(
        &plugins.join("Odd_Plugins.jar"),
        &[(
            "plugins.config",
            b"# comment\n\nbroken line without quotes\n\"No Comma\"\n\"-\", separator\nPlugins>, \"No Menu\", com.example.NoMenu\n",
        )],
    );

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    // The `Plugins>` menu spec is malformed (`firstComma <= 8`), so ImageJ
    // leaves `menu` null while still registering the command.
    assert_eq!(labels(&catalog), vec!["No Menu"]);
    let no_menu = plugin(&catalog, "No Menu");
    assert_eq!(no_menu.menu_path, None);
    assert_eq!(no_menu.class_spec, "com.example.NoMenu");

    let malformed: Vec<(usize, &str)> = catalog
        .warnings
        .iter()
        .filter_map(|w| match w {
            DiscoveryWarning::MalformedConfigLine { line, reason, .. } => {
                Some((*line, reason.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        malformed,
        vec![
            (3, "missing quoted menu label"),
            (4, "missing comma before class name"),
        ]
    );
}

#[test]
fn duplicate_labels_are_flagged() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    touch(&plugins.join("Dup_Plugin.class"));
    touch(&plugins.join("Sub").join("Dup_Plugin.class"));

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    assert_eq!(labels(&catalog), vec!["Dup Plugin", "Dup Plugin"]);
    // ImageJ's `pluginsTable` is global, so identical labels in different
    // menus are still duplicates.
    assert!(!catalog.plugins[0].duplicate);
    assert!(catalog.plugins[1].duplicate);
    assert_eq!(menu(&catalog.plugins[1]), Some(vec!["Plugins", "Sub"]));

    let duplicates: Vec<&DiscoveryWarning> = catalog
        .warnings
        .iter()
        .filter(|w| matches!(w, DiscoveryWarning::DuplicateCommand { .. }))
        .collect();
    assert_eq!(duplicates.len(), 1);
    match duplicates[0] {
        DiscoveryWarning::DuplicateCommand { label, .. } => assert_eq!(label, "Dup Plugin"),
        other => unreachable!("{other:?}"),
    }
}

#[test]
fn ordering_is_deterministic_and_sorted_like_java() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    // Created out of order on purpose: `File.list()` order is unspecified.
    touch(&plugins.join("b_Plugin.class"));
    touch(&plugins.join("A_Plugin.class"));
    touch(&plugins.join("Zed").join("Z_Plugin.class"));
    write_zip(&plugins.join("a_One.jar"), &[("One_Plugin.class", b"")]);
    write_zip(&plugins.join("B_Two.jar"), &[("Two_Plugin.class", b"")]);

    let first = PluginCatalog::scan(&plugins).unwrap();
    let second = PluginCatalog::scan(&plugins).unwrap();

    assert_eq!(first, second);
    assert_eq!(
        first
            .plugins
            .iter()
            .map(|p| p.class_spec.as_str())
            .collect::<Vec<_>>(),
        vec![
            "A_Plugin",
            "Zed.Z_Plugin",
            "b_Plugin",
            "Two_Plugin",
            "One_Plugin"
        ]
    );
    assert_eq!(
        first
            .archives
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().to_string())
            .collect::<Vec<_>>(),
        vec!["B_Two.jar", "a_One.jar"]
    );
}

#[test]
fn classpath_entries_are_absolute_and_deterministic() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    touch(&plugins.join("Top_Plugin.class"));
    touch(&plugins.join("Zed").join("Z_Plugin.class"));
    touch(&plugins.join("Alpha").join("A_Plugin.class"));
    write_zip(&plugins.join("Top_Jar.jar"), &[("Jar_Plugin.class", b"")]);
    write_zip(
        &plugins.join("Alpha").join("Alpha_Jar.jar"),
        &[("Alpha_Plugin.class", b"")],
    );

    let catalog = PluginCatalog::scan(&plugins).unwrap();

    assert!(catalog.classpath.iter().all(|p| p.is_absolute()));
    assert_eq!(
        catalog
            .classpath
            .iter()
            .map(|p| {
                p.strip_prefix(&plugins)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| p.to_string_lossy().to_string())
            })
            .collect::<Vec<_>>(),
        vec!["", "Alpha", "Zed", "Alpha/Alpha_Jar.jar", "Top_Jar.jar"]
    );
}

#[test]
fn scanner_review_includes_dependency_archives_without_menu_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(&plugins.join("library.jar"), &[("dependency/Helper.class", b"")]);
    write_zip(&plugins.join("Sub").join("support.zip"), &[("Helper.class", b"")]);
    write_zip(&plugins.join("Main_Plugin.jar"), &[("Main_Plugin.class", b"")]);

    let catalog = PluginCatalog::scan(&plugins).unwrap();
    assert!(catalog.classpath.contains(&plugins.join("library.jar")));
    assert!(catalog.classpath.contains(&plugins.join("Sub").join("support.zip")));
    assert_eq!(catalog.archives.len(), 1);
    assert_eq!(catalog.archives[0].path, plugins.join("Main_Plugin.jar"));
}

#[test]
fn scanner_review_splits_nested_menus_and_removes_trailing_separator() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(
        &plugins.join("Nested_Plugin.jar"),
        &[(
            "plugins.config",
            b"Plugins>Analysis>Morphology, \"Dilate\", my.Dilate\nFile>Save As>, \"Format\", my.Format\n",
        )],
    );
    let catalog = PluginCatalog::scan(&plugins).unwrap();
    assert_eq!(menu(plugin(&catalog, "Dilate")), Some(vec!["Plugins", "Analysis", "Morphology"]));
    assert_eq!(menu(plugin(&catalog, "Format")), Some(vec!["File", "Save As"]));
}

#[test]
fn scanner_review_duplicate_detection_uses_java_shortcut_command() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(
        &plugins.join("Shortcut_Plugin.jar"),
        &[(
            "plugins.config",
            b"Plugins>Tools, \"Capture[c]\", a.Capture\nPlugins>Tools, \"Capture\", b.Capture\nPlugins>Tools, \"Capture[F1]\", c.Capture\n",
        )],
    );
    let catalog = PluginCatalog::scan(&plugins).unwrap();
    assert!(!catalog.plugins[0].duplicate);
    assert!(catalog.plugins[1].duplicate);
    assert!(!catalog.plugins[2].duplicate);
}

#[test]
fn scanner_review_reports_plugin_limit_once() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    for name in ["A_Plugin.jar", "B_Plugin.jar"] {
        write_zip(
            &plugins.join(name),
            &[("plugins.config", b"\"One\", a.One\n\"Two\", a.Two\n")],
        );
    }
    let options = ScanOptions {
        max_plugins: 1,
        ..Default::default()
    };
    let catalog = PluginCatalog::scan_with_options(&plugins, &options).unwrap();
    assert_eq!(
        catalog
            .warnings
            .iter()
            .filter(|warning| matches!(warning, DiscoveryWarning::PluginLimitReached { .. }))
            .count(),
        1
    );
}

// ---------------------------------------------------------------------------
// Bounded reads
// ---------------------------------------------------------------------------

#[test]
fn config_read_is_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(
        &plugins.join("Long_Config.jar"),
        &[(
            "plugins.config",
            b"\"One Thing\", com.example.One\n\"Two Thing\", com.example.Two\n",
        )],
    );

    let options = ScanOptions {
        max_config_bytes: 35,
        ..ScanOptions::default()
    };
    let catalog = PluginCatalog::scan_with_options(&plugins, &options).unwrap();

    assert_eq!(labels(&catalog), vec!["One Thing"]);
    let report = archive_report(&catalog, "Long_Config.jar");
    assert!(report.config_truncated);
    assert!(catalog.warnings.iter().any(|w| matches!(
        w,
        DiscoveryWarning::ConfigTruncated { limit, .. } if *limit == 35
    )));
}

#[test]
fn archive_entry_scan_is_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    write_zip(
        &plugins.join("Many_Plugins.jar"),
        &[
            ("One_Plugin.class", b""),
            ("Two_Plugin.class", b""),
            ("Three_Plugin.class", b""),
        ],
    );

    let options = ScanOptions {
        max_archive_entries: 2,
        ..ScanOptions::default()
    };
    let catalog = PluginCatalog::scan_with_options(&plugins, &options).unwrap();

    assert_eq!(labels(&catalog), vec!["One Plugin", "Two Plugin"]);
    assert!(catalog.warnings.iter().any(|w| matches!(
        w,
        DiscoveryWarning::ArchiveEntryLimit { limit, .. } if *limit == 2
    )));
}

#[test]
fn plugin_limit_is_enforced() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    touch(&plugins.join("One_Plugin.class"));
    touch(&plugins.join("Two_Plugin.class"));

    let options = ScanOptions {
        max_plugins: 1,
        ..ScanOptions::default()
    };
    let catalog = PluginCatalog::scan_with_options(&plugins, &options).unwrap();

    assert_eq!(labels(&catalog), vec!["One Plugin"]);
    assert!(
        catalog
            .warnings
            .iter()
            .any(|w| matches!(w, DiscoveryWarning::PluginLimitReached { limit } if *limit == 1))
    );
}

// ---------------------------------------------------------------------------
// Differential evidence from the unmodified ImageJ 1.54u classes
// ---------------------------------------------------------------------------

const HARNESS_SOURCE: &str = r#"
package ij;

import java.io.*;
import java.lang.reflect.*;
import java.util.*;

/**
 * Read-only probe for ImageJ 1.54u plugin discovery. Runs in package `ij` so it
 * can call the package-private Menus helpers, and never loads or instantiates a
 * plugin class.
 */
public class DiscoveryHarness {
    public static void main(String[] args) throws Exception {
        Prefs.ImageJDir = System.getProperty("discovery.root");
        Menus m = new Menus(null, null);
        m.setupPluginsAndMacrosPaths();

        String[] plugins = Menus.getPlugins();
        System.out.println("PLUGINS_BEGIN");
        if (plugins != null) for (String p : plugins) System.out.println(p);
        System.out.println("PLUGINS_END");

        Field jarField = Menus.class.getDeclaredField("jarFiles");
        jarField.setAccessible(true);
        Vector jars = (Vector) jarField.get(null);
        if (jars != null) for (Object o : jars) {
            String jar = (String) o;
            System.out.println("JAR_BEGIN " + jar);
            InputStream is = m.getConfigurationFile(jar);
            if (is == null) System.out.println("CONFIG_NULL");
            else {
                BufferedReader r = new BufferedReader(new InputStreamReader(is, "UTF-8"));
                String line;
                while ((line = r.readLine()) != null) System.out.println("L:" + line);
            }
            System.out.println("JAR_END");
        }

        m.installJarPlugins();
        Field entryField = Menus.class.getDeclaredField("menuEntry2jarFile");
        entryField.setAccessible(true);
        Map entries = (Map) entryField.get(m);
        List<String> labels = new ArrayList<>();
        for (Object key : entries.keySet()) labels.add(key + "\t" + entries.get(key));
        Collections.sort(labels);
        System.out.println("LABELS_BEGIN");
        for (String s : labels) System.out.println(s);
        System.out.println("LABELS_END");
    }
}
"#;

#[derive(Debug, Default)]
struct JavaDiscovery {
    class_entries: Vec<String>,
    /// Archive path -> config lines, or `None` when `getConfigurationFile`
    /// returned null (unreadable archive).
    archives: Vec<(PathBuf, Option<Vec<String>>)>,
    /// `menuEntry2jarFile` entries: (label, archive).
    labels: Vec<(String, PathBuf)>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn run_java_discovery_harness(root: &Path) -> JavaDiscovery {
    let oracle_dir = repo_root().join("tools/oracle");
    let oracle_jar = oracle_dir.join("ij-oracle.jar");
    let shim_jar = oracle_dir.join("java-applet-shim.jar");
    assert!(oracle_jar.is_file(), "missing {}", oracle_jar.display());
    assert!(shim_jar.is_file(), "missing {}", shim_jar.display());

    let work = root.join("harness");
    let src_dir = work.join("src/ij");
    let classes = work.join("classes");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&classes).unwrap();
    fs::write(src_dir.join("DiscoveryHarness.java"), HARNESS_SOURCE).unwrap();

    let patch = format!("java.desktop={}", shim_jar.display());
    let compile = Command::new("javac")
        .arg("--patch-module")
        .arg(&patch)
        .arg("--add-exports")
        .arg("java.desktop/java.applet=ALL-UNNAMED")
        .arg("-cp")
        .arg(&oracle_jar)
        .arg("-d")
        .arg(&classes)
        .arg(src_dir.join("DiscoveryHarness.java"))
        .output()
        .expect("run javac");
    assert!(
        compile.status.success(),
        "javac failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new("java")
        .arg("-Djava.awt.headless=true")
        .arg("-Dapple.awt.UIElement=true")
        .arg("--patch-module")
        .arg(&patch)
        .arg("--add-exports")
        .arg("java.desktop/java.applet=ALL-UNNAMED")
        .arg(format!("-Ddiscovery.root={}", root.display()))
        .arg("-cp")
        .arg(format!("{}:{}", classes.display(), oracle_jar.display()))
        .arg("ij.DiscoveryHarness")
        .output()
        .expect("run java discovery harness");
    assert!(
        run.status.success(),
        "java harness failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let stdout = String::from_utf8_lossy(&run.stdout).to_string();
    let mut discovery = JavaDiscovery::default();
    let mut section = "";
    let mut current_jar: Option<PathBuf> = None;
    let mut current_lines: Option<Vec<String>> = None;
    for line in stdout.lines() {
        match line {
            "PLUGINS_BEGIN" => section = "plugins",
            "PLUGINS_END" => section = "",
            "LABELS_BEGIN" => section = "labels",
            "LABELS_END" => section = "",
            "JAR_END" => {
                if let Some(jar) = current_jar.take() {
                    discovery.archives.push((jar, current_lines.take()));
                }
                section = "";
            }
            _ if line.starts_with("JAR_BEGIN ") => {
                current_jar = Some(PathBuf::from(&line["JAR_BEGIN ".len()..]));
                current_lines = Some(Vec::new());
                section = "config";
            }
            "CONFIG_NULL" => current_lines = None,
            _ if section == "plugins" => discovery.class_entries.push(line.to_string()),
            _ if section == "config" && line.starts_with("L:") => {
                current_lines
                    .as_mut()
                    .expect("config lines after CONFIG_NULL")
                    .push(line[2..].to_string());
            }
            _ if section == "labels" => {
                let (label, jar) = line.split_once('\t').expect("label line");
                discovery
                    .labels
                    .push((label.to_string(), PathBuf::from(jar)));
            }
            _ => {}
        }
    }
    discovery
}

#[test]
fn differential_against_imagej_154u_discovery() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let plugins = root.join("plugins");

    // Class files: top-level, one-level subdirectory, and entries ImageJ
    // filters out (no underscore, `$` inner class, nested directory).
    touch(&plugins.join("Top_Plugin.class"));
    touch(&plugins.join("plain.class"));
    touch(&plugins.join("_Leading.class"));
    touch(&plugins.join("With$Inner_Thing.class"));
    touch(&plugins.join("Upper_Thing.CLASS"));
    touch(&plugins.join("Sub").join("Tool_A.class"));
    touch(&plugins.join("Sub").join("NoUnderscore.class"));
    touch(
        &plugins
            .join("Deep")
            .join("Inner")
            .join("Nested_Thing.class"),
    );

    // Archive without plugins.config: auto-generated entries.
    write_zip(
        &plugins.join("My_Plugins.jar"),
        &[
            ("My_Plugin.class", b""),
            ("Sub_Dir/Third_Thing.class", b""),
            ("pack/Other_Thing.class", b""),
            ("_Hidden.class", b""),
            ("Sub/_Private_Thing.class", b""),
            ("Has$Inner_X.class", b""),
            ("Note.txt", b""),
        ],
    );
    // Archive with an embedded plugins.config, in a first-level subdirectory.
    write_zip(
        &plugins.join("Sub").join("Widget_Tool.jar"),
        &[(
            "foo/plugins.config",
            b"# comment\nPlugins>My Tools, \"Do Thing\", com.example.Do_Thing\n\"Local Thing\", com.example.Local\nFile>Import, \"Import Widget\", com.example.Widget\n",
        )],
    );
    // Unreadable archive.
    write_config(&plugins.join("Broken_Thing.jar"), "not a zip archive");

    let java = run_java_discovery_harness(root);
    let catalog = PluginCatalog::scan(&plugins).unwrap();

    // 1. Class list: `getPlugins()` returns sorted, unqualified entries.
    assert_eq!(catalog_class_entries(&catalog), java.class_entries);
    assert_eq!(
        java.class_entries,
        vec!["Sub/Tool_A", "Top_Plugin", "_Leading"]
    );

    // 2. Archive config text: `getConfigurationFile` (embedded or generated).
    //    `ConfigOrigin::Absent` archives have no Java config (null) and are
    //    therefore absent from the Java map.
    let mut ours: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for report in &catalog.archives {
        if report.config_origin == ConfigOrigin::Absent {
            continue;
        }
        ours.insert(
            report.path.display().to_string(),
            report.config_text.lines().map(str::to_string).collect(),
        );
    }
    let mut java_configs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut java_unreadable = Vec::new();
    for (path, lines) in &java.archives {
        match lines {
            Some(lines) => {
                java_configs.insert(path.display().to_string(), lines.clone());
            }
            None => java_unreadable.push(path.clone()),
        }
    }
    assert_eq!(ours, java_configs);
    assert_eq!(java_unreadable, vec![plugins.join("Broken_Thing.jar")]);
    assert_eq!(catalog.archives.len(), 2);

    // 3. Menu labels: `installJarPlugin` -> `menuEntry2jarFile`.
    let mut ours_labels: Vec<(String, String)> = catalog
        .plugins
        .iter()
        .filter_map(|p| match &p.source {
            PluginSource::Archive { archive, .. } => {
                Some((p.label.clone(), archive.display().to_string()))
            }
            PluginSource::ClassFile { .. } => None,
        })
        .collect();
    ours_labels.sort();
    let mut java_labels: Vec<(String, String)> = java
        .labels
        .iter()
        .map(|(label, jar)| (label.clone(), jar.display().to_string()))
        .collect();
    java_labels.sort();
    assert_eq!(ours_labels, java_labels);

    // Malformed archive surfaced as a warning instead of an entry.
    assert!(catalog.warnings.iter().any(|w| matches!(
        w,
        DiscoveryWarning::ArchiveUnreadable { path, .. }
            if path == &plugins.join("Broken_Thing.jar")
    )));
}

// ---------------------------------------------------------------------------
// Explicit opt-in class inspection (phase 2)
// ---------------------------------------------------------------------------

fn fixtures_jar() -> PathBuf {
    repo_root().join("tools/plugin-host/fixtures/fixtures.jar")
}

fn copy_fixture_jar(plugins: &Path) {
    // `locate_from_repo_root` builds the fixture jar on a clean checkout
    // before it can be copied into the temporary plugins directory.
    let _ = inspection_bridge();
    fs::create_dir_all(plugins).unwrap();
    // The underscore makes the scanner treat the fixture archive as a
    // classpath archive. It has no plugins.config and no `_` class names, so
    // it contributes classes but no menu entries.
    fs::copy(fixtures_jar(), plugins.join("Fixtures_Classes.jar")).unwrap();
}

fn inspection_bridge() -> PluginBridge {
    PluginBridge::locate_from_repo_root(repo_root()).expect("locate plugin bridge")
}

fn assert_markers_absent(work: &Path) {
    for marker in [
        "inspection-static-initializer.marker",
        "inspection-setup.marker",
        "inspection-run.marker",
    ] {
        let path = work.join(marker);
        assert!(
            !path.exists(),
            "inspection executed plugin code: {} exists",
            path.display()
        );
    }
}

#[test]
fn inspection_loads_the_class_without_initializing_or_running_it() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();

    let request = PluginInspectionRequest::new("fixtures.InspectionProbeFilter")
        .with_classpath_entry(fixtures_jar())
        .with_work_dir(&work);
    let result = inspection_bridge()
        .inspect_plugin(&request)
        .expect("inspect fixture");

    assert!(
        matches!(
            result.status,
            PluginInspectionStatus::Inspected {
                kind: PluginInspectionKind::PlugInFilter,
                ..
            }
        ),
        "unexpected inspection status: {:#?}",
        result.status
    );
    // `Class.forName(name, false, loader)` loads and links the class but must
    // not run its static initializer, and inspection must never instantiate
    // it, so setup()/run() cannot run either.
    assert_markers_absent(&work);
}

#[test]
fn catalog_inspection_classifies_entries_without_running_them() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins = tmp.path().join("plugins");
    copy_fixture_jar(&plugins);
    write_zip(
        &plugins.join("Probe_Plugins.jar"),
        &[(
            "plugins.config",
            b"Plugins, \"Probe Filter\", fixtures.InspectionProbeFilter(\"inspect-arg\")\n\
              Plugins, \"Simple Plugin\", fixtures.SimplePlugIn\n\
              Plugins, \"Extended Filter\", fixtures.ExtendedProbeFilter\n\
              Plugins, \"Ambiguous Probe\", fixtures.AmbiguousProbe\n\
              Plugins, \"Plain Class\", fixtures.PlainClassProbe\n\
              Plugins, \"Missing Class\", fixtures.DoesNotExist\n\
              Plugins, \"Internal Filter\", ij.plugin.filter.GaussianBlur\n",
        )],
    );

    let mut catalog = PluginCatalog::scan(&plugins).unwrap();
    // Scanning alone stays read-only; classification requires opting in.
    assert!(
        catalog
            .plugins
            .iter()
            .all(|p| p.kind == PluginKind::Uninspected && p.inspection.is_none())
    );

    let summary = catalog.inspect_plugins_with_options(
        &inspection_bridge(),
        &InspectionOptions {
            timeout: Duration::from_secs(60),
            ..InspectionOptions::default()
        },
    );

    assert_eq!(summary.inspected, 7, "summary: {summary:#?}");
    assert_eq!(summary.skipped, 0);
    assert_eq!(summary.supported, 1);
    assert_eq!(summary.unsupported, 4);
    assert_eq!(summary.internal, 1);
    assert_eq!(summary.load_failed, 1);

    assert_eq!(plugin(&catalog, "Probe Filter").kind, PluginKind::PlugInFilter);
    // The `("inspect-arg")` suffix is removed like `Executer.runCommand`; only
    // the binary class name reaches `Class.forName`.
    assert_eq!(
        plugin(&catalog, "Probe Filter")
            .inspection
            .as_ref()
            .map(|i| i.class_name.as_str()),
        Some("fixtures.InspectionProbeFilter")
    );
    assert_eq!(plugin(&catalog, "Simple Plugin").kind, PluginKind::PlugIn);
    assert_eq!(
        plugin(&catalog, "Extended Filter").kind,
        PluginKind::ExtendedPlugInFilter
    );
    assert_eq!(
        plugin(&catalog, "Ambiguous Probe").kind,
        PluginKind::Ambiguous
    );
    assert_eq!(plugin(&catalog, "Plain Class").kind, PluginKind::NotAPlugin);
    assert_eq!(
        plugin(&catalog, "Internal Filter").kind,
        PluginKind::Internal
    );

    let missing = plugin(&catalog, "Missing Class");
    assert_eq!(missing.kind, PluginKind::LoadFailed);
    let inspection = missing.inspection.as_ref().expect("structured inspection");
    assert_eq!(inspection.class_name, "fixtures.DoesNotExist");
    assert!(
        matches!(
            inspection.failure,
            Some(PluginInspectionFailure::ClassNotFound { .. })
        ),
        "unexpected failure: {:#?}",
        inspection.failure
    );
    // Every inspected entry records the class it actually looked up.
    assert_eq!(
        plugin(&catalog, "Internal Filter")
            .inspection
            .as_ref()
            .map(|i| i.class_name.as_str()),
        Some("ij.plugin.filter.GaussianBlur")
    );
}

#[test]
fn inspection_rejects_malformed_class_names_without_a_host_process() {
    let bridge = inspection_bridge();
    for name in [
        "",
        "   ",
        " fixtures.Bad",
        "fixtures.Bad ",
        "fixtures..Bad",
        ".fixtures.Bad",
        "fixtures.Bad.",
        "fixtures.Bad Name",
        "fixtures/Bad",
        "fixtures.Bad('x')",
        "-Dfixtures.Bad",
        "macro:fixtures.Bad",
        "1fixtures.Bad",
    ] {
        let request = PluginInspectionRequest::new(name).with_classpath_entry(fixtures_jar());
        let result = bridge.inspect_plugin(&request).expect("inspect name");
        assert!(
            matches!(result.status, PluginInspectionStatus::InvalidClassName { .. }),
            "class name {name:?} should be rejected, got {:#?}",
            result.status
        );
        // The reported identity is the unmodified input, never a command.
        assert_eq!(result.class_name, name);
    }
}

#[test]
fn internal_classes_are_classified_without_launching_the_host() {
    let request = PluginInspectionRequest::new("ij.plugin.filter.GaussianBlur");
    let result = inspection_bridge()
        .inspect_plugin(&request)
        .expect("inspect internal class");
    assert!(
        matches!(
            result.status,
            PluginInspectionStatus::Inspected {
                kind: PluginInspectionKind::Internal,
                ..
            }
        ),
        "unexpected status: {:#?}",
        result.status
    );
}

/// Reconstructs the `Menus.getPlugins()` class list from the catalog.
fn catalog_class_entries(catalog: &PluginCatalog) -> Vec<String> {
    catalog
        .plugins
        .iter()
        .filter_map(|p| match &p.source {
            PluginSource::ClassFile { path } => {
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                match &p.menu_path {
                    Some(menu) if menu.len() > 1 => Some(format!("{}/{}", menu[1], name)),
                    _ => Some(name),
                }
            }
            PluginSource::Archive { .. } => None,
        })
        .collect()
}
