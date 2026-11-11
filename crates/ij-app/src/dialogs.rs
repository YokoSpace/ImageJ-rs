use ij_bridge::plugin_discovery::{DiscoveredPlugin, PluginCatalog, PluginKind, PluginSource};
use ij_session::ImageId;
use std::path::PathBuf;

/// Editable state of `Image > Duplicate...`.
///
/// `source` pins the image that was active when the dialog opened. Duplicating
/// after the focus moved must not fall through to whichever image happens to be
/// active when OK is pressed. `stack_size` and `range` follow Duplicator's
/// dialog: the Range field is shown only for stacks and defaults to
/// `1-<stackSize>` (original_imagej/ij/plugin/Duplicator.java 485-521).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DuplicateDialog {
    pub open: bool,
    pub source: Option<ImageId>,
    pub title: String,
    pub duplicate_stack: bool,
    pub range: String,
    pub stack_size: u32,
}

/// Editable state of `Analyze > Set Scale...`.
///
/// The numeric fields mirror ScaleDialog's four `GenericDialog` inputs
/// (original_imagej/ij/plugin/filter/ScaleDialog.java 66-71). `raw_length` is
/// the uncalibrated line length or largest rectangle side used by the
/// "Click to Remove Scale" button (ScaleDialog.java 38-48, 182-203), and
/// `removed` suppresses a redundant Set Scale command when OK follows that
/// button without further edits.
#[derive(Debug, Clone, PartialEq)]
pub struct SetScaleDialog {
    pub open: bool,
    pub distance: f64,
    pub known: f64,
    pub aspect: f64,
    pub unit: String,
    pub raw_length: f64,
    pub removed: bool,
}

impl Default for SetScaleDialog {
    fn default() -> Self {
        Self {
            open: false,
            distance: 0.0,
            known: 0.0,
            aspect: 1.0,
            unit: "pixel".to_string(),
            raw_length: 0.0,
            removed: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DialogState {
    pub show_about: bool,
    pub show_gaussian_blur: bool,
    pub gaussian_blur_sigma: f64,
    pub show_brightness_contrast: bool,
    pub bc_min: f64,
    pub bc_max: f64,
    pub show_threshold: bool,
    pub threshold_method: String,
    pub threshold_dark_bg: bool,
    /// ThresholdAdjuster's "16-bit histogram" checkbox; only meaningful for
    /// 16-bit images.
    pub threshold_16bit: bool,
    pub last_error: Option<String>,
    pub show_plugin_filter: bool,
    pub plugin_filter_class: String,
    pub plugin_filter_arg: String,
    pub plugin_filter_classpath: Vec<PathBuf>,
    pub plugin_filter_classpath_input: String,
    pub plugin_filter_target_id: Option<ImageId>,
    pub duplicate: DuplicateDialog,
    /// Duplicator's `staticDuplicateStack`: the checkbox starts checked again
    /// on the next stack duplicate (Duplicator.java 56, 528).
    pub duplicate_stack_default: bool,
}

impl DialogState {
    pub fn new() -> Self {
        Self {
            show_about: false,
            show_gaussian_blur: false,
            gaussian_blur_sigma: 2.0,
            show_brightness_contrast: false,
            bc_min: 0.0,
            bc_max: 255.0,
            show_threshold: false,
            threshold_method: "IsoData".into(),
            threshold_dark_bg: false,
            threshold_16bit: false,
            last_error: None,
            show_plugin_filter: false,
            plugin_filter_class: String::new(),
            plugin_filter_arg: String::new(),
            plugin_filter_classpath: Vec::new(),
            plugin_filter_classpath_input: String::new(),
            plugin_filter_target_id: None,
            duplicate: DuplicateDialog::default(),
            duplicate_stack_default: false,
        }
    }
}

/// Lifecycle of the installed-plugin catalog browser.
///
/// `Scanning` and `Inspecting` are driven by a worker thread in `app.rs`; the
/// UI model only records what the last completed/started job was so that a
/// stale worker result can be rejected by generation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PluginCatalogPhase {
    #[default]
    Idle,
    Scanning,
    Ready,
    Failed,
}

/// Explicit inspection state for one selected catalog entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PluginInspectPhase {
    #[default]
    Idle,
    Inspecting {
        index: usize,
    },
    Cancelling {
        index: usize,
    },
    Done {
        index: usize,
    },
    Failed {
        index: usize,
        message: String,
    },
}

/// UI-only state of the Plugins > Installed Plugins browser.
#[derive(Debug, Clone, Default)]
pub struct PluginBrowserState {
    pub show: bool,
    /// Raw text from the directory field. Kept as text because the user may
    /// still be typing; `Scan` resolves it like ImageJ's `plugins.dir`.
    pub directory_input: String,
    pub phase: PluginCatalogPhase,
    /// Last successfully scanned catalog, if one exists. A failed rescan keeps
    /// this old catalog visible only when the phase is `Ready`; on failure the
    /// app clears it so entries are never silently stale.
    pub catalog: Option<PluginCatalog>,
    pub selected: Option<usize>,
    pub scan_error: Option<String>,
    /// One-line user-facing status for the browser (scan/inspection/cancel).
    pub status: Option<String>,
    pub inspect_phase: PluginInspectPhase,
    /// Per-entry source/classpath validation for a catalog restored from the
    /// JSON cache. `None` means the source and classpath were present when the
    /// cache was loaded; `Some(reason)` disables the entry without pretending
    /// the inspection result is still executable.
    pub source_block_reasons: Vec<Option<String>>,
}

impl PluginBrowserState {
    pub fn is_busy(&self) -> bool {
        self.phase == PluginCatalogPhase::Scanning
            || matches!(
                self.inspect_phase,
                PluginInspectPhase::Inspecting { .. } | PluginInspectPhase::Cancelling { .. }
            )
    }
}

/// Short badge text used in the browser rows. Kept deliberately literal so an
/// uninspected or unsupported entry cannot look runnable.
pub fn plugin_kind_badge(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Uninspected => "Uninspected",
        PluginKind::PlugInFilter => "PlugInFilter",
        PluginKind::PlugIn => "PlugIn",
        PluginKind::ExtendedPlugInFilter => "ExtendedPlugInFilter",
        PluginKind::Ambiguous => "Ambiguous",
        PluginKind::Internal => "Internal",
        PluginKind::NotAPlugin => "NotAPlugin",
        PluginKind::LoadFailed => "LoadFailed",
        // `PluginKind` is `#[non_exhaustive]`; a future variant must not be
        // mistaken for a supported filter.
        _ => "Unsupported",
    }
}

