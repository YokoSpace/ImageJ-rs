#!/usr/bin/env bash
# Run inside the desktop container on the app display.
# Subcommands: windows | key ID KEY | click ID X Y | screenshot
set -euo pipefail

app_ids() {
    local pid id
    for pid in $(pgrep -x -u "$(id -u)" ij-app || true); do
        for id in $(xdotool search --onlyvisible --pid "$pid" 2>/dev/null || true); do
            printf '%s\n' "$id"
        done
    done
}

require_app_window() {
    local id=$1 pid comm
    [[ $id =~ ^[0-9]+$ ]] || { echo "Invalid window ID: $id" >&2; exit 2; }
    pid=$(xdotool getwindowpid "$id" 2>/dev/null) || { echo "No such window: $id" >&2; exit 2; }
    comm=$(ps -o comm= -p "$pid" 2>/dev/null) || { echo "No such window: $id" >&2; exit 2; }
    comm=${comm//[[:space:]]/}
    [[ $comm == ij-app ]] || { echo "Window $id is not owned by ij-app" >&2; exit 2; }
    timeout 4 xdotool windowactivate --sync "$id"
}

cmd=${1:-}
case "$cmd" in
    windows)
        app_ids | while IFS= read -r id; do
            printf '%s\t%s\n' "$id" "$(xdotool getwindowname "$id" 2>/dev/null || true)"
        done
        ;;
    key)
        id=${2:?usage: gui.sh key ID KEY}
        key=${3:?usage: gui.sh key ID KEY}
        require_app_window "$id"
        xdotool key --clearmodifiers "$key"
        ;;
    click)
        id=${2:?usage: gui.sh click ID X Y}
        x=${3:?usage: gui.sh click ID X Y}
        y=${4:?usage: gui.sh click ID X Y}
        [[ $x =~ ^[0-9]+$ && $y =~ ^[0-9]+$ ]] || { echo 'X and Y must be integers >= 0' >&2; exit 2; }
        require_app_window "$id"
        w= h=
        while IFS== read -r k v; do
            case $k in
                WIDTH)  w=$v ;;
                HEIGHT) h=$v ;;
            esac
        done < <(xdotool getwindowgeometry --shell "$id")
        w=$((10#$w)); h=$((10#$h))
        ((x < w && y < h)) || { echo "click $x,$y is outside window $id (${w}x${h})" >&2; exit 2; }
        xdotool mousemove --window "$id" "$x" "$y" click 1
        ;;
    screenshot)
        tmp=$(mktemp /tmp/imagej-XXXXXX.png)
        xfce4-screenshooter -f -s "$tmp"
        cat "$tmp"
        rm -f "$tmp"
        ;;
    *)
        echo 'usage: gui.sh {windows|key ID KEY|click ID X Y|screenshot}' >&2
        exit 2
        ;;
esac
