#!/usr/bin/env zsh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ORACLE_DIR="$REPO_ROOT/tools/oracle"

# If called directly without --compile, delegate to check_freshness.py --rebuild to ensure
# cross-process locking, isolated staging, build stamping, and atomic publication.
if [[ "${1:-}" != "--compile" ]]; then
    exec python3 "$ORACLE_DIR/check_freshness.py" --rebuild "$@"
fi

shift
STAGING_DIR="$1"
BUILD_DIR="$STAGING_DIR"
PATCH_CLASSES="$BUILD_DIR/patch_classes"
IJ_CLASSES="$BUILD_DIR/classes"
SRC_OVERRIDE="$BUILD_DIR/src_override"
PATCH_JAR="$BUILD_DIR/java-applet-shim.jar"
ORACLE_JAR="$BUILD_DIR/ij-oracle.jar"

mkdir -p "$PATCH_CLASSES" "$IJ_CLASSES" "$SRC_OVERRIDE/ij/macro"

# 1. Compile java.applet.Applet shim for JDK 26+ runtime compatibility
javac --patch-module java.desktop="$ORACLE_DIR/shim" \
    -d "$PATCH_CLASSES" \
    "$ORACLE_DIR/shim/java/applet/Applet.java"

jar cf "$PATCH_JAR" -C "$PATCH_CLASSES" .

# 2. Source shims for Interpreter.java:
#    - Replace threads[i].stop() with threads[i].interrupt() for modern JDK (Thread.stop removed)
#    - Intercept showError() in headless/batch/CLI mode to output to stderr and abort cleanly
#      rather than hanging on interactive AWT modal GenericDialog
python3 -c '
import sys
repo_root = "'"$REPO_ROOT"'"
src_override = "'"$SRC_OVERRIDE"'"

with open(f"{repo_root}/original_imagej/ij/macro/Interpreter.java", "r", encoding="utf-8") as f:
    content = f.read()

# Shim 1: Thread.stop() removal
content = content.replace("threads[i].stop();", "threads[i].interrupt();")

# Shim 2: Suppress interactive GenericDialog in headless/batch mode
target = "void showError(String title, String msg, String[] variables) {"
replacement = target + """
\t\tif (IJ.getInstance() == null || batchMode || java.awt.GraphicsEnvironment.isHeadless()) {
\t\t\tSystem.err.println(title + ": " + msg);
\t\t\treturn;
\t\t}"""
assert target in content, "showError signature not found in Interpreter.java"
content = content.replace(target, replacement, 1)

with open(f"{src_override}/ij/macro/Interpreter.java", "w", encoding="utf-8") as f:
    f.write(content)
'

# 3. Collect Java sources: original_imagej sources (with Interpreter override) + OracleRunner
find "$REPO_ROOT/original_imagej/ij" -name "*.java" ! -name "Interpreter.java" > "$BUILD_DIR/sources.txt"
echo "$SRC_OVERRIDE/ij/macro/Interpreter.java" >> "$BUILD_DIR/sources.txt"
echo "$ORACLE_DIR/runner/OracleRunner.java" >> "$BUILD_DIR/sources.txt"

# 4. Compile ImageJ classes with patched java.desktop module
javac --patch-module java.desktop="$PATCH_JAR" \
    --add-exports java.desktop/java.applet=ALL-UNNAMED \
    -d "$IJ_CLASSES" \
    -encoding utf-8 \
    @"$BUILD_DIR/sources.txt" 2>/dev/null

# 5. Copy resources and macros
cp "$REPO_ROOT/original_imagej/IJ_Props.txt" "$IJ_CLASSES/"
cp "$REPO_ROOT/original_imagej/images/microscope.gif" "$IJ_CLASSES/microscope.gif"
cp "$REPO_ROOT/original_imagej/images/about.jpg" "$IJ_CLASSES/about.jpg"
cp -R "$REPO_ROOT/original_imagej/macros" "$IJ_CLASSES/macros"

# 6. Package final oracle JAR into staging
jar cfm "$ORACLE_JAR" "$REPO_ROOT/original_imagej/MANIFEST.MF" -C "$IJ_CLASSES" .
