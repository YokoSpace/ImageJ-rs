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
- Display range, Brightness & Contrast, threshold controls, selected filters, measurements, TIFF I/O, and macro recording for supported commands.
- A Java compatibility path for a **limited subset** of classic ImageJ plug-ins, described below.

Supported menu items are wired to working commands. Items marked **unsupported** in the app are still on the roadmap. In particular, Crop, Duplicate, image type conversion, Plot Profile, several measurement options, and parts of hyperstack interaction are not complete. See the [Phase 2 roadmap](docs/roadmap_phase2_ui_fidelity.md) for the detailed task list.

## Classic ImageJ plug-ins

The **Plugins → Installed Plugins** window can scan a classic ImageJ plug-ins directory and inspect entries. The current execution path supports a narrow subset of plain `PlugInFilter` classes through a separate Java process. Filter input must be a single 8-bit grayscale image with default metadata and either no ROI or an in-bounds rectangular ROI. Other image types, stacks, and plug-ins needing unsupported UI or host features are rejected rather than silently converted.

Plug-ins do **not** run in a security sandbox. Run only code you trust. General Java plug-in compatibility, one-click execution from the Plugins menu, and a Rust plug-in API remain future work.

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
