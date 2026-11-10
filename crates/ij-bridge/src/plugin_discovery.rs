//! Read-only discovery of classic ImageJ 1.x user plugins.
//!
//! This is a faithful, deliberately bounded port of the *discovery* half of
//! `Menus.installPlugins()` from ImageJ 1.54u
//! (`original_imagej/ij/Menus.java`): `getPlugins`, `checkSubdirectory`,
//! `getConfigurationFile`, `autoGenerateConfigFile` and the menu/label/class
//! parsing performed by `installJarPlugin`.
//!
//! [`PluginCatalog::scan`] is pure and read-only: it never starts a JVM, never
//! reads class file contents and never instantiates a plugin, so every entry is
//! reported with [`PluginKind::Uninspected`]. Classification is a separate,
//! explicit opt-in step ([`PluginCatalog::inspect_plugins`]) that delegates to
//! the bounded [`PluginBridge`] host. The host loads each class with
//! `Class.forName(name, false, loader)`, checks interface assignability and
//! never runs a static initializer, constructor, `setup` or `run`.
//!
//! # Coverage
//!
//! * `.class` files whose name contains `_` in the plugins directory and in its
//!   immediate subdirectories (excluding `$` inner classes), matching
//!   `Menus.getPlugins` / `Menus.checkSubdirectory`.
//! * `.jar`/`.zip` files whose name contains `_` in the same two levels,
//!   matching the archive collection in `Menus.getPlugins`.
//! * The first archive entry whose name ends with `plugins.config`, otherwise
//!   the entries ImageJ would synthesize in `autoGenerateConfigFile`.
//! * Menu paths, menu labels and class names as parsed by
//!   `Menus.installJarPlugin`, including `Plugins>Sub, ...`, unqualified
//!   `"Label", class` entries (which use the archive's submenu), and
//!   `File>Import, ...` style menu paths.
//! * Deterministic ordering, duplicate labels, and diagnostics for unreadable
//!   archives and malformed configuration lines.
//! * Absolute classpath entries for the plugins directory, its immediate
//!   subdirectories and every discovered archive, mirroring
//!   `ij.io.PluginClassLoader`.
//!
//! # Deliberate limits (phase 2)
//!
//! * Classification only distinguishes the relevant `PlugIn` interfaces. It
//!   does not run `setup` or `run`, inspect filters returned by `setup`, or
//!   verify that a plain `PlugInFilter` can actually process a given image.
//!   GUI/`PlugInFrame` plugins, dialogs and menu wiring remain out of scope.
//! * Inspection is opt-in and never implicit: scanning leaves every entry
//!   [`PluginKind::Uninspected`]. Both the class name and the classpath come
//!   from the catalog; labels are never interpreted as commands.
//! * Macro/script plugins (`.ijm`, `_*.txt`, `.js`, `.bsh`, `.py`) are ignored;
//!   they are not Java classes and are installed by `Menus.installMacros`.
//! * `Prefs.moveToMisc` menu relocation, `IJ_Prefs.txt` `pluginNN=` overrides,
//!   the `<ImageJ>/jars` classpath entry and duplicate labels that collide with
//!   built-in ImageJ commands are not applied. Duplicates are only detected
//!   among the discovered entries.
//! * Reads are bounded ([`ScanOptions`]); a bounded scan reports truncation
//!   instead of silently producing a partial catalog.
//! * `addPluginItem` shortcut suffixes such as `[q]` are preserved verbatim in
//!   [`DiscoveredPlugin::label`]; menu installation strips them in phase 2.

use crate::plugin::{
    CancellationToken, DEFAULT_PLUGIN_TIMEOUT, PluginBridge, PluginInspectionKind,
    PluginInspectionRequest, PluginInspectionResult, PluginInspectionStatus,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use zip::ZipArchive;

/// Default cap on the compressed size of a single archive that will be opened.
pub const DEFAULT_MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
/// Default cap on the bytes read from an embedded `plugins.config`.
pub const DEFAULT_MAX_CONFIG_BYTES: u64 = 1024 * 1024;
/// Default cap on the number of central-directory entries inspected per archive.
pub const DEFAULT_MAX_ARCHIVE_ENTRIES: usize = 100_000;
/// Default cap on the number of catalog entries.
pub const DEFAULT_MAX_PLUGINS: usize = 20_000;
/// Default cap on the number of retained diagnostics.
pub const DEFAULT_MAX_WARNINGS: usize = 1_000;

/// Bounds applied while scanning. All limits are inclusive maxima.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    pub max_archive_bytes: u64,
    pub max_config_bytes: u64,
    pub max_archive_entries: usize,
    pub max_plugins: usize,
    pub max_warnings: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_archive_bytes: DEFAULT_MAX_ARCHIVE_BYTES,
            max_config_bytes: DEFAULT_MAX_CONFIG_BYTES,
            max_archive_entries: DEFAULT_MAX_ARCHIVE_ENTRIES,
            max_plugins: DEFAULT_MAX_PLUGINS,
            max_warnings: DEFAULT_MAX_WARNINGS,
        }
    }
}

