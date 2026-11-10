#!/usr/bin/env python3
"""
Deterministic content manifest verifier and generator for original_imagej/.

Protects original_imagej/ against any unauthorized modifications, additions,
or deletions without relying on git commits or git status.
Does not alter or chmod original_imagej/.
"""

import argparse
import hashlib
import os
import sys
from pathlib import Path

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


def scan_original_imagej(base_dir: Path) -> dict[str, str]:
    """Scans all files in base_dir and returns {relative_posix_path: sha256}."""
    file_hashes: dict[str, str] = {}
    for root, dirs, files in os.walk(base_dir):
        # Deterministic traversal order
        dirs.sort()
        for filename in sorted(files):
            full_path = Path(root) / filename
            rel_path = full_path.relative_to(base_dir).as_posix()
            file_hashes[rel_path] = compute_sha256(full_path)
    return file_hashes


def generate_manifest(base_dir: Path, manifest_file: Path) -> None:
    file_hashes = scan_original_imagej(base_dir)
    sorted_paths = sorted(file_hashes.keys())
    with open(manifest_file, "w", encoding="utf-8") as f:
        for rel_path in sorted_paths:
            f.write(f"{file_hashes[rel_path]}  {rel_path}\n")
    print(f"Generated manifest {manifest_file} with {len(sorted_paths)} entries.")


def check_manifest(base_dir: Path, manifest_file: Path) -> bool:
    if not manifest_file.is_file():
        print(f"ERROR: Manifest file not found: {manifest_file}", file=sys.stderr)
        return False

    expected: dict[str, str] = {}
    with open(manifest_file, "r", encoding="utf-8") as f:
        for line_num, line in enumerate(f, 1):
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split("  ", 1)
            if len(parts) != 2:
                print(f"ERROR: Invalid manifest line {line_num}: {line}", file=sys.stderr)
                return False
            expected[parts[1]] = parts[0]

    current = scan_original_imagej(base_dir)

    added = sorted(set(current.keys()) - set(expected.keys()))
    deleted = sorted(set(expected.keys()) - set(current.keys()))
    modified: list[tuple[str, str, str]] = []

    for path in sorted(set(current.keys()) & set(expected.keys())):
        if current[path] != expected[path]:
            modified.append((path, expected[path], current[path]))

    has_errors = False
    if added:
        has_errors = True
        print(f"ERROR: {len(added)} added file(s) in original_imagej/ not in manifest:", file=sys.stderr)
        for p in added:
            print(f"  + {p}", file=sys.stderr)

    if deleted:
        has_errors = True
        print(f"ERROR: {len(deleted)} missing file(s) expected by manifest:", file=sys.stderr)
        for p in deleted:
            print(f"  - {p}", file=sys.stderr)

    if modified:
        has_errors = True
        print(f"ERROR: {len(modified)} modified file(s) in original_imagej/:", file=sys.stderr)
        for p, exp, cur in modified:
            print(f"  * {p} (expected {exp[:12]}, actual {cur[:12]})", file=sys.stderr)

    if has_errors:
        return False

    print(f"Verified original_imagej/ deterministic manifest OK ({len(current)} files unchanged).")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description="Deterministic manifest manager for original_imagej/")
    parser.add_argument("--check", action="store_true", help="Check original_imagej against manifest")
    parser.add_argument("--generate", action="store_true", help="Generate or regenerate manifest")
    parser.add_argument(
        "--base-dir",
        type=Path,
        default=None,
        help="Path to original_imagej directory (defaults to repo root / original_imagej)",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=None,
        help="Path to manifest file (defaults to tools/oracle/original_imagej.manifest)",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent.parent
    base_dir = args.base_dir or (repo_root / "original_imagej")
    manifest_file = args.manifest or (repo_root / "tools/oracle/original_imagej.manifest")

    if args.generate:
        generate_manifest(base_dir, manifest_file)
        return 0
    elif args.check:
        ok = check_manifest(base_dir, manifest_file)
        return 0 if ok else 1
    else:
        parser.print_help()
        return 2


if __name__ == "__main__":
    sys.exit(main())
