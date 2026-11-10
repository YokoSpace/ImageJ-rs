# Command ledger synchronization

## Sub-features
- Extraction of hardcoded menu items from `original_imagej/ij/Menus.java` and properties-driven items from `original_imagej/IJ_Props.txt`.
- Generation of `docs/command_ledger.md` and `crates/ij-session/src/generated_commands.rs`.

## How to get to it (user POV)
Run `python3 tools/ledger/sync_ledger.py --check`.

## Driving it with `sync_ledger.py`
```bash
python3 tools/ledger/sync_ledger.py --check
```
Observable end state:
- Prints `Ledger check passed (414 commands synchronized).` and exits with code `0`.

## Gotchas
- Never edit `docs/command_ledger.md` or `crates/ij-session/src/generated_commands.rs` directly; edit `RUST_SLICE_1_COMMANDS` in `tools/ledger/sync_ledger.py` and re-run the script.