#[derive(Debug, Error)]
pub enum PluginDiscoveryError {
    #[error("plugins directory is not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Classification of a discovered entry.
///
/// Scanning never inspects class files, so every entry starts as
/// [`PluginKind::Uninspected`]. [`PluginCatalog::inspect_plugins`] (or its
/// `_with_options` form) is the explicit opt-in step that replaces the kind
/// with one of the other variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PluginKind {
    /// No class inspection has been requested yet.
    Uninspected,
    /// Plain `ij.plugin.filter.PlugInFilter`, the only supported plugin kind.
    PlugInFilter,
    /// `ij.plugin.PlugIn`. It does not consume an image and is unsupported by
    /// the narrow image filter path.
    PlugIn,
    /// `ij.plugin.filter.ExtendedPlugInFilter` (dialogs/preview/stacks) is
    /// unsupported.
    ExtendedPlugInFilter,
    /// Implements both `PlugIn` and `PlugInFilter`, so dispatch depends on the
    /// invocation path.
    Ambiguous,
    /// Internal `ij.*` class. `IJ.runPlugIn` handles these without the user
    /// plugin loader; they are never user plugin candidates.
    Internal,
    /// The class loads but implements neither `PlugIn` nor `PlugInFilter`.
    NotAPlugin,
    /// The class could not be loaded, linked, or named validly. The structured
    /// reason is in [`DiscoveredPlugin::inspection`].
    LoadFailed,
}

impl PluginKind {
    /// True only for the supported plain `PlugInFilter` category.
    pub fn is_supported_filter(self) -> bool {
        matches!(self, Self::PlugInFilter)
    }
}

/// Where a catalog entry was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginSource {
    /// A `_`-named `.class` file in the plugins directory or an immediate
    /// subdirectory.
    ClassFile { path: PathBuf },
    /// An entry read from (or generated for) a `_`-named `.jar`/`.zip` file.
    Archive {
        archive: PathBuf,
        /// 1-based line number in the archive's configuration text.
        config_line: usize,
    },
}

impl fmt::Display for PluginSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClassFile { path } => write!(f, "{}", path.display()),
            Self::Archive {
                archive,
                config_line,
            } => {
                write!(f, "{}:{config_line}", archive.display())
            }
        }
    }
}

/// A single installed-plugin candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredPlugin {
    /// Menu path, e.g. `["Plugins"]` or `["Plugins", "My Tools"]`.
    ///
    /// `None` mirrors `installJarPlugin` leaving `menu` null for a malformed
    /// `Plugins>` menu specification; ImageJ still registers the command but
    /// never adds it to a menu.
    pub menu_path: Option<Vec<String>>,
    /// Menu label as ImageJ records it (quoted text for archive entries,
    /// underscores replaced by spaces for class files).
    pub label: String,
    /// Class text as ImageJ records it. For archive entries this is the raw
    /// text after the last comma and may include a `("argument")` suffix; for
    /// class files it is the dotted class name.
    pub class_spec: String,
    /// [`PluginKind::Uninspected`] until [`PluginCatalog::inspect_plugins`] is
    /// called.
    pub kind: PluginKind,
    pub source: PluginSource,
    /// True when an earlier catalog entry already used this label
    /// (`Menus.pluginsTable` is global).
    pub duplicate: bool,
    /// Structured detail from the explicit inspection step. `None` while the
    /// entry is uninspected.
    #[serde(default)]
    pub inspection: Option<PluginInspection>,
}

/// Structured reason why inspection could not classify a class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "failure")]
pub enum PluginInspectionFailure {
    /// The class spec was empty or was not a Java binary name. No host process
    /// was started.
    InvalidClassName { message: String },
    /// The class itself was absent from the plugin classpath.
    ClassNotFound { message: String },
    /// Loading or linking failed for another reason (for example a missing
    /// dependency or malformed bytecode).
    LoadFailed {
        exception_type: String,
        message: String,
    },
    /// The bounded host could not complete (timeout, spawn failure, protocol
    /// error). The entry is left classified as [`PluginKind::LoadFailed`]
    /// because it could not be verified.
    Host { message: String },
}

/// Per-entry result of the explicit inspection step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInspection {
    /// Class name passed to the host. The class-spec argument suffix has been
    /// removed exactly as `Executer.runCommand` does.
    pub class_name: String,
    /// Never [`PluginKind::Uninspected`].
    pub kind: PluginKind,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub failure: Option<PluginInspectionFailure>,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
}

/// Bounds for [`PluginCatalog::inspect_plugins_with_options`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionOptions {
    /// Per-entry host timeout. Each entry gets its own bounded child process.
    pub timeout: Duration,
    /// Re-inspect entries that already have a non-`Uninspected` kind.
    pub reinspect: bool,
}

impl Default for InspectionOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_PLUGIN_TIMEOUT,
            reinspect: false,
        }
    }
}

/// Aggregate counters from one inspection pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionSummary {
    /// Entries inspected during this pass (including failures).
    pub inspected: usize,
    /// Entries already classified and skipped because `reinspect` was false.
    pub skipped: usize,
    /// Plain `PlugInFilter` entries.
    pub supported: usize,
    /// `PlugIn`, `ExtendedPlugInFilter`, `Ambiguous` and `NotAPlugin` entries.
    pub unsupported: usize,
    /// Internal `ij.*` entries.
    pub internal: usize,
    /// Entries whose class could not be validated or loaded, plus host errors.
    pub load_failed: usize,
}

/// How an archive's configuration text was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigOrigin {
    /// An entry whose name ends with `plugins.config` was found and read.
    Embedded,
    /// No configuration entry was found; entries were synthesized exactly like
    /// `Menus.autoGenerateConfigFile`.
    AutoGenerated,
    /// No configuration entry and no auto-generatable class; ImageJ skips the
    /// archive entirely.
    Absent,
}

/// Per-archive discovery report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveReport {
    pub path: PathBuf,
    pub config_origin: ConfigOrigin,
    /// Name of the embedded configuration entry, if any.
    pub config_entry: Option<String>,
    /// Configuration text (embedded or auto-generated) used for parsing.
    pub config_text: String,
    /// True when the configured byte limit cut the configuration text short.
    pub config_truncated: bool,
    /// Indices into [`PluginCatalog::plugins`] produced by this archive.
    pub plugins: Vec<usize>,
}

