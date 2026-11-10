#!/usr/bin/env zsh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HOST_DIR="$REPO_ROOT/tools/plugin-host"
ORACLE_DIR="$REPO_ROOT/tools/oracle"
PATCH_JAR="$ORACLE_DIR/java-applet-shim.jar"
ORACLE_JAR="$ORACLE_DIR/ij-oracle.jar"
HOST_JAR="$HOST_DIR/plugin-host.jar"

if [[ ! -f "$HOST_JAR" ]]; then
    "$HOST_DIR/build_host.sh"
fi

exec java \
    --patch-module java.desktop="$PATCH_JAR" \
    --add-exports java.desktop/java.applet=ALL-UNNAMED \
    -Dapple.awt.UIElement=true \
    -Djava.awt.headless=true \
    -cp "$HOST_JAR:$ORACLE_JAR" \
    tools.plugin.PluginHost "$@"