/// Human-readable limitation note for each classification.
pub fn plugin_kind_support_note(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Uninspected => {
            "Not inspected: no class file was read and no JVM was started. Use Inspect Selected."
        }
        PluginKind::PlugInFilter => {
            "Verified plain PlugInFilter. It may only prefill the narrow image-filter dialog."
        }
        PluginKind::PlugIn => {
            "PlugIn has no image-filter path; it is unsupported by the narrow runner."
        }
        PluginKind::ExtendedPlugInFilter => {
            "ExtendedPlugInFilter needs dialogs/preview/stacks and is unsupported."
        }
        PluginKind::Ambiguous => {
            "Implements both PlugIn and PlugInFilter; dispatch is ambiguous and it is unsupported."
        }
        PluginKind::Internal => "Internal ij.* class; not a user plug-in candidate.",
        PluginKind::NotAPlugin => "The class implements neither PlugIn nor PlugInFilter.",
        PluginKind::LoadFailed => "The class could not be validated or loaded; see the detail.",
        _ => "Unknown classification; treated as unsupported.",
    }
}

/// True only for entries that have been explicitly verified as plain
/// `PlugInFilter`. This is the single gate used by prefill/run.
pub fn plugin_kind_is_prefillable(kind: PluginKind) -> bool {
    kind.is_supported_filter()
}

/// Menu path as displayed in the browser. `None` mirrors ImageJ's malformed
/// `Plugins>` spec: the entry exists but was never added to a menu.
pub fn plugin_menu_display(plugin: &DiscoveredPlugin) -> String {
    match &plugin.menu_path {
        Some(path) if !path.is_empty() => path.join(">"),
        _ => "<no menu>".to_string(),
    }
}

/// Row text for the scrollable catalog list.
///
/// The source is included because duplicate labels are legal in ImageJ's
/// global `pluginsTable`; two rows may share a label but must remain visually
/// distinct and selectable by index.
pub fn plugin_row_text(
    plugin: &DiscoveredPlugin,
    conflicting_label: bool,
    selected: bool,
) -> String {
    let marker = if selected { "▶" } else { " " };
    let duplicate = if conflicting_label {
        "  [duplicate label]"
    } else {
        ""
    };
    format!(
        "{marker} {}  [{}]{}  — {} — {}",
        plugin.label,
        plugin_kind_badge(plugin.kind),
        duplicate,
        plugin_menu_display(plugin),
        plugin.source
    )
}