/// Non-fatal problem found while scanning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryWarning {
    DirectoryUnreadable {
        path: PathBuf,
        message: String,
    },
    DirectoryEntryNotUtf8 {
        path: PathBuf,
    },
    ArchiveUnreadable {
        path: PathBuf,
        message: String,
    },
    ArchiveTooLarge {
        path: PathBuf,
        size: u64,
        limit: u64,
    },
    ArchiveEntryLimit {
        path: PathBuf,
        limit: usize,
    },
    ConfigUnreadable {
        path: PathBuf,
        message: String,
    },
    ConfigTruncated {
        path: PathBuf,
        limit: u64,
    },
    NonUtf8Config {
        path: PathBuf,
    },
    MalformedConfigLine {
        path: PathBuf,
        line: usize,
        reason: String,
    },
    DuplicateCommand {
        menu_path: Option<Vec<String>>,
        label: String,
        first: PluginSource,
        duplicate: PluginSource,
    },
    PluginLimitReached {
        limit: usize,
    },
    WarningLimitReached {
        limit: usize,
    },
}

impl fmt::Display for DiscoveryWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectoryUnreadable { path, message } => {
                write!(f, "cannot read directory {}: {message}", path.display())
            }
            Self::DirectoryEntryNotUtf8 { path } => {
                write!(f, "ignoring non-UTF-8 directory entry {}", path.display())
            }
            Self::ArchiveUnreadable { path, message } => {
                write!(f, "cannot read archive {}: {message}", path.display())
            }
            Self::ArchiveTooLarge { path, size, limit } => write!(
                f,
                "skipping archive {}: {size} bytes exceeds the {limit}-byte limit",
                path.display()
            ),
            Self::ArchiveEntryLimit { path, limit } => write!(
                f,
                "archive {} has more than {limit} entries; the rest were not inspected",
                path.display()
            ),
            Self::ConfigUnreadable { path, message } => {
                write!(
                    f,
                    "cannot read plugins.config in {}: {message}",
                    path.display()
                )
            }
            Self::ConfigTruncated { path, limit } => write!(
                f,
                "plugins.config in {} was truncated at the {limit}-byte limit",
                path.display()
            ),
            Self::NonUtf8Config { path } => write!(
                f,
                "plugins.config in {} is not valid UTF-8; decoded lossily",
                path.display()
            ),
            Self::MalformedConfigLine { path, line, reason } => write!(
                f,
                "{} line {line}: ignoring entry ({reason})",
                path.display()
            ),
            Self::DuplicateCommand {
                menu_path,
                label,
                first,
                duplicate,
            } => {
                let menu = menu_path
                    .as_ref()
                    .map(|path| path.join(">"))
                    .unwrap_or_default();
                write!(
                    f,
                    "duplicate command {label:?} in {menu}: first at {first}, again at {duplicate}"
                )
            }
            Self::PluginLimitReached { limit } => {
                write!(f, "plugin limit of {limit} entries reached")
            }
            Self::WarningLimitReached { limit } => {
                write!(f, "warning limit of {limit} entries reached")
            }
        }
    }
}

/// Result of scanning a plugins directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCatalog {
    pub plugins_dir: PathBuf,
    /// Absolute classpath entries in `PluginClassLoader` order (plugins
    /// directory, immediate subdirectories, archives).
    pub classpath: Vec<PathBuf>,
    pub plugins: Vec<DiscoveredPlugin>,
    pub archives: Vec<ArchiveReport>,
    pub warnings: Vec<DiscoveryWarning>,
    /// True when [`ScanOptions::max_warnings`] suppressed further diagnostics.
    pub warnings_truncated: bool,
}

impl PluginCatalog {
    /// Scans `plugins_dir` with the default bounds.
    pub fn scan(plugins_dir: impl AsRef<Path>) -> Result<Self, PluginDiscoveryError> {
        Self::scan_with_options(plugins_dir, &ScanOptions::default())
    }

    /// Scans `plugins_dir` with explicit bounds.
    pub fn scan_with_options(
        plugins_dir: impl AsRef<Path>,
        options: &ScanOptions,
    ) -> Result<Self, PluginDiscoveryError> {
        let requested = plugins_dir.as_ref();
        let plugins_dir =
            std::path::absolute(requested).map_err(|source| PluginDiscoveryError::Io {
                path: requested.to_path_buf(),
                source,
            })?;
        let metadata = fs::metadata(&plugins_dir).map_err(|source| PluginDiscoveryError::Io {
            path: plugins_dir.clone(),
            source,
        })?;
        if !metadata.is_dir() {
            return Err(PluginDiscoveryError::NotADirectory(plugins_dir));
        }

        let mut warnings = Warnings::new(options.max_warnings);
        let mut class_entries: Vec<(String, PathBuf)> = Vec::new();
        let mut archives: Vec<PathBuf> = Vec::new();
        let mut classpath_archives: Vec<PathBuf> = Vec::new();
        let mut subdirs: Vec<PathBuf> = Vec::new();

        let listing = fs::read_dir(&plugins_dir).map_err(|source| PluginDiscoveryError::Io {
            path: plugins_dir.clone(),
            source,
        })?;
        for entry in listing {
            let entry = match entry {
                Ok(entry) => entry,
                Err(source) => {
                    warnings.push(DiscoveryWarning::DirectoryUnreadable {
                        path: plugins_dir.clone(),
                        message: source.to_string(),
                    });
                    continue;
                }
            };
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                warnings.push(DiscoveryWarning::DirectoryEntryNotUtf8 { path: entry.path() });
                continue;
            };
            let is_class_file = name.ends_with(".class");
            let has_underscore = name.contains('_');
            if has_underscore && is_class_file && !name.contains('$') {
                let stem = &name[..name.len() - ".class".len()];
                class_entries.push((stem.to_string(), entry.path()));
            } else if name.ends_with(".jar") || name.ends_with(".zip") {
                classpath_archives.push(entry.path());
                if has_underscore {
                    archives.push(entry.path());
                }
            } else if is_macro_name(name, has_underscore) {
                // Macro/script plugins are installed by `Menus.installMacros`
                // and are outside the phase-1 class catalog.
            } else if !is_class_file {
                // `Menus.checkSubdirectory`: only the first level is inspected.
                collect_subdirectory(
                    &entry.path(),
                    name,
                    &mut class_entries,
                    &mut archives,
                    &mut classpath_archives,
                    &mut subdirs,
                    &mut warnings,
                );
            }
        }

