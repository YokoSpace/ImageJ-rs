# ImageJ 1.54u Java Oracle Tools and Shims

This directory contains the build and execution harness for the ImageJ 1.54u Java reference oracle (`ij-oracle.jar`).

## Purpose

The Java Oracle serves as an authoritative ground-truth oracle for differential parity testing against `ImageJ-rs`. It is compiled from the pristine `original_imagej/` reference source tree without altering or chmodding any files inside `original_imagej/`.

## Integrity Protection

`original_imagej/` is verified deterministically using SHA-256 hashes:
- Manifest: `tools/oracle/original_imagej.manifest`
- Verifier: `tools/oracle/check_manifest.py --check`
- Detects any modified file, added file, or deleted file without requiring git commits.

## Binary Freshness

`tools/oracle/check_freshness.py` verifies that `ij-oracle.jar` and `java-applet-shim.jar` exist and are newer than all reference sources, shims, and runner scripts. If stale or missing, `run_oracle.sh` and `check_freshness.py --ensure` rebuild them automatically.

## Truthful Documentation of Source Shims

To run ImageJ 1.54u on modern OpenJDK runtimes (such as OpenJDK 27) in a headless, automated testing environment without mutating the original source tree in `original_imagej/`, the following minimal and documented shims are applied at build time:

### 1. `java.applet.Applet` Module Shim (`tools/oracle/shim/java/applet/Applet.java`)
- **Reason:** `java.applet.Applet` was deprecated and completely removed from the standard Java SE runtime in modern JDK versions (JDK 26+).
- **Impact:** Original ImageJ 1.54u contains references to `java.applet.Applet` in method signatures and class hierarchy even when running standalone or headless.
- **Shim:** A minimal stub class `Applet extends java.awt.Panel` is compiled into `java-applet-shim.jar` and injected at compile-time and runtime via `--patch-module java.desktop=...` and `--add-exports java.desktop/java.applet=ALL-UNNAMED`.

### 2. Thread Interruption Shim (`Interpreter.java`)
- **Reason:** `Thread.stop()` was permanently removed in modern OpenJDK and throws `UnsupportedOperationException`.
- **Impact:** ImageJ's macro abort handler in `ij.macro.Interpreter.java` previously called `threads[i].stop();` when stopping macro execution threads.
- **Shim:** `threads[i].stop();` is overridden with `threads[i].interrupt();` in `tools/oracle/.build/src_override/ij/macro/Interpreter.java`.

### 3. Headless Batch Error Dialog Suppression Shim (`Interpreter.java`)
- **Reason:** In ImageJ 1.54u, `Interpreter.showError(title, msg, variables)` creates an interactive AWT modal dialog (`GenericDialog.showDialog()`) on macro errors (such as undefined identifiers or syntax errors).
- **Impact:** When running batch/CLI automation (`-batch`, `UIElement`, or headless), this modal dialog blocks the thread indefinitely waiting for a user click on an invisible window.
- **Shim:** In `Interpreter.showError()`, if running in headless or batch mode without a GUI instance (`IJ.getInstance() == null || batchMode || GraphicsEnvironment.isHeadless()`), the error message is written directly to `System.err` and the method returns immediately, allowing `Macro_Runner` to catch `Macro.MACRO_CANCELED` and abort cleanly without hanging.

### 4. Protocol Runner (`tools/oracle/runner/OracleRunner.java`)
- **Reason:** `ij.ImageJ.main` unconditionally exits with status `0` even when an `-eval` macro aborts (`[aborted]`) or encounters an unhandled exception. It also printed `[aborted]` directly to `System.out`, making it impossible to distinguish between ordinary macro stdout containing `[aborted]` and actual macro failure.
- **Shim:** `tools.oracle.OracleRunner` checks the macro evaluation outcome:
  - If the macro completes without abortion, the exit code is `0` and ordinary output is preserved.
  - If the macro aborts (via `exit(...)` or runtime error), it outputs `[oracle-macro-aborted]` to `System.err` and exits with status `1`.
  - Non-eval invocations continue to delegate directly to `ij.ImageJ.main(args)`.
