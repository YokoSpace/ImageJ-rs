#!/usr/bin/env zsh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

echo "[1/6] Verifying original_imagej/ integrity against deterministic manifest and oracle freshness..."
python3 tools/oracle/check_manifest.py --check
python3 tools/oracle/check_freshness.py --ensure
./tools/plugin-host/build_host.sh

echo "[2/6] Checking command ledger sync..."
python3 tools/ledger/sync_ledger.py --check

echo "[3/6] Rejecting compiler warnings in test targets..."
WARNING_LOG="$(mktemp "${TMPDIR:-/tmp}/ij-verify-warnings.XXXXXX")"
cargo test --workspace --no-run 2>&1 | tee "$WARNING_LOG"
if grep -q '^warning' "$WARNING_LOG"; then
  rm -f "$WARNING_LOG"
  echo "VERIFY_FAIL: compiler warnings" >&2
  exit 1
fi
rm -f "$WARNING_LOG"

echo "[4/6] Running Java 1.54u oracle & Rust doctor..."
cargo run --quiet --bin ij-cli -- doctor

echo "[5/6] Running cargo test --workspace (unit + differential oracle parity)..."
cargo test --workspace --quiet

echo "[6/6] Driving Vertical Slice 1 end-to-end and capturing proof artifacts..."
EVIDENCE_DIR="$REPO_ROOT/verification/evidence/latest"
rm -rf "$EVIDENCE_DIR"
mkdir -p "$EVIDENCE_DIR"
cargo run --quiet --bin ij-cli -- slice1 --out-dir "$EVIDENCE_DIR" > "$EVIDENCE_DIR/slice1_stdout.txt"
cargo run --quiet --bin ij-cli -- parity-check > "$EVIDENCE_DIR/parity_stdout.txt"

test -s "$EVIDENCE_DIR/Results.tsv"
test -s "$EVIDENCE_DIR/ramp_saved.tif"
test -s "$EVIDENCE_DIR/recorded_macro.ijm"
grep -q "PARITY_OK" "$EVIDENCE_DIR/parity_stdout.txt"

echo "VERIFY_ALL_OK (evidence saved in $EVIDENCE_DIR)"