        // `Menus.getPlugins` sorts the class list with `StringSorter.sort`
        // (String.compareTo) before installing entries. Archives keep a
        // deterministic path order here because `File.list()` order is
        // unspecified.
        class_entries.sort_by(|a, b| java_string_cmp(&a.0, &b.0));
        archives.sort();
        classpath_archives.sort();
        subdirs.sort();

        let mut classpath = Vec::with_capacity(1 + subdirs.len() + classpath_archives.len());
        classpath.push(plugins_dir.clone());
        classpath.extend(subdirs.iter().cloned());
        classpath.extend(classpath_archives.iter().cloned());

        let mut plugins: Vec<DiscoveredPlugin> = Vec::new();
        let mut plugin_limit_reported = false;
        for (entry_name, path) in &class_entries {
            if plugins.len() >= options.max_plugins {
                warnings.push(DiscoveryWarning::PluginLimitReached {
                    limit: options.max_plugins,
                });
                plugin_limit_reported = true;
                break;
            }
            let (menu_path, label, class_spec) = match entry_name.split_once('/') {
                Some((dir, class_name)) => (
                    vec!["Plugins".to_string(), dir.to_string()],
                    class_name.replace('_', " "),
                    format!("{}.{}", dir, class_name),
                ),
                None => (
                    vec!["Plugins".to_string()],
                    entry_name.replace('_', " "),
                    entry_name.clone(),
                ),
            };
            plugins.push(DiscoveredPlugin {
                menu_path: Some(menu_path),
                label,
                class_spec,
                kind: PluginKind::Uninspected,
                source: PluginSource::ClassFile { path: path.clone() },
                duplicate: false,
                inspection: None,
            });
        }

        let mut archive_reports = Vec::new();
        for archive in &archives {
            if plugins.len() >= options.max_plugins {
                if !plugin_limit_reported {
                    warnings.push(DiscoveryWarning::PluginLimitReached {
                        limit: options.max_plugins,
                    });
                }
                break;
            }
            let Some((report, entries)) =
                scan_archive(archive, &plugins_dir, options, &mut warnings)
            else {
                continue;
            };
            let first_index = plugins.len();
            for (config_line, entry) in entries {
                if plugins.len() >= options.max_plugins {
                    if !plugin_limit_reported {
                        warnings.push(DiscoveryWarning::PluginLimitReached {
                            limit: options.max_plugins,
                        });
                        plugin_limit_reported = true;
                    }
                    break;
                }
                plugins.push(DiscoveredPlugin {
                    menu_path: entry.menu_path,
                    label: entry.label,
                    class_spec: entry.class_spec,
                    kind: PluginKind::Uninspected,
                    source: PluginSource::Archive {
                        archive: archive.clone(),
                        config_line,
                    },
                    duplicate: false,
                    inspection: None,
                });
            }
            archive_reports.push(ArchiveReport {
                plugins: (first_index..plugins.len()).collect(),
                ..report
            });
        }

        mark_duplicates(&mut plugins, &mut warnings);

        let warnings_truncated = warnings.truncated;
        let mut warning_items = warnings.items;
        if warnings_truncated {
            warning_items.push(DiscoveryWarning::WarningLimitReached {
                limit: options.max_warnings,
            });
        }

