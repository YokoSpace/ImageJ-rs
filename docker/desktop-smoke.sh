#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --locked -p ij-app
"${CARGO_TARGET_DIR:-target}/debug/ij-app" &
app_pid=$!

cleanup() {
    rc=$?
    if ((rc != 0)) && [[ -n ${SMOKE_FAILURE_PNG:-} ]]; then
        timeout 5 bash docker/gui.sh screenshot > "$SMOKE_FAILURE_PNG" 2>/dev/null || true
    fi
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
    exit "$rc"
}
trap cleanup EXIT

win_id=
for ((i = 0; i < 30; i++)); do
    if ! kill -0 "$app_pid" 2>/dev/null; then
        echo 'ImageJ-rs exited before opening a window' >&2
        exit 1
    fi
    win_id=$(xdotool search --onlyvisible --all --pid "$app_pid" --name '^ImageJ 1\.54u$' 2>/dev/null | head -n 1 || true)
    if [[ -n $win_id ]]; then
        break
    fi
    sleep 1
done
if [[ -z $win_id ]]; then
    echo 'Timed out waiting for the ImageJ-rs window' >&2
    exit 1
fi

# Drive a real UI quit on the clean main window.
bash docker/gui.sh key "$win_id" ctrl+q

for ((i = 0; i < 10; i++)); do
    if ! kill -0 "$app_pid" 2>/dev/null; then
        rc=0
        wait "$app_pid" || rc=$?
        if ((rc == 0)); then
            trap - EXIT
            echo 'DESKTOP_SMOKE_OK'
            exit 0
        fi
        echo "ImageJ-rs exited with status $rc after ctrl+q" >&2
        exit 1
    fi
    sleep 1
done
echo 'ImageJ-rs did not exit cleanly after ctrl+q' >&2
exit 1
