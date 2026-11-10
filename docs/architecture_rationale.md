# ImageJ-rs architecture rationale and implementation roadmap

## Problem

We are porting ImageJ 1.54u (`original_imagej/`) to Rust (`ImageJ-rs`) while preserving the original multi-window desktop UX and existing Java plugin compatibility. Subsequent implementation phases will be executed by `deepseek-V4.1-flash`. A lightweight model degrades when forced to infer hidden state across 3,500-line classes like `ImagePlus.java` or guess floating-point rounding rules. The architecture must isolate pure pixel and measurement logic from UI and JVM state, derive command tables from `original_imagej/`, and verify every unit against the real ImageJ 1.54u Java runtime before advancing.

## Usage (caller's view)

Both the future desktop UI (`ij-app`) and the verification CLI (`ij-cli`) drive the system by sending typed `Command` values to `Session::dispatch` and inspecting `CommandOutcome` or `Session.results`:

```rust
use ij_session::{Command, CommandOutcome, Session};
use std::path::PathBuf;

let mut session = Session::new();
session.dispatch(Command::OpenTiff {
    path: PathBuf::from("sample.tif"),
})?;
session.dispatch(Command::SetDisplayRange { min: 16.0, max: 240.0 })?;
session.dispatch(Command::MakeRectRoi { x: 2, y: 3, width: 8, height: 6 })?;

if let CommandOutcome::Measured(stats) = session.dispatch(Command::Measure)? {
    assert_eq!(stats.area, 48.0);
}

session.dispatch(Command::SaveResultsTsv {
    path: PathBuf::from("Results.tsv"),
})?;
```

When a command or test needs differential verification against original ImageJ 1.54u, the caller invokes `JavaOracleBridge`:

```rust
use ij_bridge::JavaOracleBridge;
use ij_core::ImageDocument;

let bridge = JavaOracleBridge::locate_from_repo_root(".")?;
let doc = ImageDocument::new_ramp_8bit("test.tif", 16, 16)?;
let oracle = bridge.run_macro_on_image(&doc, "makeRectangle(4, 4, 8, 8); run(\"Invert\");")?;
```

## Shape

The workspace is divided into five crates with strict dependency direction (`ij-core` <- `ij-io` <- `ij-bridge` <- `ij-session` <- `ij-cli`):

1. `crates/ij-core`: Pure domain types with zero I/O, UI, or JVM dependencies (`per principle-boundary-discipline`).
   - `PixelPlane` encodes the five ImageJ pixel representations as a single sum type (`Gray8`, `Color256 { pixels, lut }`, `Gray16`, `Gray32`, `ColorRgb`) so bit-depth and LUT invariants cannot drift out of sync (`per principle-model-the-domain` and `per principle-type-system-discipline`).
   - `ImageDocument` owns dimensions, `PixelPlane`, `DisplayRange`, `Calibration`, `Option<Roi>`, and `Option<PixelPlane>` undo snapshot.
   - `MeasurementSummary` and `ResultsTable` compute and format statistics matching `ByteStatistics.java` and `ResultsTable.java` (including the `+0.5` pixel-center offset on `xCentroid`, `yCentroid`, `xCenterOfMass`, and `yCenterOfMass`).
2. `crates/ij-io`: Boundary parsers and encoders (`load_tiff`, `save_tiff`) that convert raw TIFF byte streams into validated `ImageDocument` instances (`per principle-boundary-discipline`).
3. `crates/ij-bridge`: Subprocess bridge to the unmodified ImageJ 1.54u Java runtime (`tools/oracle/run_oracle.sh`). Exchanges images via temporary TIFF files and executes `.ijm` macros in headless batch mode (`-Dapple.awt.UIElement=true`).
4. `crates/ij-session`: Owns `Session`, `ImageId`, `Command`, `CommandOutcome`, macro recording, and `generated_commands.rs` (414 commands derived from `Menus.java` and `IJ_Props.txt` `per principle-encode-lessons-in-structure`).
5. `crates/ij-cli`: Executable verification binary exposing `doctor`, `slice1`, and `parity-check`.

## Synthesis decision

We evaluated two candidate architectures (`per architect` and `per principle-exhaust-the-design-space`):

- **Candidate 1 (Object-Graph Mirror):** Port `ImagePlus`, `ImageProcessor`, `ImageWindow`, and `WindowManager` as shared `Arc<Mutex<...>>` structs with JNI callbacks into a shared JVM. Rejected because it exhibits the *Split ownership*, *Shallow module*, and *Information leakage* red flags (`references/design-red-flags.md`). A fast coding model editing one method cannot see lock ordering or implicit mutations in `getPixels()`.
- **Candidate 2 (Command-Driven Session + Out-of-Process Java Oracle Bridge):** Selected as the base. All state mutations enter through `Session::dispatch(Command)`. `ij-core` owns pure data transformations, and Java compatibility runs in an isolated JVM process (`tools/oracle/run_oracle.sh`). This hides AWT/JVM quirks behind two methods on `JavaOracleBridge` and makes every operation testable without a window server.

## Tradeoffs accepted

