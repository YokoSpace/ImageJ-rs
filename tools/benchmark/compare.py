#!/usr/bin/env python3
"""Compare ImageJ 1.54u and ImageJ-rs command time and peak process RSS."""

import argparse
import csv
import json
import os
import platform
import re
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CASES = ("invert", "gaussian", "rotate90")
RESULT_PREFIX = "BENCH_RESULT="


def parse_result(stdout: str) -> dict:
    lines = [line[len(RESULT_PREFIX):] for line in stdout.splitlines()
             if line.startswith(RESULT_PREFIX)]
    if len(lines) != 1:
        raise ValueError(f"expected one {RESULT_PREFIX} line, found {len(lines)}")
    result = json.loads(lines[0])
    required = ("case", "width", "height", "slices", "iterations", "elapsed_ns", "fingerprint")
    if any(key not in result for key in required):
        raise ValueError("incomplete benchmark result")
    if result["elapsed_ns"] <= 0:
        raise ValueError("benchmark elapsed_ns must be positive")
    return result


def parse_peak_rss_mib(stderr: str, system: str) -> float:
    if system == "darwin":
        match = re.search(r"^\s*(\d+)\s+maximum resident set size\s*$", stderr, re.MULTILINE)
        divisor = 1024 * 1024
    elif system == "linux":
        match = re.search(r"^\s*Maximum resident set size \(kbytes\):\s*(\d+)\s*$",
                          stderr, re.MULTILINE)
        divisor = 1024
    else:
        raise ValueError(f"unsupported platform for RSS measurement: {system}")
    if not match:
        raise ValueError("/usr/bin/time did not report maximum resident set size")
    return int(match.group(1)) / divisor


def check_equivalent(java: dict, rust: dict) -> None:
    for key in ("case", "width", "height", "slices", "iterations", "fingerprint"):
        if java[key] != rust[key]:
            raise ValueError(f"Java/Rust {key} mismatch: {java[key]} != {rust[key]}")


def invoke(command: list[str], system: str) -> dict:
    time_option = "-l" if system == "darwin" else "-v"
    completed = subprocess.run(["/usr/bin/time", time_option, *command], cwd=ROOT,
                               text=True, capture_output=True, timeout=600)
    if completed.returncode != 0:
        raise RuntimeError(f"command failed ({completed.returncode}): {' '.join(command)}\n"
                           f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}")
    result = parse_result(completed.stdout)
    result["peak_rss_mib"] = parse_peak_rss_mib(completed.stderr, system)
    return result


def command_output(command: list[str]) -> str:
    completed = subprocess.run(command, cwd=ROOT, text=True, capture_output=True, check=True)
    return (completed.stdout or completed.stderr).strip()


def build_workers(profile: str, classes: Path) -> tuple[list[str], list[str]]:
    subprocess.run(["python3", "tools/oracle/check_freshness.py", "--ensure"],
                   cwd=ROOT, check=True)
    subprocess.run(["cargo", "build", "-p", "ij-cli", "--profile", profile],
                   cwd=ROOT, check=True)
    oracle = ROOT / "tools/oracle/ij-oracle.jar"
    patch = ROOT / "tools/oracle/java-applet-shim.jar"
    java_flags = ["--patch-module", f"java.desktop={patch}",
                  "--add-exports", "java.desktop/java.applet=ALL-UNNAMED"]
    subprocess.run(["javac", *java_flags, "-cp", str(oracle), "-d", str(classes),
                    str(ROOT / "tools/benchmark/BenchRunner.java")], cwd=ROOT, check=True)
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target_dir.is_absolute():
        target_dir = ROOT / target_dir
    rust_binary = target_dir / ("release" if profile == "release" else "debug") / "ij-cli"
    rust_command = [str(rust_binary), "bench-worker"]
    java_command = ["java", *java_flags, "-Dapple.awt.UIElement=true",
                    "-cp", f"{oracle}{':' if sys.platform != 'win32' else ';'}{classes}",
                    "tools.benchmark.BenchRunner"]
    return java_command, rust_command