        Ok(Self {
            plugins_dir,
            classpath,
            plugins,
            archives: archive_reports,
            warnings: warning_items,
            warnings_truncated,
        })
    }

    /// Explicitly inspects every entry with default bounds.
    pub fn inspect_plugins(&mut self, bridge: &PluginBridge) -> InspectionSummary {
        self.inspect_plugins_with_options(bridge, &InspectionOptions::default())
    }

    /// Explicit opt-in class classification.
    ///
    /// Each entry gets its own bounded [`PluginBridge::inspect_plugin`] call.
    /// The scanner itself remains read-only; this method only mutates the
    /// `kind`/`inspection` fields of this catalog. Class names are derived from
    /// the catalog's class spec (with the `("arg")` suffix removed like
    /// `Executer.runCommand`), and labels are never interpreted as commands.
    /// A host failure is recorded as [`PluginInspectionFailure::Host`] on that
    /// entry instead of aborting the pass.
    pub fn inspect_plugins_with_options(
        &mut self,
        bridge: &PluginBridge,
        options: &InspectionOptions,
    ) -> InspectionSummary {
        self.inspect_plugins_with_options_and_cancel(bridge, options, None)
    }

    pub fn inspect_plugins_with_options_and_cancel(
        &mut self,
        bridge: &PluginBridge,
        options: &InspectionOptions,
        cancel: Option<&CancellationToken>,
    ) -> InspectionSummary {
        let mut summary = InspectionSummary::default();
        for plugin in &mut self.plugins {
            if cancel.is_some_and(CancellationToken::is_cancelled) {
                break;
            }
            if !options.reinspect && plugin.kind != PluginKind::Uninspected {
                summary.skipped += 1;
                continue;
            }

            let class_name = match class_name_from_spec(&plugin.class_spec) {
                Ok(name) => name,
                Err(message) => {
                    summary.inspected += 1;
                    summary.load_failed += 1;
                    let inspection = PluginInspection {
                        class_name: String::new(),
                        kind: PluginKind::LoadFailed,
                        message: Some(message.clone()),
                        failure: Some(PluginInspectionFailure::InvalidClassName { message }),
                        stdout: String::new(),
                        stderr: String::new(),
                    };
                    plugin.kind = inspection.kind;
                    plugin.inspection = Some(inspection);
                    continue;
                }
            };

            let request = PluginInspectionRequest::new(&class_name)
                .with_classpath_entries(self.classpath.iter().cloned());
            let inspection = match bridge.inspect_plugin_with_timeout_and_cancel(
                &request,
                options.timeout,
                cancel,
            ) {
                Ok(result) => inspection_from_result(result),
                Err(error) => PluginInspection {
                    class_name: class_name.clone(),
                    kind: PluginKind::LoadFailed,
                    message: Some(error.to_string()),
                    failure: Some(PluginInspectionFailure::Host {
                        message: error.to_string(),
                    }),
                    stdout: String::new(),
                    stderr: String::new(),
                },
            };

            summary.inspected += 1;
            match inspection.kind {
                PluginKind::PlugInFilter => summary.supported += 1,
                PluginKind::Internal => summary.internal += 1,
                PluginKind::LoadFailed | PluginKind::Uninspected => summary.load_failed += 1,
                _ => summary.unsupported += 1,
            }
            plugin.kind = inspection.kind;
            plugin.inspection = Some(inspection);
        }
        summary
    }
}

/// Removes the `("argument")` suffix exactly like `Executer.runCommand` does.
///
/// Only leading spaces are dropped, matching `Menus.addPluginItem`; trailing
/// spaces are preserved and later rejected as an invalid name, just as Java's
/// `ClassLoader.loadClass` would fail on them.
fn class_name_from_spec(class_spec: &str) -> Result<String, String> {
    let mut spec = class_spec.trim_start();
    if spec.ends_with("\")") {
        if let Some(index) = spec.rfind("(\"") {
            if index > 0 {
                spec = &spec[..index];
            }
        }
    }
    if spec.is_empty() {
        return Err("class name is empty".to_string());
    }
    Ok(spec.to_string())
}

fn inspection_from_result(result: PluginInspectionResult) -> PluginInspection {
    match result.status {
        PluginInspectionStatus::Inspected { kind, message } => PluginInspection {
            class_name: result.class_name,
            kind: match kind {
                PluginInspectionKind::PlugInFilter => PluginKind::PlugInFilter,
                PluginInspectionKind::PlugIn => PluginKind::PlugIn,
                PluginInspectionKind::ExtendedPlugInFilter => PluginKind::ExtendedPlugInFilter,
                PluginInspectionKind::Ambiguous => PluginKind::Ambiguous,
                PluginInspectionKind::NotAPlugin => PluginKind::NotAPlugin,
                PluginInspectionKind::Internal => PluginKind::Internal,
            },
            message: if message.is_empty() {
                None
            } else {
                Some(message)
            },
            failure: None,
            stdout: result.stdout,
            stderr: result.stderr,
        },
        PluginInspectionStatus::ClassNotFound { message } => PluginInspection {
            class_name: result.class_name,
            kind: PluginKind::LoadFailed,
            message: Some(message.clone()),
            failure: Some(PluginInspectionFailure::ClassNotFound { message }),
            stdout: result.stdout,
            stderr: result.stderr,
        },
        PluginInspectionStatus::LoadFailed {
            exception_type,
            message,
        } => PluginInspection {
            class_name: result.class_name,
            kind: PluginKind::LoadFailed,
            message: Some(message.clone()),
            failure: Some(PluginInspectionFailure::LoadFailed {
                exception_type,
                message,
            }),
            stdout: result.stdout,
            stderr: result.stderr,
        },
        PluginInspectionStatus::InvalidClassName { message } => PluginInspection {
            class_name: result.class_name,
            kind: PluginKind::LoadFailed,
            message: Some(message.clone()),
            failure: Some(PluginInspectionFailure::InvalidClassName { message }),
            stdout: result.stdout,
            stderr: result.stderr,
        },
    }
}

fn is_macro_name(name: &str, has_underscore: bool) -> bool {
    (has_underscore && name.ends_with(".txt"))
        || name.ends_with(".ijm")
        || name.ends_with(".js")
        || name.ends_with(".bsh")
        || name.ends_with(".py")
}

