# Vertical Slice 1: TIFF open, display range, ROI, Measure, and save

## Sub-features
- Uncompressed 8-bit, 16-bit, 32-bit float, and RGB TIFF load and save (`ij-io`).
- Display range (`setMinAndMax`) and rectangular ROI clamping (`ij-core`).
- Area, Mean, StdDev, Min, Max, Centroid, and Center of Mass calculation (`ij-core`).
- Macro command recording (`Session.macro_recorder`) and TSV export (`Results.tsv`).

## How to get to it (user POV)
Run `ij-cli slice1 --out-dir <dir>` from the command line or dispatch `Command::OpenTiff`, `Command::SetDisplayRange`, `Command::MakeRectRoi`, `Command::Measure`, `Command::SaveResultsTsv`, and `Command::SaveTiff` on `ij_session::Session`.

## Driving it with `ij-cli`
```bash
cargo run --quiet --bin ij-cli -- slice1 --out-dir /Users/nauti/GitHub/ImageJ-rs/verification/evidence/latest
```
Observable end state:
- Exit code `0`.
- `verification/evidence/latest/Results.tsv` contains header ` \tArea\tMean\tMin\tMax` and row `1\t48\t88\t32\t144`.
- `verification/evidence/latest/ramp_saved.tif` is a valid 16x16 TIFF file.
- `verification/evidence/latest/recorded_macro.ijm` contains the 6 recorded ImageJ macro statements.

## Gotchas
- Centroid coordinates in ImageJ 1.54u add `+0.5` (`ByteStatistics.java` lines 115-116) to report pixel centers rather than top-left corners.
