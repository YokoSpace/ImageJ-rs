# AGENTS.md — Instructions for `deepseek-V4.1-flash` and coding agents

## Mission

You are implementing `ImageJ-rs`, a faithful Rust reproduction of ImageJ 1.54u (`original_imagej/`) with pixel-exact and measurement-exact parity and a Java compatibility session for legacy plugins.

## Strict guardrails (Do-Not-Touch fences)

1. **Never edit any file inside `original_imagej/`.** It is the read-only reference specification and the source for `tools/oracle/ij-oracle.jar`.
2. **Never edit `docs/command_ledger.md` or `crates/ij-session/src/generated_commands.rs` by hand.** Run `python3 tools/ledger/sync_ledger.py` whenever command classifications change.
3. **Never put I/O, UI, or subprocess code inside `crates/ij-core`.** `ij-core` is strictly pure domain structures and algorithms.
4. **Never advance to the next task or commit without running `./tools/verify.sh` and observing `VERIFY_ALL_OK`.**

## Mandatory workflow for every task

1. Units 1-6 in `docs/architecture_rationale.md` are complete. Current work lives in `docs/roadmap_phase2_ui_fidelity.md`. Read its section 0 (common rules, stop conditions, report format) and then only the task you were assigned.
2. Read the corresponding original Java implementation in `original_imagej/ij/` first (for example, `original_imagej/ij/process/ShortStatistics.java`). Do not guess formulas, rounding modes, or edge behavior.
3. Write a failing differential parity test in `crates/ij-session/tests/oracle_parity.rs` that runs the operation in both `JavaOracleBridge` (the real ImageJ 1.54u oracle) and Rust `Session` / `ImageDocument`. For UI tasks, write the failing test the task names (pure-function table test, `UiProbe` oracle test, or headless egui test per roadmap section 0.4).
4. Confirm the new test fails for the expected reason:
   ```bash
   cargo test --test oracle_parity
   ```
5. Implement the minimal pure domain change in `crates/ij-core`, wire the `Command` variant in `crates/ij-session` if needed, and keep comments restricted to non-obvious *why* explanations (such as citing an `original_imagej` line number for a `+0.5` offset).
6. Run the full verification lever:
   ```bash
   ./tools/verify.sh
   ```
7. Only declare the unit complete when `./tools/verify.sh` exits with code `0` and prints `VERIFY_ALL_OK`.

## Docker desktop environment

When a task needs a real X11 desktop (Linux UI regressions, window-level input), use the containerized Xfce environment instead of skipping GUI verification:

- `./docker/ci check` — build, health check, `./tools/verify.sh`, native-window smoke (Ctrl+Q clean quit), evidence copy. This wraps the same full gate; it is never a substitute or bypass for it, and every fence above still applies.
- `./docker/ci test <crate> <test_target>` — run one Rust test target inside the container for the RED→GREEN loop.
- `./docker/ci open` + `windows` / `key` / `click` / `screenshot` — inspect and drive real windows. See `docs/docker-desktop-ci.md` for details.

For an integration regression, use `./docker/ci test CRATE TARGET EXACT_NAME` for RED and GREEN. Finish with `./docker/ci check CRATE TARGET EXACT_NAME` on the plain Compose configuration. This checks that exactly one regression case passed, then verifies the workspace, native desktop, and evidence copy. For other test types, run the issue-specific test and finish with `./docker/ci check`. Do not report done on partial success: both `VERIFY_ALL_OK` and `DESKTOP_SMOKE_OK` plus the issue-specific test must pass. When a step fails, inspect the bounded tail printed by `./docker/ci` and `rg -n -m 5 'error|FAILED|panicked' <run-log> | cut -c1-240` in the `desktop-evidence/run.*/` dir; follow the stop/retry limits in roadmap section 0.2 instead of re-running the same failing gate. Never weaken tests and never guess.

## Key repository levers

- `./tools/verify.sh` — Runs ledger synchronization check, builds/verifies the Java 1.54u oracle, runs `cargo test --workspace`, executes `ij-cli slice1`, and verifies output artifacts.
- `./tools/oracle/run_oracle.sh -eval "<macro>" -batch` — Runs any ImageJ 1.54u macro directly against the unmodified Java reference implementation and prints stdout. Use this whenever you need to observe what original ImageJ does on an input.
- `python3 tools/ledger/sync_ledger.py` — Regenerates `docs/command_ledger.md` and `crates/ij-session/src/generated_commands.rs` from `original_imagej/ij/Menus.java` and `original_imagej/IJ_Props.txt`.

## Formatting and stacked pull requests

- Format only the Rust files changed for the task. For a module root, use `rustfmt --edition 2024 --config skip_children=true path/to/file.rs` and pass any edited children explicitly. Check `git diff --name-status HEAD` and `git diff --cached` before committing. In a clean worktree, `git diff --exit-code HEAD -- . ':(exclude)path/to/file.rs'` detects tracked changes outside the planned files (repeat the exclusion for each intended path). Inspect unexpected paths for pre-existing user edits before removing formatting changes.
- Merge a dependent PR after its prerequisite lands. Check the dependent PR's current base (GitHub may already have updated it), set it to main when needed, then inspect the changed files and the checks for the updated PR. Wait for Desktop CI to pass on the intended head and base before merging.