/// Counts exact labels so the browser can mark every conflicting row, not just
/// the later one flagged by `Menus.pluginsTable`.
pub fn plugin_label_counts(catalog: &PluginCatalog) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::new();
    for plugin in &catalog.plugins {
        *counts.entry(plugin.label.clone()).or_insert(0) += 1;
    }
    counts
}

/// Splits an ImageJ class spec into class name and optional argument, matching
/// `Executer.runCommand`: only a trailing `("argument")` is removed and only
/// when the class name would remain non-empty.
pub fn split_plugin_class_spec(spec: &str) -> (String, Option<String>) {
    let trimmed = spec.trim_start();
    if trimmed.ends_with("\")") {
        if let Some(index) = trimmed.rfind("(\"") {
            if index > 0 {
                return (
                    trimmed[..index].to_string(),
                    Some(trimmed[index + 2..trimmed.len() - 2].to_string()),
                );
            }
        }
    }
    (trimmed.to_string(), None)
}

/// Validates that a catalog entry still has a real source file and that every
/// classpath entry it was inspected with still exists.
///
/// A cached inspection only proves what the class was when it was inspected.
/// After a reload the files may be gone, so the entry must stay visible but
/// disabled instead of looking like a verified executable.
pub fn plugin_source_block_reason(
    plugin: &DiscoveredPlugin,
    classpath: &[PathBuf],
) -> Option<String> {
    plugin_source_file_missing_reason(plugin).or_else(|| plugin_classpath_missing_reason(classpath))
}

/// Source-file half of [`plugin_source_block_reason`]. Split out so a cache
/// load can validate the classpath once instead of once per entry.
pub fn plugin_source_file_missing_reason(plugin: &DiscoveredPlugin) -> Option<String> {
    match &plugin.source {
        PluginSource::ClassFile { path } => {
            if !path.is_file() {
                return Some(format!(
                    "Source class file no longer exists: {}. Re-scan the plugins directory.",
                    path.display()
                ));
            }
        }
        PluginSource::Archive { archive, .. } => {
            if !archive.is_file() {
                return Some(format!(
                    "Source archive no longer exists: {}. Re-scan the plugins directory.",
                    archive.display()
                ));
            }
        }
    }
    None
}

/// Classpath half of [`plugin_source_block_reason`].
pub fn plugin_classpath_missing_reason(classpath: &[PathBuf]) -> Option<String> {
    if classpath.is_empty() {
        return Some("The plugin classpath is empty. Re-scan the plugins directory.".to_string());
    }
    if let Some(missing) = classpath.iter().find(|path| !path.exists()) {
        return Some(format!(
            "Plugin classpath entry no longer exists: {}. Re-scan the plugins directory.",
            missing.display()
        ));
    }
    None
}

/// Returns why the narrow PlugInFilter dialog may not be prefilled from the
/// selected catalog entry, or `None` when prefill is allowed.
///
/// This function is pure so the same rule can be asserted without rendering.
pub fn plugin_prefill_block_reason(
    plugin: Option<&DiscoveredPlugin>,
    bridge_available: bool,
    filter_running: bool,
    target: Option<ImageId>,
    preflight_error: Option<&str>,
) -> Option<String> {
    let plugin = match plugin {
        Some(plugin) => plugin,
        None => return Some("Select a catalog entry first.".to_string()),
    };
    if plugin.kind == PluginKind::Uninspected {
        return Some(
            "This entry is Uninspected. Press Inspect Selected before using it.".to_string(),
        );
    }
    if !plugin_kind_is_prefillable(plugin.kind) {
        return Some(format!(
            "Only a verified PlugInFilter can prefill the narrow runner; this entry is {}.",
            plugin_kind_badge(plugin.kind)
        ));
    }
    if filter_running {
        return Some("A Java PlugInFilter is already executing.".to_string());
    }
    if !bridge_available {
        return Some("The Java plugin bridge is not configured.".to_string());
    }
    let target = match target {
        Some(target) => target,
        None => {
            return Some(
                "Open and activate an image first; prefill pins that image as the target."
                    .to_string(),
            );
        }
    };
    if let Some(error) = preflight_error {
        return Some(format!(
            "The active image (ID {}) is not compatible with the narrow PlugInFilter subset: {error}",
            target.0
        ));
    }
    None
}