/// `Menus.checkSubdirectory`: one level of subdirectories, no recursion.
fn collect_subdirectory(
    path: &Path,
    name: &str,
    class_entries: &mut Vec<(String, PathBuf)>,
    archives: &mut Vec<PathBuf>,
    classpath_archives: &mut Vec<PathBuf>,
    subdirs: &mut Vec<PathBuf>,
    warnings: &mut Warnings,
) {
    if name.ends_with(".java") {
        return;
    }
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        _ => return,
    }
    subdirs.push(path.to_path_buf());
    let listing = match fs::read_dir(path) {
        Ok(listing) => listing,
        Err(source) => {
            warnings.push(DiscoveryWarning::DirectoryUnreadable {
                path: path.to_path_buf(),
                message: source.to_string(),
            });
            return;
        }
    };
    for entry in listing {
        let entry = match entry {
            Ok(entry) => entry,
            Err(source) => {
                warnings.push(DiscoveryWarning::DirectoryUnreadable {
                    path: path.to_path_buf(),
                    message: source.to_string(),
                });
                continue;
            }
        };
        let file_name = entry.file_name();
        let Some(child) = file_name.to_str() else {
            warnings.push(DiscoveryWarning::DirectoryEntryNotUtf8 { path: entry.path() });
            continue;
        };
        let has_underscore = child.contains('_');
        if has_underscore && child.ends_with(".class") && !child.contains('$') {
            let stem = &child[..child.len() - ".class".len()];
            class_entries.push((format!("{name}/{stem}"), entry.path()));
        } else if child.ends_with(".jar") || child.ends_with(".zip") {
            classpath_archives.push(entry.path());
            if has_underscore {
                archives.push(entry.path());
            }
        } else if is_macro_name(child, has_underscore) {
            // Out of scope (see module docs).
        }
        // Nested directories only contribute macros in ImageJ, so they are not
        // traversed here.
    }
}

fn scan_archive(
    path: &Path,
    plugins_dir: &Path,
    options: &ScanOptions,
    warnings: &mut Warnings,
) -> Option<(ArchiveReport, Vec<(usize, ParsedEntry)>)> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) => {
            warnings.push(DiscoveryWarning::ArchiveUnreadable {
                path: path.to_path_buf(),
                message: source.to_string(),
            });
            return None;
        }
    };
    if metadata.len() > options.max_archive_bytes {
        warnings.push(DiscoveryWarning::ArchiveTooLarge {
            path: path.to_path_buf(),
            size: metadata.len(),
            limit: options.max_archive_bytes,
        });
        return None;
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(source) => {
            warnings.push(DiscoveryWarning::ArchiveUnreadable {
                path: path.to_path_buf(),
                message: source.to_string(),
            });
            return None;
        }
    };
    let mut archive = match ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(error) => {
            warnings.push(DiscoveryWarning::ArchiveUnreadable {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            return None;
        }
    };

    let total_entries = archive.len();
    let scanned_entries = total_entries.min(options.max_archive_entries);
    if total_entries > scanned_entries {
        warnings.push(DiscoveryWarning::ArchiveEntryLimit {
            path: path.to_path_buf(),
            limit: options.max_archive_entries,
        });
    }

    // `Menus.getConfigurationFile` returns the first entry whose name ends with
    // "plugins.config", in central-directory order.
    let mut config_entry: Option<(usize, String)> = None;
    for index in 0..scanned_entries {
        match archive.by_index(index) {
            Ok(entry) => {
                let name = match entry.name() {
                    Ok(name) => name.into_owned(),
                    Err(error) => {
                        warnings.push(DiscoveryWarning::ArchiveUnreadable {
                            path: path.to_path_buf(),
                            message: error.to_string(),
                        });
                        return None;
                    }
                };
                if name.ends_with("plugins.config") {
                    config_entry = Some((index, name));
                    break;
                }
            }
            Err(error) => {
                warnings.push(DiscoveryWarning::ArchiveUnreadable {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                });
                return None;
            }
        }
    }

    let (config_origin, config_entry_name, config_text, config_truncated) =
        if let Some((index, entry_name)) = config_entry {
            let (bytes, truncated) =
                match read_entry_bounded(&mut archive, index, options.max_config_bytes) {
                    Ok(result) => result,
                    Err(message) => {
                        warnings.push(DiscoveryWarning::ConfigUnreadable {
                            path: path.to_path_buf(),
                            message,
                        });
                        (Vec::new(), false)
                    }
                };
            let text = decode_config(&bytes, path, warnings);
            if truncated {
                warnings.push(DiscoveryWarning::ConfigTruncated {
                    path: path.to_path_buf(),
                    limit: options.max_config_bytes,
                });
            }
            (ConfigOrigin::Embedded, Some(entry_name), text, truncated)
        } else {
            let mut text = String::new();
            let mut truncated = false;
            let mut generated = false;
            for index in 0..scanned_entries {
                let name = match archive.by_index(index) {
                    Ok(entry) => match entry.name() {
                        Ok(name) => name.into_owned(),
                        Err(error) => {
                            warnings.push(DiscoveryWarning::ArchiveUnreadable {
                                path: path.to_path_buf(),
                                message: error.to_string(),
                            });
                            break;
                        }
                    },
                    Err(error) => {
                        warnings.push(DiscoveryWarning::ArchiveUnreadable {
                            path: path.to_path_buf(),
                            message: error.to_string(),
                        });
                        break;
                    }
                };
                if !auto_generated_candidate(&name) {
                    continue;
                }
                generated = true;
                let line = auto_generated_line(&name);
                if text.len() as u64 + line.len() as u64 > options.max_config_bytes {
                    truncated = true;
                    break;
                }
                text.push_str(&line);
            }
            if truncated {
                warnings.push(DiscoveryWarning::ConfigTruncated {
                    path: path.to_path_buf(),
                    limit: options.max_config_bytes,
                });
            }
            let origin = if generated {
                ConfigOrigin::AutoGenerated
            } else {
                ConfigOrigin::Absent
            };
            (origin, None, text, truncated)
        };

    let parse_text = if config_truncated {
        complete_lines(&config_text)
    } else {
        config_text.as_str()
    };
    let entries = parse_config(parse_text, path, plugins_dir, warnings);

    Some((
        ArchiveReport {
            path: path.to_path_buf(),
            config_origin,
            config_entry: config_entry_name,
            config_text,
            config_truncated,
            plugins: Vec::new(),
        },
        entries,
    ))
}

