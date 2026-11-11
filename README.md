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

You can also use **File → Open** in the app. For a quick tour without an image file, choose **File → New → Image... (8-bit Ramp)**, draw a rectangle, and select **Analyze → Measure** to see the Results window. The TIFF reader currently supports uncompressed files in its implemented formats; compressed TIFFs are not supported. The app is still being developed, so save a copy of valuable images before editing them.

## What works today

- A desktop toolbar, separate image windows, zoom, a Results window, and ImageJ-style status and image information.
- 8-bit grayscale and indexed color, 16-bit grayscale, 32-bit float, and RGB image data; multi-slice stacks and C/Z/T dimensions in the data model and TIFF round trips.
- Rectangle, oval, line, polygon, freehand, and Wand selections. Rectangle, oval, and line ROIs can also be moved or resized interactively.
- Display range, Brightness & Contrast, threshold controls with live histogram previews, selected filters, measurements, TIFF I/O, and macro recording for supported commands.
- Crop, Duplicate, 8-bit/16-bit/32-bit/RGB type conversion, Set Measurements, Histogram, and Plot Profile commands, with stack-wide processing for the supported filters.
- Hyperstack C/Z/T navigation, slice animation, and the ImageJ-style native macOS menu bar (the in-window menu is used on other platforms).
- A Java compatibility path for a **limited subset** of classic ImageJ plug-ins, described below.

The app is still being expanded. Items marked **unsupported** remain on the roadmap; current gaps include common Edit drawing and clipboard commands, binary and math processing, Set Scale, Analyze Particles, ROI Manager, additional toolbar tools, and compressed or non-TIFF image formats. See the [Phase 2 roadmap](docs/roadmap_phase2_ui_fidelity.md) for the completed fidelity work and the [issue tracker](https://github.com/YokoSpace/ImageJ-rs/issues) for follow-up work.

## Classic ImageJ plug-ins

The **Plugins → Installed Plugins** window can scan a classic ImageJ plug-ins directory and inspect entries. After a catalog has been scanned and inspected, it is cached and compatible entries also appear directly in the **Plugins** menu for one-click execution. The current execution path supports a narrow subset of plain `PlugInFilter` classes through a separate Java process. Filter input must be a single 8-bit grayscale image with default metadata and either no ROI or an in-bounds rectangular ROI. Other image types, stacks, and plug-ins needing unsupported UI or host features are rejected rather than silently converted.

Plug-ins do **not** run in a security sandbox. Run only code you trust. Broader Java plug-in compatibility and a Rust plug-in API remain future work.

## Verify the implementation

```sh
./tools/verify.sh
```

This checks the untouched Java reference, builds the ImageJ 1.54u oracle, runs Rust tests including differential comparisons, and exercises a TIFF → ROI → measurement → save workflow. Success ends with `VERIFY_ALL_OK`; artifacts are written to `verification/evidence/latest/`.

For a shorter environment check:

```sh
cargo run -p ij-cli -- doctor
```

`ij-cli` is a verification tool, not a general ImageJ command-line replacement. Its current commands are `doctor`, `slice1`, and `parity-check`.

## Repository layout

| Path | Purpose |
| --- | --- |
| [`crates/ij-core/`](crates/ij-core/) | Pure image, ROI, measurement, and processing logic |
| [`crates/ij-io/`](crates/ij-io/) | TIFF reading and writing |
| [`crates/ij-session/`](crates/ij-session/) | Documents, commands, results, and macro recording |
| [`crates/ij-app/`](crates/ij-app/) | Desktop application |
| [`crates/ij-bridge/`](crates/ij-bridge/) | Java oracle and plug-in process bridges |
| [`original_imagej/`](original_imagej/) | Read-only ImageJ reference source |
| [`tools/`](tools/) | Oracle, plug-in host, and verification scripts |

The [architecture notes](docs/architecture_rationale.md) explain the design. When contributing behavior changes, compare them with ImageJ 1.54u and run `./tools/verify.sh`. See [AGENTS.md](AGENTS.md) for repository rules, especially the read-only boundary around `original_imagej/`.

## Origin and licensing

ImageJ is a public-domain project developed at the US National Institutes of Health; see [`original_imagej/LICENSE.txt`](original_imagej/LICENSE.txt). The Rust crates declare `MIT OR Apache-2.0` in [`Cargo.toml`](Cargo.toml).