def summarize(samples: list[dict], cases: list[str]) -> list[dict]:
    rows = []
    for case in cases:
        java = [sample for sample in samples if sample["case"] == case and sample["engine"] == "java"]
        rust = [sample for sample in samples if sample["case"] == case and sample["engine"] == "rust"]
        java_ms = statistics.median(s["elapsed_ns"] / s["iterations"] / 1_000_000 for s in java)
        rust_ms = statistics.median(s["elapsed_ns"] / s["iterations"] / 1_000_000 for s in rust)
        rows.append({"case": case, "java_ms_per_op": java_ms, "rust_ms_per_op": rust_ms,
                     "java_peak_rss_mib": statistics.median(s["peak_rss_mib"] for s in java),
                     "rust_peak_rss_mib": statistics.median(s["peak_rss_mib"] for s in rust),
                     "speed_ratio_java_over_rust": java_ms / rust_ms})
    return rows


def write_outputs(out_dir: Path, report: dict) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "summary.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    columns = ("case", "run", "engine", "width", "height", "slices", "iterations",
               "elapsed_ns", "peak_rss_mib", "fingerprint")
    with (out_dir / "samples.csv").open("w", newline="") as file:
        writer = csv.DictWriter(file, fieldnames=columns)
        writer.writeheader()
        writer.writerows({key: sample[key] for key in columns} for sample in report["samples"])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", nargs="+", choices=CASES, default=list(CASES))
    parser.add_argument("--width", type=int, default=1024)
    parser.add_argument("--height", type=int, default=768)
    parser.add_argument("--slices", type=int, default=4)
    parser.add_argument("--iterations", type=int, default=5)
    parser.add_argument("--warmup", type=int, default=2)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--profile", choices=("release", "dev"), default="release")
    parser.add_argument("--out-dir", type=Path)
    args = parser.parse_args()
    if min(args.width, args.height, args.slices, args.iterations, args.runs) < 1 or args.warmup < 0:
        parser.error("dimensions, iterations and runs must be positive; warmup must be nonnegative")
    if args.width * args.height * args.slices > 100_000_000:
        parser.error("input exceeds 100 million pixels")
    system = platform.system().lower()
    if system not in ("darwin", "linux"):
        parser.error("peak RSS measurement supports macOS and Linux")
    samples = []
    fingerprints = {}
    with tempfile.TemporaryDirectory(prefix="ij-benchmark-") as temporary:
        java_command, rust_command = build_workers(args.profile, Path(temporary))
        worker_args = [str(args.width), str(args.height), str(args.slices),
                       str(args.iterations), str(args.warmup)]
        for case in args.cases:
            for run in range(1, args.runs + 1):
                order = [("java", java_command), ("rust", rust_command)]
                if run % 2 == 0:
                    order.reverse()
                pair = {}
                for engine, command in order:
                    result = invoke([*command, case, *worker_args], system)
                    result.update({"engine": engine, "run": run})
                    samples.append(result)
                    pair[engine] = result
                check_equivalent(pair["java"], pair["rust"])
                previous = fingerprints.setdefault(case, pair["java"]["fingerprint"])
                if pair["java"]["fingerprint"] != previous:
                    raise ValueError(f"{case} fingerprint changed between runs")
                print(f"{case} run {run}/{args.runs}: Java/Rust fingerprint {pair['java']['fingerprint']}",
                      flush=True)
    summary = summarize(samples, args.cases)
    report = {"settings": {"width": args.width, "height": args.height,
                           "slices": args.slices, "iterations": args.iterations,
                           "warmup": args.warmup, "runs": args.runs, "profile": args.profile},
              "environment": {"platform": platform.platform(),
                              "machine": platform.machine(),
                              "python": sys.version.split()[0],
                              "java": command_output(["java", "-version"]),
                              "rustc": command_output(["rustc", "--version"]),
                              "git_commit": command_output(["git", "rev-parse", "HEAD"]),
                              "git_dirty": bool(command_output(["git", "status", "--porcelain"]))},
              "summary": summary, "samples": samples}
    print("\ncase       Java ms/op   Rust ms/op   Java RSS MiB   Rust RSS MiB   Java/Rust time")
    for row in summary:
        print(f"{row['case']:<10} {row['java_ms_per_op']:>10.3f} {row['rust_ms_per_op']:>12.3f}"
              f" {row['java_peak_rss_mib']:>14.1f} {row['rust_peak_rss_mib']:>14.1f}"
              f" {row['speed_ratio_java_over_rust']:>16.2f}x")
    if args.out_dir:
        write_outputs(args.out_dir, report)
        print(f"Saved {args.out_dir / 'summary.json'} and {args.out_dir / 'samples.csv'}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, RuntimeError, subprocess.CalledProcessError,
            subprocess.TimeoutExpired) as error:
        print(f"benchmark failed: {error}", file=sys.stderr)
        raise SystemExit(1)