fn read_entry_bounded(
    archive: &mut ZipArchive<File>,
    index: usize,
    limit: u64,
) -> Result<(Vec<u8>, bool), String> {
    let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
    let declared_size = entry.size();
    let mut bytes = Vec::new();
    entry
        .by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let truncated = declared_size > limit || bytes.len() as u64 > limit;
    if bytes.len() as u64 > limit {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        bytes.truncate(limit);
    }
    Ok((bytes, truncated))
}

fn decode_config(bytes: &[u8], path: &Path, warnings: &mut Warnings) -> String {
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => text,
        Err(error) => {
            warnings.push(DiscoveryWarning::NonUtf8Config {
                path: path.to_path_buf(),
            });
            String::from_utf8_lossy(error.as_bytes()).into_owned()
        }
    }
}

/// `Menus.autoGenerateConfigFile` entry predicate.
fn auto_generated_candidate(name: &str) -> bool {
    if !name.ends_with(".class") {
        return false;
    }
    if !name.find('_').is_some_and(|index| index > 0) {
        return false;
    }
    if name.contains('$') || name.contains("/_") || name.starts_with('_') {
        return false;
    }
    // ImageJ skips classes in lowercase package directories.
    if name.chars().next().is_some_and(char::is_lowercase) && name.contains('/') {
        return false;
    }
    true
}

/// One line of the configuration text `Menus.autoGenerateConfigFile` builds.
fn auto_generated_line(name: &str) -> String {
    let class_name = &name[..name.len() - ".class".len()];
    match class_name.rfind('/') {
        Some(slash) => {
            let menu = class_name[..slash].replace('/', ">").replace('_', " ");
            let label = class_name[slash + 1..].replace('_', " ");
            let class_spec = class_name.replace('/', ".");
            format!("Plugins>{menu}, \"{label}\", {class_spec}\n")
        }
        None => {
            let label = class_name.replace('_', " ");
            format!("Plugins, \"{label}\", {class_name}\n")
        }
    }
}

fn parse_config(
    text: &str,
    archive: &Path,
    plugins_dir: &Path,
    warnings: &mut Warnings,
) -> Vec<(usize, ParsedEntry)> {
    let mut entries = Vec::new();
    for (index, line) in split_java_lines(text).into_iter().enumerate() {
        let line_number = index + 1;
        // `Menus.installJarPlugins`: `s.length()>=3 && !s.startsWith("#")`.
        if line.len() < 3 || line.starts_with('#') {
            continue;
        }
        let trimmed = line.trim();
        // Indented comments are not entries; ImageJ silently drops them too.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match parse_config_line(trimmed, archive, plugins_dir) {
            Ok(Some(entry)) => entries.push((line_number, entry)),
            Ok(None) => {}
            Err(reason) => warnings.push(DiscoveryWarning::MalformedConfigLine {
                path: archive.to_path_buf(),
                line: line_number,
                reason: reason.to_string(),
            }),
        }
    }
    entries
}

struct ParsedEntry {
    menu_path: Option<Vec<String>>,
    label: String,
    class_spec: String,
}

/// Port of the parsing in `Menus.installJarPlugin` + `Menus.addPluginItem`.
fn parse_config_line(
    line: &str,
    archive: &Path,
    plugins_dir: &Path,
) -> Result<Option<ParsedEntry>, &'static str> {
    let Some(first_quote) = line.find('"') else {
        return Err("missing quoted menu label");
    };
    let rest = &line[first_quote..];
    if rest.starts_with("\"-\"") {
        // `addPluginItem` treats `"-"` as a menu separator.
        return Ok(None);
    }
    let menu_path = config_menu_path(line, archive, plugins_dir);
    // `Menus.addPluginItem` uses `lastIndexOf(',')` even for class arguments.
    let Some(last_comma) = rest.rfind(',') else {
        return Err("missing comma before class name");
    };
    if last_comma <= 1 {
        return Err("missing quoted menu label");
    }
    let class_spec = rest[last_comma + 1..].trim_start();
    if class_spec.is_empty() {
        return Err("missing class name");
    }
    // `installJarPlugin` derives `menuEntry` from the text between the first
    // pair of quotes (or the remainder when the quote is unterminated).
    let label = match rest[1..].find('"') {
        Some(close) => rest[1..1 + close].to_string(),
        None => rest[1..].to_string(),
    };
    Ok(Some(ParsedEntry {
        menu_path,
        label,
        class_spec: class_spec.to_string(),
    }))
}

fn config_menu_path(line: &str, archive: &Path, plugins_dir: &Path) -> Option<Vec<String>> {
    if line.starts_with("Plugins>") {
        match line.find(',') {
            Some(comma) if comma > 8 => {
                let name = line[8..comma].strip_suffix('>').unwrap_or(&line[8..comma]);
                let mut path = vec!["Plugins".to_string()];
                path.extend(name.split('>').map(str::to_string));
                Some(path)
            }
            _ => None,
        }
    } else if line.starts_with('"') || line.starts_with("Plugins") {
        let menu_path = match submenu_name(archive, plugins_dir) {
            Some(submenu) => vec!["Plugins".to_string(), submenu],
            None => vec!["Plugins".to_string()],
        };
        Some(menu_path)
    } else {
        let name = match line.find('"') {
            Some(quote) => line[..quote].trim(),
            None => line,
        };
        let name = match name.find(',') {
            Some(comma) => &name[..comma],
            None => name,
        };
        // Backward-compatibility rename from `installJarPlugin`.
        let name = if name.starts_with("Help>About") {
            "Help>About Plugins"
        } else {
            name
        };
        let name = name.strip_suffix('>').unwrap_or(name);
        Some(name.split('>').map(str::to_string).collect())
    }
}

