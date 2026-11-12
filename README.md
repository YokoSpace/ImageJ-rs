# ImageJ-rs

[日本語版](README_ja.md)

**ImageJ-rs is an in-progress Rust reproduction of ImageJ 1.54u.** The goal is to keep ImageJ's familiar windows and workflows while reproducing its image processing and measurements as closely as possible. The original Java implementation in [`original_imagej/`](original_imagej/) is the reference for behavior, and automated tests compare Rust results with that implementation.

This is a development project, not a complete replacement for ImageJ or Fiji. There is no packaged release yet; build it from source.

## Try the desktop app

You need a recent Rust toolchain, Python 3, `zsh`, and a JDK providing `java`, `javac`, and `jar`. Development and verification currently take place on macOS with JDK 27. Other platforms and JDK versions have not been verified.

```sh
git clone https://github.com/YokoSpace/ImageJ-rs.git
cd ImageJ-rs
./tools/oracle/build_oracle.sh
cargo run --release -p ij-app
```

To open a TIFF at startup:

```sh
cargo run --release -p ij-app -- /path/to/image.tif
```

You can also use **File → Open** for TIFF, PNG, and JPEG images. For a quick tour without an image file, choose **File → New → Image...**, select an 8-bit ramp, draw a rectangle, and select **Analyze → Measure** to see the Results window. The TIFF reader handles PackBits, Deflate, and LZW compression for its supported image types. The app is still being developed, so save a copy of valuable images before editing them.

## What works today

- A desktop toolbar, separate image windows, zoom, a Results window, and ImageJ-style status and image information.
- 8-bit grayscale and indexed color, 16-bit grayscale, 32-bit float, and RGB image data; multi-slice stacks and C/Z/T dimensions in the data model and TIFF round trips.
- All 14 toolbar tools are enabled, including point, angle, segmented and freehand lines, Text, and Color Picker. The ROI Manager can store, select, delete, and measure selections.
- Display range, Brightness & Contrast, threshold controls with live histogram previews and mask conversion, binary Erode/Dilate/Open/Close, measurements, and macro recording for supported commands.
- Edit → Cut, Copy, Paste, Clear, Fill, and Draw with foreground and background colors, ROI masks, and Undo.
- File → New → Image, Edit → Clear Outside and Selection → Create Mask/Create Selection, Image → Transform flips and rotations, and Image → Adjust → Size. Auto Threshold supports 32-bit float images and stack histograms.
- Crop, Duplicate, 8-bit/16-bit/32-bit/RGB type conversion, Set Measurements, Set Scale, Analyze Particles, Histogram, Plot Profile, 14 Process → Math operations, and Image Calculator. Supported filters, math operations, and binary conversions can process stacks.
- TIFF, PNG, and JPEG opening and saving, plus Results table export. PNG and TIFF preserve pixels on supported lossless round trips; JPEG uses lossy compression.
- Hyperstack C/Z/T navigation, slice animation, and the ImageJ-style native macOS menu bar (the in-window menu is used on other platforms).
- A Java compatibility path for a **limited subset** of classic ImageJ plug-ins and an API for Rust image plug-ins, described below.

The [Phase 3 daily workflow](docs/roadmap_phase3_practical_workflows.md) and [Phase 4 Wave 1](docs/roadmap_phase4_completeness.md) are implemented for the tested image types. The project remains narrower than ImageJ: some TIFF variants, advanced plug-in APIs and dialogs, and specialized image commands are not yet supported. Text uses a small built-in bitmap font. Disjoint or holed masks that require ImageJ's composite ShapeRoi remain unsupported.

## Classic ImageJ plug-ins

The **Plugins → Installed Plugins** window can scan a classic ImageJ plug-ins directory and inspect entries. After a catalog has been scanned and inspected, it is cached and compatible entries also appear directly in the **Plugins** menu for one-click execution. A separate Java process runs supported plain `PlugInFilter` classes on 8-bit, 16-bit, 32-bit, and RGB images. It handles single slices and ordinary stacks, including filters that declare `DOES_STACKS`. One standard `GenericDialog` with numeric, string, choice, and checkbox fields can be shown in egui. Indexed-color images, hyperstacks, non-rectangular ROIs, advanced filter flags, and more complex dialogs remain outside this compatibility path.

Plug-ins do **not** run in a security sandbox. Run only code you trust. The [`ij-plugin-api` crate](crates/ij-plugin-api/) also provides a typed Rust image plug-in interface with transactional document updates.

## Verify the implementation

```sh
./tools/verify.sh
```

This checks the untouched Java reference, builds the ImageJ 1.54u oracle, runs Rust tests including differential comparisons and the Phase 3 compressed TIFF → particle analysis → save workflow, and exercises the original TIFF → ROI → measurement → save slice. Success ends with `VERIFY_ALL_OK`; artifacts are written to `verification/evidence/latest/`.

For a shorter environment check:

```sh
cargo run -p ij-cli -- doctor
```

`ij-cli` is a verification tool, not a general ImageJ command-line replacement. Its current commands are `doctor`, `slice1`, and `parity-check`.

## Repository layout

| Path | Purpose |
| --- | --- |
| [`crates/ij-core/`](crates/ij-core/) | Pure image, ROI, measurement, and processing logic |
| [`crates/ij-io/`](crates/ij-io/) | TIFF, PNG, and JPEG reading and writing |
| [`crates/ij-session/`](crates/ij-session/) | Documents, commands, results, and macro recording |
| [`crates/ij-app/`](crates/ij-app/) | Desktop application |
| [`crates/ij-bridge/`](crates/ij-bridge/) | Java oracle and plug-in process bridges |
| [`crates/ij-plugin-api/`](crates/ij-plugin-api/) | Typed Rust image plug-in interface |
| [`original_imagej/`](original_imagej/) | Read-only ImageJ reference source |
| [`tools/`](tools/) | Oracle, plug-in host, and verification scripts |

The [architecture notes](docs/architecture_rationale.md) explain the design. When contributing behavior changes, compare them with ImageJ 1.54u and run `./tools/verify.sh`. See [AGENTS.md](AGENTS.md) for repository rules, especially the read-only boundary around `original_imagej/`.

## Origin and licensing

ImageJ is a public-domain project developed at the US National Institutes of Health; see [`original_imagej/LICENSE.txt`](original_imagej/LICENSE.txt). The Rust crates declare `MIT OR Apache-2.0` in [`Cargo.toml`](Cargo.toml).
