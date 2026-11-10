# Differential oracle parity against ImageJ 1.54u

## Sub-features
- Headless batch execution of unmodified ImageJ 1.54u (`tools/oracle/run_oracle.sh`).
- Macro evaluation (`JavaOracleBridge::eval_macro`) and TIFF round-trip pixel comparison (`JavaOracleBridge::run_macro_on_image`).

## How to get to it (user POV)
Run `ij-cli parity-check` or `cargo test --test oracle_parity`.

## Driving it with `ij-cli`
```bash
cargo run --quiet --bin ij-cli -- parity-check
cargo test --test oracle_parity
```
Observable end state:
- `ij-cli parity-check` prints `PARITY_OK` and exits with code `0`.
- `oracle_parity` integration tests pass both measurement table comparison and ROI `Invert` pixel-by-pixel comparison.

## Gotchas
- Passing `-Djava.awt.headless=true` to ImageJ 1.x throws `java.awt.HeadlessException` when `IJ.run` initializes `Menus.addMenuBar`. Always use `-Dapple.awt.UIElement=true` on macOS (handled automatically by `tools/oracle/run_oracle.sh`).
- JDK 26+ removed `java.applet.Applet`. `tools/oracle/build_oracle.sh` supplies `tools/oracle/java-applet-shim.jar` via `--patch-module java.desktop` so `original_imagej/` stays untouched.