/// `Menus.getSubmenuName`: the JAR's immediate parent directory, or `None` for
/// archives directly in the plugins directory.
fn submenu_name(archive: &Path, plugins_dir: &Path) -> Option<String> {
    let relative = archive.strip_prefix(plugins_dir).ok()?;
    let parent = relative.parent()?;
    let name = parent.file_name()?.to_str()?;
    if name == "plugins" {
        None
    } else {
        Some(name.to_string())
    }
}

fn mark_duplicates(plugins: &mut [DiscoveredPlugin], warnings: &mut Warnings) {
    let mut first_by_label: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for index in 0..plugins.len() {
        let label = match &plugins[index].source {
            PluginSource::Archive { .. } => java_command_label(&plugins[index].label),
            PluginSource::ClassFile { .. } => &plugins[index].label,
        }
        .to_string();
        match first_by_label.get(&label) {
            Some(&first) => {
                plugins[index].duplicate = true;
                warnings.push(DiscoveryWarning::DuplicateCommand {
                    menu_path: plugins[index].menu_path.clone(),
                    label,
                    first: plugins[first].source.clone(),
                    duplicate: plugins[index].source.clone(),
                });
            }
            None => {
                first_by_label.insert(label, index);
            }
        }
    }
}

// `Menus.addPluginItem` removes a valid non-function-key shortcut from the
// command before inserting it into the global pluginsTable.
fn java_command_label(label: &str) -> &str {
    if !label.ends_with(']') {
        return label;
    }
    let Some(open) = label.rfind('[').filter(|&open| open > 0) else {
        return label;
    };
    let shortcut = &label[open + 1..label.len() - 1];
    let valid_non_function = match shortcut.as_bytes() {
        [ch] => ch.is_ascii_alphanumeric(),
        [b'A', direction] => matches!(direction, b'L' | b'U' | b'R' | b'D'),
        [b'N', key] => key.is_ascii_digit() || matches!(key, b'/' | b'*' | b'-' | b'+' | b'.'),
        _ => false,
    };
    if valid_non_function {
        &label[..open]
    } else {
        label
    }
}

/// Java `String.compareTo` orders by UTF-16 code units.
fn java_string_cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

/// Splits text like `java.io.LineNumberReader` (`\n`, `\r\n`, `\r`).
fn split_java_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                lines.push(&text[start..index]);
                start = index + 1;
            }
            b'\r' => {
                lines.push(&text[start..index]);
                if index + 1 < bytes.len() && bytes[index + 1] == b'\n' {
                    index += 1;
                }
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Drops an incomplete trailing line from a truncated configuration text.
fn complete_lines(text: &str) -> &str {
    if text.ends_with('\n') || text.ends_with('\r') {
        return text;
    }
    match text.rfind(['\n', '\r']) {
        Some(index) => &text[..index],
        None => "",
    }
}

struct Warnings {
    items: Vec<DiscoveryWarning>,
    truncated: bool,
    limit: usize,
}

impl Warnings {
    fn new(limit: usize) -> Self {
        Self {
            items: Vec::new(),
            truncated: false,
            limit,
        }
    }

    fn push(&mut self, warning: DiscoveryWarning) {
        if self.items.len() < self.limit {
            self.items.push(warning);
        } else {
            self.truncated = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_line_splitting_matches_line_number_reader() {
        assert_eq!(split_java_lines("a\nb\r\nc\rd"), vec!["a", "b", "c", "d"]);
        assert_eq!(split_java_lines("a\n"), vec!["a"]);
        assert_eq!(split_java_lines(""), Vec::<&str>::new());
    }

    #[test]
    fn java_string_order_is_case_sensitive() {
        let mut names = vec!["b_Plugin", "A_Plugin", "_Leading", "Zed/Z_Plugin"];
        names.sort_by(|a, b| java_string_cmp(a, b));
        assert_eq!(
            names,
            vec!["A_Plugin", "Zed/Z_Plugin", "_Leading", "b_Plugin"]
        );
    }

    #[test]
    fn auto_generation_predicate_matches_menus_java() {
        assert!(auto_generated_candidate("My_Plugin.class"));
        assert!(auto_generated_candidate("Sub_Dir/Third_Thing.class"));
        assert!(!auto_generated_candidate("_Hidden.class"));
        assert!(!auto_generated_candidate("Has$Inner_X.class"));
        assert!(!auto_generated_candidate("Sub/_Private_Thing.class"));
        assert!(!auto_generated_candidate("pack/Other_Thing.class"));
        assert!(!auto_generated_candidate("Note.txt"));
    }

    #[test]
    fn auto_generated_line_matches_menus_java() {
        assert_eq!(
            auto_generated_line("My_Plugin.class"),
            "Plugins, \"My Plugin\", My_Plugin\n"
        );
        assert_eq!(
            auto_generated_line("Sub_Dir/Third_Thing.class"),
            "Plugins>Sub Dir, \"Third Thing\", Sub_Dir.Third_Thing\n"
        );
    }

    #[test]
    fn complete_lines_drops_partial_tail() {
        assert_eq!(complete_lines("a\nb"), "a");
        assert_eq!(complete_lines("a\nb\n"), "a\nb\n");
        assert_eq!(complete_lines("partial"), "");
    }
}
