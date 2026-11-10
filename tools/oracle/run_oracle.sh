#!/usr/bin/env zsh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ORACLE_DIR="$REPO_ROOT/tools/oracle"
PATCH_JAR="$ORACLE_DIR/java-applet-shim.jar"
ORACLE_JAR="$ORACLE_DIR/ij-oracle.jar"

# Ensure binary freshness and exec java under continuous shared lock (LOCK_SH)
# held across the entire JVM execution, preventing rebuild races.
exec python3 "$ORACLE_DIR/check_freshness.py" --exec java \
    --patch-module java.desktop="$PATCH_JAR" \
    --add-exports java.desktop/java.applet=ALL-UNNAMED \
    -Dapple.awt.UIElement=true \
    -cp "$ORACLE_JAR" \
    tools.oracle.OracleRunner "$@"

