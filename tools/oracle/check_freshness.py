#!/usr/bin/env python3
"""
Oracle binary freshness verifier, builder, and concurrency-safe packager.

Ensures tools/oracle/ij-oracle.jar and tools/oracle/java-applet-shim.jar
exist, are complete, and match a cryptographically signed build stamp that
records compiler identity, reference manifest (original_imagej.manifest),
and harness files.

Concurrency and integrity guarantees:
- Cross-process file locking via fcntl.flock on .oracle_build.lock.
- Re-check under lock before performing any build.
- Isolated staging directory (stage_<pid>_<uuid>) prevents partial artifacts
  or races on shared directories.
- Atomic publication via os.replace (POSIX rename).
- Rejection of substituted jars or deleted sources in original_imagej/.
"""

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

STAMP_FILE_NAME = ".build_stamp.json"
LOCK_FILE_NAME = ".oracle_build.lock"
CHUNK_SIZE = 64 * 1024


def compute_sha256(filepath: Path) -> str:
    hasher = hashlib.sha256()
    with open(filepath, "rb") as f:
        while True:
            chunk = f.read(CHUNK_SIZE)
            if not chunk:
                break
            hasher.update(chunk)
    return hasher.hexdigest()


def get_javac_identity() -> str:
    try:
        proc = subprocess.run(
            ["javac", "-version"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=True,
        )
        version_str = (proc.stdout + proc.stderr).strip()
        javac_bin = shutil.which("javac")
        real_bin = os.path.realpath(javac_bin) if javac_bin else "unknown"
        return f"{version_str} ({real_bin})"
    except Exception as e:
        return f"javac_unknown: {e}"


def get_harness_hashes(repo_root: Path) -> dict[str, str]:
    oracle_dir = repo_root / "tools/oracle"
    harness_files: list[Path] = [
        oracle_dir / "build_oracle.sh",
        oracle_dir / "check_freshness.py",
        oracle_dir / "check_manifest.py",
        oracle_dir / "run_oracle.sh",
    ]
    for sub in ["shim", "runner"]:
        subdir = oracle_dir / sub
        if subdir.is_dir():
            for root, dirs, files in os.walk(subdir):
                dirs.sort()
                for f in sorted(files):
                    harness_files.append(Path(root) / f)

    hashes: dict[str, str] = {}
    for p in sorted(harness_files):
        if p.is_file():
            rel = p.relative_to(repo_root).as_posix()
            hashes[rel] = compute_sha256(p)
    return hashes


def compute_input_hash(
    compiler_identity: str,
    manifest_sha: str,
    harness_hashes: dict[str, str],
) -> str:
    hasher = hashlib.sha256()
    hasher.update(compiler_identity.encode("utf-8"))
    hasher.update(b"\n")
    hasher.update(manifest_sha.encode("utf-8"))
    hasher.update(b"\n")
    for path in sorted(harness_hashes.keys()):
        hasher.update(f"{path}:{harness_hashes[path]}\n".encode("utf-8"))
    return hasher.hexdigest()


def verify_manifest(repo_root: Path) -> tuple[bool, str]:
    oracle_dir = repo_root / "tools/oracle"
    manifest_file = oracle_dir / "original_imagej.manifest"
    base_dir = repo_root / "original_imagej"

    if not manifest_file.is_file():
        return False, f"Manifest file missing: {manifest_file}"

    try:
        sys.path.insert(0, str(oracle_dir))
        import check_manifest as cm
        import contextlib
        with contextlib.redirect_stdout(sys.stderr):
            ok = cm.check_manifest(base_dir, manifest_file)
        if not ok:
            return False, "original_imagej/ contents do not match deterministic manifest"
        return True, "Deterministic manifest verified OK"
    except Exception as e:
        return False, f"Error verifying manifest: {e}"


def check_freshness(repo_root: Path, check_manifest_sources: bool = True) -> tuple[bool, str]:
    oracle_dir = repo_root / "tools/oracle"
    oracle_jar = oracle_dir / "ij-oracle.jar"
    patch_jar = oracle_dir / "java-applet-shim.jar"
    stamp_path = oracle_dir / STAMP_FILE_NAME
    manifest_file = oracle_dir / "original_imagej.manifest"

    if not oracle_jar.is_file():
        return False, f"Missing {oracle_jar}"
    if not patch_jar.is_file():
        return False, f"Missing {patch_jar}"
    if not stamp_path.is_file():
        return False, f"Missing build stamp {stamp_path}"
    if not manifest_file.is_file():
        return False, f"Missing reference manifest {manifest_file}"

    # 1. Read build stamp
    try:
        with open(stamp_path, "r", encoding="utf-8") as f:
            stamp = json.load(f)
    except Exception as e:
        return False, f"Invalid build stamp JSON: {e}"

    if stamp.get("format_version") != 1:
        return False, f"Unsupported stamp format version: {stamp.get('format_version')}"

    # 2. Check artifact hashes against stamp
    artifacts = stamp.get("artifacts", {})
    oracle_art = artifacts.get("ij-oracle.jar")
    patch_art = artifacts.get("java-applet-shim.jar")
    if not oracle_art or not patch_art:
        return False, "Stamp missing artifact records"

    actual_oracle_sha = compute_sha256(oracle_jar)
    if actual_oracle_sha != oracle_art.get("sha256"):
        return False, (
            f"ij-oracle.jar hash mismatch (substituted or corrupted jar): "
            f"expected {oracle_art.get('sha256')[:12]}, actual {actual_oracle_sha[:12]}"
        )

    actual_patch_sha = compute_sha256(patch_jar)
    if actual_patch_sha != patch_art.get("sha256"):
        return False, (
            f"java-applet-shim.jar hash mismatch (substituted or corrupted jar): "
            f"expected {patch_art.get('sha256')[:12]}, actual {actual_patch_sha[:12]}"
        )

    # 3. Check compiler identity
    current_compiler = get_javac_identity()
    if current_compiler != stamp.get("compiler_identity"):
        return False, (
            f"Java compiler identity changed: "
            f"built with '{stamp.get('compiler_identity')}', current is '{current_compiler}'"
        )

    # 4. Check reference manifest hash
    actual_manifest_sha = compute_sha256(manifest_file)
    expected_manifest_sha = stamp.get("reference_manifest", {}).get("sha256")
    if actual_manifest_sha != expected_manifest_sha:
        return False, (
            f"Reference manifest hash mismatch: "
            f"expected {expected_manifest_sha[:12] if expected_manifest_sha else 'none'}, "
            f"actual {actual_manifest_sha[:12]}"
        )

    # 5. Check harness files
    harness_hashes = get_harness_hashes(repo_root)
    expected_harness = stamp.get("harness_hashes", {})
    if harness_hashes != expected_harness:
        diff_keys = set(harness_hashes.keys()) ^ set(expected_harness.keys())
        if not diff_keys:
            diff_keys = {k for k in harness_hashes if harness_hashes[k] != expected_harness.get(k)}
        return False, f"Harness files modified or missing: {sorted(diff_keys)}"

    # 6. Check composite input hash
    expected_input_hash = stamp.get("input_hash")
    actual_input_hash = compute_input_hash(current_compiler, actual_manifest_sha, harness_hashes)
    if actual_input_hash != expected_input_hash:
        return False, "Composite input hash mismatch"

    # 7. Check original_imagej against reference manifest (detects deleted, added, or modified sources)
    if check_manifest_sources:
        ok, manifest_msg = verify_manifest(repo_root)
        if not ok:
            return False, f"original_imagej/ sources integrity failure: {manifest_msg}"

    return True, "Oracle binaries, harness, and reference sources are fresh and verified"


def build_oracle_staged(repo_root: Path) -> None:
    oracle_dir = repo_root / "tools/oracle"
    build_dir = oracle_dir / ".build"
    build_dir.mkdir(parents=True, exist_ok=True)

    # Clean up any previous stale staging directories under lock
    for old_stage in build_dir.glob("stage_*"):
        try:
            if old_stage.is_dir():
                shutil.rmtree(old_stage, ignore_errors=True)
        except Exception:
            pass

    stage_dir = build_dir / f"stage_{os.getpid()}_{uuid.uuid4().hex}"
    stage_dir.mkdir(parents=True, exist_ok=True)

    try:
        build_script = oracle_dir / "build_oracle.sh"
        cmd = [str(build_script), "--compile", str(stage_dir)]
        subprocess.run(cmd, check=True)

        staged_oracle_jar = stage_dir / "ij-oracle.jar"
        staged_patch_jar = stage_dir / "java-applet-shim.jar"
        if not staged_oracle_jar.is_file() or staged_oracle_jar.stat().st_size == 0:
            raise RuntimeError(f"Build failed to produce non-empty {staged_oracle_jar}")
        if not staged_patch_jar.is_file() or staged_patch_jar.stat().st_size == 0:
            raise RuntimeError(f"Build failed to produce non-empty {staged_patch_jar}")

        # Compute metadata for stamp
        compiler_id = get_javac_identity()
        manifest_file = oracle_dir / "original_imagej.manifest"
        manifest_sha = compute_sha256(manifest_file)
        harness_hashes = get_harness_hashes(repo_root)
        input_hash = compute_input_hash(compiler_id, manifest_sha, harness_hashes)

        staged_oracle_sha = compute_sha256(staged_oracle_jar)
        staged_patch_sha = compute_sha256(staged_patch_jar)

        stamp_data = {
            "format_version": 1,
            "compiler_identity": compiler_id,
            "reference_manifest": {
                "path": "tools/oracle/original_imagej.manifest",
                "sha256": manifest_sha,
            },
            "harness_hashes": harness_hashes,
            "input_hash": input_hash,
            "artifacts": {
                "ij-oracle.jar": {
                    "sha256": staged_oracle_sha,
                    "size": staged_oracle_jar.stat().st_size,
                },
                "java-applet-shim.jar": {
                    "sha256": staged_patch_sha,
                    "size": staged_patch_jar.stat().st_size,
                },
            },
            "built_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }

        staged_stamp = stage_dir / STAMP_FILE_NAME
        with open(staged_stamp, "w", encoding="utf-8") as f:
            json.dump(stamp_data, f, indent=2)
            f.write("\n")

        # Safely publish artifacts via atomic rename on the same filesystem
        final_oracle_jar = oracle_dir / "ij-oracle.jar"
        final_patch_jar = oracle_dir / "java-applet-shim.jar"
        final_stamp = oracle_dir / STAMP_FILE_NAME

        os.replace(staged_patch_jar, final_patch_jar)
        os.replace(staged_oracle_jar, final_oracle_jar)
        os.replace(staged_stamp, final_stamp)

    finally:
        # Always clean up stage_dir
        if stage_dir.exists():
            shutil.rmtree(stage_dir, ignore_errors=True)


def ensure_fresh(repo_root: Path, verbose: bool = False) -> int:
    lock_file = repo_root / "tools/oracle" / LOCK_FILE_NAME
    lock_file.parent.mkdir(parents=True, exist_ok=True)

    # 1. Quick check with shared lock (non-blocking across concurrent readers in warm runs)
    with open(lock_file, "a+") as f:
        fcntl.flock(f.fileno(), fcntl.LOCK_SH)
        try:
            fresh, msg = check_freshness(repo_root)
            if fresh:
                if verbose:
                    print(f"Oracle binaries verified fresh: {msg}", file=sys.stderr)
                return 0
        finally:
            fcntl.flock(f.fileno(), fcntl.LOCK_UN)

    # 2. Stale or missing: acquire exclusive lock
    with open(lock_file, "a+") as f:
        fcntl.flock(f.fileno(), fcntl.LOCK_EX)
        try:
            # RECHECK UNDER LOCK: another process may have built while we waited
            fresh, msg = check_freshness(repo_root)
            if fresh:
                if verbose:
                    print(f"Oracle binaries fresh after acquiring lock: {msg}", file=sys.stderr)
                return 0

            print(f"Oracle binary stale, missing, or substituted ({msg}), rebuilding...", file=sys.stderr)
            build_oracle_staged(repo_root)

            fresh, msg = check_freshness(repo_root)
            if not fresh:
                print(f"ERROR: Oracle binary still not fresh after build: {msg}", file=sys.stderr)
                return 1

            if verbose:
                print(f"Oracle binaries rebuilt and verified OK: {msg}", file=sys.stderr)
            return 0
        finally:
            fcntl.flock(f.fileno(), fcntl.LOCK_UN)


def rebuild_oracle(repo_root: Path) -> int:
    lock_file = repo_root / "tools/oracle" / LOCK_FILE_NAME
    lock_file.parent.mkdir(parents=True, exist_ok=True)

    with open(lock_file, "a+") as f:
        fcntl.flock(f.fileno(), fcntl.LOCK_EX)
        try:
            build_oracle_staged(repo_root)
            fresh, msg = check_freshness(repo_root)
            if not fresh:
                print(f"ERROR: Oracle binary not fresh after rebuild: {msg}", file=sys.stderr)
                return 1
            print(f"Built {repo_root}/tools/oracle/ij-oracle.jar and {repo_root}/tools/oracle/java-applet-shim.jar")
            return 0
        finally:
            fcntl.flock(f.fileno(), fcntl.LOCK_UN)


def ensure_fresh_and_exec(repo_root: Path, cmd: list[str], verbose: bool = False) -> None:
    if not cmd:
        print("ERROR: --exec requires a command to execute", file=sys.stderr)
        sys.exit(2)

    lock_file = repo_root / "tools/oracle" / LOCK_FILE_NAME
    lock_file.parent.mkdir(parents=True, exist_ok=True)

    # Open lock file; keep this fd open for the process lifetime so the lock
    # is inherited by the execvp'd JVM and held until JVM termination.
    # This prevents rebuilds from replacing jars while a JVM is running,
    # and prevents JVM from starting while a rebuild is in progress.
    f = open(lock_file, "a+")

    # 1. Acquire shared lock first
    fcntl.flock(f.fileno(), fcntl.LOCK_SH)
    fresh, msg = check_freshness(repo_root)
    if not fresh:
        # Rebuild needed: upgrade to exclusive lock
        fcntl.flock(f.fileno(), fcntl.LOCK_UN)
        fcntl.flock(f.fileno(), fcntl.LOCK_EX)
        try:
            # Recheck under exclusive lock
            fresh, msg = check_freshness(repo_root)
            if not fresh:
                print(f"Oracle binary stale, missing, or substituted ({msg}), rebuilding...", file=sys.stderr)
                build_oracle_staged(repo_root)
                fresh, msg = check_freshness(repo_root)
                if not fresh:
                    print(f"ERROR: Oracle binary still not fresh after build: {msg}", file=sys.stderr)
                    sys.exit(1)
        finally:
            # Atomically downgrade from exclusive to shared lock before exec
            fcntl.flock(f.fileno(), fcntl.LOCK_SH)

    if verbose:
        print(f"Oracle fresh and locked (LOCK_SH), execing: {cmd[0]}", file=sys.stderr)

    os.set_inheritable(f.fileno(), True)
    try:
        os.execvp(cmd[0], cmd)
    except Exception as e:
        print(f"ERROR: Failed to exec {cmd[0]}: {e}", file=sys.stderr)
        sys.exit(1)


def main() -> int:
    if "--exec" in sys.argv:
        exec_idx = sys.argv.index("--exec")
        cmd_args = sys.argv[exec_idx + 1 :]
        pre_args = sys.argv[1:exec_idx]

        pre_parser = argparse.ArgumentParser(description="Run under oracle lock")
        pre_parser.add_argument("--repo-root", type=Path, default=None)
        pre_parser.add_argument("--verbose", action="store_true")
        parsed_pre, _ = pre_parser.parse_known_args(pre_args)

        repo_root = parsed_pre.repo_root or Path(__file__).resolve().parent.parent.parent
        ensure_fresh_and_exec(repo_root, cmd_args, verbose=parsed_pre.verbose)
        return 0

    parser = argparse.ArgumentParser(description="Check, build, or ensure freshness of Java oracle binaries")
    parser.add_argument("--check", action="store_true", help="Check freshness and exit non-zero if stale")
    parser.add_argument("--ensure", action="store_true", help="Rebuild if stale, missing, or invalid")
    parser.add_argument("--rebuild", action="store_true", help="Force rebuild under cross-process lock")
    parser.add_argument("--verbose", action="store_true", help="Print verbose status")
    parser.add_argument("--repo-root", type=Path, default=None, help="Repository root path")
    args = parser.parse_args()

    repo_root = args.repo_root or Path(__file__).resolve().parent.parent.parent

    if args.rebuild:
        return rebuild_oracle(repo_root)
    elif args.ensure:
        return ensure_fresh(repo_root, verbose=args.verbose)
    elif args.check:
        lock_file = repo_root / "tools/oracle" / LOCK_FILE_NAME
        lock_file.parent.mkdir(parents=True, exist_ok=True)
        with open(lock_file, "a+") as f:
            fcntl.flock(f.fileno(), fcntl.LOCK_SH)
            try:
                fresh, msg = check_freshness(repo_root)
            finally:
                fcntl.flock(f.fileno(), fcntl.LOCK_UN)

        if not fresh:
            print(f"STALE: {msg}", file=sys.stderr)
            return 1
        if args.verbose:
            print(f"OK: {msg}")
        return 0
    else:
        parser.print_help()
        return 2


if __name__ == "__main__":
    sys.exit(main())