- We accept TIFF serialization overhead when crossing the Rust/Java boundary in exchange for complete memory isolation and zero JNI UB or thread-deadlock risk.
- We accept compiling a 15-line `java.applet.Applet` shim (`tools/oracle/shim/java/applet/Applet.java`) via `--patch-module java.desktop` in exchange for running `original_imagej` on modern JDK 27 without modifying files inside `original_imagej/`.
- We accept single-slice 2D planes in Vertical Slice 1 in exchange for locking down exact numerical and TIFF parity before introducing C/Z/T hyperstack indexing in Slice 2.

## Alternatives considered

- **In-process JNI bridge (`jni` crate sharing pixel arrays):** Rejected because ImageJ 1.x plugins retain raw array references after `run()` returns and spawn background AWT threads, making Rust aliasing rules (`&mut [u8]`) unsound.
- **Hand-maintained Markdown command checklist:** Rejected because 414 menu commands across `Menus.java` and `IJ_Props.txt` will drift if edited by hand (`per principle-build-the-lever`). `tools/ledger/sync_ledger.py` generates both the documentation table and the Rust `COMMAND_SPECS` array.

## Open questions and risks

- Which Rust GUI crate (`egui` with multi-viewport vs native `winit` + `softbuffer`/`wgpu` vs `iced`) best reproduces macOS top-level menu bars and independent floating image windows on macOS Tahoe? (Scheduled as a dedicated prototype step in Unit 6).

## Sequenced units for `deepseek-V4.1-flash`

Each unit below is scoped to 1-3 files and ends with a mandatory verification command (`per principle-sequence-verifiable-units`). Do not start Unit $N+1$ until Unit $N$ passes `./tools/verify.sh`.

> Status (2026-11-10): Units 1-6 are complete. Phase 2 (UI and usability fidelity) is sequenced in `docs/roadmap_phase2_ui_fidelity.md`.

### Unit 1: 16-bit and 32-bit float measurement parity
- **Files to edit:** `crates/ij-core/src/lib.rs`, `crates/ij-session/tests/oracle_parity.rs`
- **Task:** Add `ImageDocument::new_ramp_16bit` and `ImageDocument::new_ramp_32bit`. Verify `measure()` and `invert()` against `ShortStatistics.java` and `FloatStatistics.java` via `JavaOracleBridge`.
- **Verification:** `./tools/verify.sh`

### Unit 2: Histogram and Mode / Median / StdDev / Skewness / Kurtosis parity
- **Files to edit:** `crates/ij-core/src/lib.rs`, `crates/ij-session/tests/oracle_parity.rs`
- **Task:** Add `Histogram` (256-bin and 16-bit 65536-bin) plus `mode`, `median`, `skewness`, and `kurtosis` fields to `MeasurementSummary`, matching `ByteStatistics.java` lines 123-159 (`+ Double.MIN_VALUE` moment stabilization).
- **Verification:** `./tools/verify.sh`

### Unit 3: Non-rectangular ROIs (OvalRoi, PolygonRoi, Line) and mask generation
- **Files to edit:** `crates/ij-core/src/lib.rs`, `crates/ij-session/src/lib.rs`, `crates/ij-session/tests/oracle_parity.rs`
- **Task:** Add `Roi::Oval`, `Roi::Polygon`, and `Roi::Line` variants to `ij_core::Roi` and rasterize masks matching `OvalRoi.java` and `PolygonRoi.java`. Compare `Area` and `Mean` inside an oval ROI against `makeOval(x, y, w, h); run("Measure");` in the Java oracle.
- **Verification:** `./tools/verify.sh`

### Unit 4: Multi-slice `ImageStack` and C/Z/T `HyperStack` model
- **Files to edit:** `crates/ij-core/src/lib.rs`, `crates/ij-io/src/lib.rs`, `crates/ij-session/tests/oracle_parity.rs`
- **Task:** Replace single `plane: PixelPlane` in `ImageDocument` with `slices: Vec<PixelPlane>` and 1-based `(channel, slice, frame)` position matching `ImagePlus.java`. Extend `ij-io` to read and write multi-IFD TIFF stacks.
- **Verification:** `./tools/verify.sh`

### Unit 5: Core filters (`GaussianBlur`, `RankFilters` median/min/max, `AutoThresholder`)
- **Files to edit:** `crates/ij-core/src/filters.rs`, `crates/ij-core/src/lib.rs`, `crates/ij-session/src/lib.rs`, `crates/ij-session/tests/oracle_parity.rs`
- **Task:** Port `GaussianBlur`, 3x3 `Smooth`/`Sharpen`/`Find Edges`, and `IsoData`/`Otsu` thresholding. Verify pixel-by-pixel output against `JavaOracleBridge::run_macro_on_image`.
- **Verification:** `./tools/verify.sh`

### Unit 6: Multi-window desktop UI shell (`crates/ij-app`)
- **Files to edit:** `Cargo.toml`, `crates/ij-app/Cargo.toml`, `crates/ij-app/src/main.rs`
- **Task:** Build the main ImageJ toolbar window, image canvas window with zoom and ROI mouse drag, and Results table window, wired strictly through `Session::dispatch(Command)`.
- **Verification:** `./tools/verify.sh`
