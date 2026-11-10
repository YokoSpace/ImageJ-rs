#!/usr/bin/env zsh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HOST_DIR="$REPO_ROOT/tools/plugin-host"
ORACLE_DIR="$REPO_ROOT/tools/oracle"
PATCH_JAR="$ORACLE_DIR/java-applet-shim.jar"
ORACLE_JAR="$ORACLE_DIR/ij-oracle.jar"

HOST_CLASSES="$HOST_DIR/classes"
HOST_JAR="$HOST_DIR/plugin-host.jar"
FIXTURES_CLASSES="$HOST_DIR/fixtures/classes"
FIXTURES_JAR="$HOST_DIR/fixtures/fixtures.jar"

# Serialize builds from the Rust bridge, run_host.sh, and run_reference_filter.sh.
# mkdir is atomic on every supported platform and works from both zsh and Rust's
# Command; the flock(1) utility is not available on stock macOS.
LOCK_DIR="$HOST_DIR/.build.lock"
STAGING_DIR=""
LOCK_WAIT_TICKS=0

cleanup() {
    if [[ -n "$STAGING_DIR" ]]; then
        rm -rf "$STAGING_DIR"
    fi
    rmdir "$LOCK_DIR" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

while ! mkdir "$LOCK_DIR" 2>/dev/null; do
    # Recover from a builder killed with SIGKILL. A compile should never take
    # ten minutes; a stale lock would otherwise wedge every later test run.
    if [[ -d "$LOCK_DIR" && -n "$(find "$LOCK_DIR" -maxdepth 0 -mmin +10 2>/dev/null)" ]]; then
        echo "Removing stale plugin-host build lock at $LOCK_DIR" >&2
        rm -rf "$LOCK_DIR"
        continue
    fi
    sleep 0.05
    LOCK_WAIT_TICKS=$((LOCK_WAIT_TICKS + 1))
    if (( LOCK_WAIT_TICKS >= 6000 )); then
        echo "Timed out waiting for plugin-host build lock at $LOCK_DIR" >&2
        exit 1
    fi
done

# Ensure oracle jar exists (dependency for ij.plugin.PlugIn interface)
if [[ ! -f "$ORACLE_JAR" || ! -f "$PATCH_JAR" ]]; then
    python3 "$ORACLE_DIR/check_freshness.py" --ensure
fi

# Compile into a private staging directory. Publish the jars with atomic
# renames so a concurrently running Java process never observes a half-written
# jar, and a failed compile leaves the previous build usable.
STAGING_DIR="$(mktemp -d "$HOST_DIR/.build.XXXXXX")"
mkdir -p "$STAGING_DIR/classes" "$STAGING_DIR/fixtures-classes"

# 1. Compile PluginHost
javac \
    --patch-module java.desktop="$PATCH_JAR" \
    --add-exports java.desktop/java.applet=ALL-UNNAMED \
    -cp "$ORACLE_JAR" \
    -d "$STAGING_DIR/classes" \
    -encoding utf-8 \
    "$HOST_DIR"/src/tools/plugin/*.java

jar cf "$STAGING_DIR/plugin-host.jar" -C "$STAGING_DIR/classes" .

# 2. Compile Fixtures
javac \
    --patch-module java.desktop="$PATCH_JAR" \
    --add-exports java.desktop/java.applet=ALL-UNNAMED \
    -cp "$ORACLE_JAR" \
    -d "$STAGING_DIR/fixtures-classes" \
    -encoding utf-8 \
    "$HOST_DIR"/fixtures/src/fixtures/*.java

jar cf "$STAGING_DIR/fixtures.jar" -C "$STAGING_DIR/fixtures-classes" .

mv -f "$STAGING_DIR/plugin-host.jar" "$HOST_JAR"
mv -f "$STAGING_DIR/fixtures.jar" "$FIXTURES_JAR"

# Class directories are only consumed by tests that pass them on the plugin
# classpath. Publish each class file with an atomic rename instead of removing
# the directory, so a Java process that is already loading fixture classes
# never sees a missing directory. Files removed from the sources may linger
# until a clean checkout; this swap is not coordinated with external javac.
publish_classes() {
    local src="$1"
    local dest="$2"
    local file
    local rel
    mkdir -p "$dest"
    find "$src" -type f | while IFS= read -r file; do
        rel="${file#$src/}"
        mkdir -p "$dest/$(dirname "$rel")"
        mv -f "$file" "$dest/$rel"
    done
}

publish_classes "$STAGING_DIR/classes" "$HOST_CLASSES"
publish_classes "$STAGING_DIR/fixtures-classes" "$FIXTURES_CLASSES"

echo "Plugin host and test fixtures built successfully."
