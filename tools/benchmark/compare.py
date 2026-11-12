#!/usr/bin/env python3
"""Compare ImageJ 1.54u and ImageJ-rs command time and peak process RSS.

Both engines run the same command on the same deterministic input and must
produce the same pixel fingerprint; a mismatch aborts the run instead of
reporting a time for different work.
"""

import argparse
import csv
import json
import math
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
TYPES = (8, 16, 32)
INPUTS = ("noise", "ramp", "impulse")
PROFILES = ("release", "dev")
DEFAULT_SEED = 0x2545F4914F6CDD1D
MAX_SIGMA = 5.0e8
MAX_PIXELS = 100_000_000
MAX_JAVA_INT = 2 ** 31 - 1
RESULT_PREFIX = "BENCH_RESULT="

# Fields both engines must agree on. Thread counts are deliberately excluded:
# they describe each engine's own executor, not the work that was done.
COMPARED_FIELDS = ("case", "type", "sigma", "input", "seed", "width", "height",
                   "slices", "iterations", "fingerprint")


def parse_result(stdout: str) -> dict:
    lines = [line[len(RESULT_PREFIX):] for line in stdout.splitlines()
             if line.startswith(RESULT_PREFIX)]
    if len(lines) != 1:
        raise ValueError(f"expected one {RESULT_PREFIX} line, found {len(lines)}")
    result = json.loads(lines[0])
    required = ("case", "type", "sigma", "input", "seed", "width", "height", "slices",
                "iterations", "elapsed_ns", "fingerprint", "threads", "machine_threads")
    missing = [key for key in required if key not in result]
    if missing:
        raise ValueError(f"incomplete benchmark result: missing {', '.join(missing)}")
    if result["elapsed_ns"] <= 0:
        raise ValueError("benchmark elapsed_ns must be positive")
    threads = result["threads"]
    if not isinstance(threads, dict) or set(threads) != {"requested", "resolved", "policy"}:
        raise ValueError("benchmark result must report threads.requested/resolved/policy")
    if threads["resolved"] is not None and threads["resolved"] < 1:
        raise ValueError("benchmark threads.resolved must be positive when reported")
    if not isinstance(threads["policy"], str) or not threads["policy"]:
        raise ValueError("benchmark threads.policy must be a non-empty string")
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
    for key in COMPARED_FIELDS:
        if java[key] != rust[key]:
            raise ValueError(f"Java/Rust {key} mismatch: {java[key]} != {rust[key]}")


def build_settings(cases, width, height, slices, iterations, warmup, runs, profile,
                   type_bits, sigma, input_name, seed, java_threads, rust_threads) -> dict:
    """Validates and normalizes one benchmark configuration.

    Raises ValueError with a user-facing message; `main` turns that into an
    argparse error so a bad invocation never reaches a worker.
    """
    cases = list(cases)
    if not cases:
        raise ValueError("at least one case is required")
    unknown = [case for case in cases if case not in CASES]
    if unknown:
        raise ValueError(f"unknown case: {', '.join(unknown)}")
    for name, value in (("width", width), ("height", height), ("slices", slices),
                        ("iterations", iterations), ("runs", runs)):
        if value < 1:
            raise ValueError(f"{name} must be positive, got {value}")
    if warmup < 0:
        raise ValueError(f"warmup must be non-negative, got {warmup}")
    for name, value in (("width", width), ("height", height), ("slices", slices),
                        ("iterations", iterations), ("warmup", warmup),
                        ("java-threads", java_threads), ("rust-threads", rust_threads)):
        if value is not None and value > MAX_JAVA_INT:
            raise ValueError(f"{name} must be at most {MAX_JAVA_INT}, got {value}")
    if profile not in PROFILES:
        raise ValueError(f"unknown profile: {profile}")
    if type_bits not in TYPES:
        raise ValueError(f"unknown type: {type_bits} (expected 8, 16 or 32)")
    sigma = float(sigma)
    if not math.isfinite(sigma) or sigma <= 0.0 or sigma > MAX_SIGMA:
        raise ValueError(f"sigma must be finite, positive and at most {MAX_SIGMA}, got {sigma}")
    if input_name not in INPUTS:
        raise ValueError(f"unknown input: {input_name} (expected {', '.join(INPUTS)})")
    if not isinstance(seed, int) or isinstance(seed, bool) or not 0 <= seed < 2 ** 64:
        raise ValueError(f"seed must be an unsigned 64-bit integer, got {seed}")
    for name, value in (("java-threads", java_threads), ("rust-threads", rust_threads)):
        if value is not None and value < 1:
            raise ValueError(f"{name} must be positive, got {value}")
    if width * height * slices > MAX_PIXELS:
        raise ValueError(f"input exceeds {MAX_PIXELS} pixels")
    return {"cases": cases, "width": width, "height": height, "slices": slices,
            "iterations": iterations, "warmup": warmup, "runs": runs, "profile": profile,
            "type": type_bits, "sigma": sigma, "input": input_name, "seed": seed,
            "java_threads": java_threads, "rust_threads": rust_threads}


def worker_options(settings: dict, threads) -> list:
    """Builds the `key=value` worker options for one engine.

    `threads` is per engine: Java applies it through `Prefs.setThreads`, while
    Rust passes it to the Gaussian blur's explicit worker-budget API.
    """
    options = [f"type={settings['type']}", f"sigma={settings['sigma']!r}",
               f"input={settings['input']}", f"seed={settings['seed']}"]
    if threads is not None:
        options.append(f"threads={threads}")
    return options


def invoke(command: list, system: str) -> dict:
    time_option = "-l" if system == "darwin" else "-v"
    completed = subprocess.run(["/usr/bin/time", time_option, *command], cwd=ROOT,
                               text=True, capture_output=True, timeout=600)
    if completed.returncode != 0:
        raise RuntimeError(f"command failed ({completed.returncode}): {' '.join(command)}\n"
                           f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}")
    result = parse_result(completed.stdout)
    result["peak_rss_mib"] = parse_peak_rss_mib(completed.stderr, system)
    return result


def invoke_pair(java_command: list, rust_command: list, case: str, worker_args: list,
                settings: dict, system: str, java_first: bool = True) -> dict:
    """Runs one Java/Rust pair and returns `{engine: result}` after checking parity."""
    commands = {
        "java": [*java_command, case, *worker_args,
                 *worker_options(settings, settings["java_threads"])],
        "rust": [*rust_command, case, *worker_args,
                 *worker_options(settings, settings["rust_threads"])],
    }
    order = ("java", "rust") if java_first else ("rust", "java")
    pair = {}
    for engine in order:
        result = invoke(commands[engine], system)
        result["engine"] = engine
        pair[engine] = result
    check_equivalent(pair["java"], pair["rust"])
    return pair


def command_output(command: list) -> str:
    completed = subprocess.run(command, cwd=ROOT, text=True, capture_output=True, check=True)
    return (completed.stdout or completed.stderr).strip()


def build_workers(profile: str, classes: Path) -> tuple:
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


def _single(values, label: str):
    distinct = set(values)
    if len(distinct) != 1:
        raise ValueError(f"{label} changed between runs: {sorted(distinct)}")
    return distinct.pop()


def summarize(samples: list, cases: list) -> list:
    rows = []
    for case in cases:
        java = [sample for sample in samples if sample["case"] == case and sample["engine"] == "java"]
        rust = [sample for sample in samples if sample["case"] == case and sample["engine"] == "rust"]
        java_ms = statistics.median(s["elapsed_ns"] / s["iterations"] / 1_000_000 for s in java)
        rust_ms = statistics.median(s["elapsed_ns"] / s["iterations"] / 1_000_000 for s in rust)
        rows.append({
            "case": case,
            "java_ms_per_op": java_ms,
            "rust_ms_per_op": rust_ms,
            "java_peak_rss_mib": statistics.median(s["peak_rss_mib"] for s in java),
            "rust_peak_rss_mib": statistics.median(s["peak_rss_mib"] for s in rust),
            "speed_ratio_java_over_rust": java_ms / rust_ms,
            "java_threads_resolved": _single(
                (s["threads"]["resolved"] for s in java), f"{case} Java threads"),
            "rust_threads_resolved": _single(
                (s["threads"]["resolved"] for s in rust), f"{case} Rust threads"),
            "rust_thread_policy": _single(
                (s["threads"]["policy"] for s in rust), f"{case} Rust thread policy"),
            "machine_threads": _single(
                (s["machine_threads"] for s in java), f"{case} machine threads"),
        })
    return rows


def write_outputs(out_dir: Path, report: dict) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "summary.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    columns = ("case", "run", "engine", "type", "sigma", "input", "seed", "width", "height",
               "slices", "iterations", "elapsed_ns", "peak_rss_mib", "threads_requested",
               "threads_resolved", "threads_policy", "machine_threads", "fingerprint")
    with (out_dir / "samples.csv").open("w", newline="") as file:
        writer = csv.DictWriter(file, fieldnames=columns)
        writer.writeheader()
        for sample in report["samples"]:
            row = {key: sample[key] for key in columns if key in sample}
            row["threads_requested"] = sample["threads"]["requested"]
            row["threads_resolved"] = sample["threads"]["resolved"]
            row["threads_policy"] = sample["threads"]["policy"]
            writer.writerow(row)


def parse_threads(value: str):
    if value == "default":
        return None
    try:
        count = int(value)
    except ValueError:
        raise argparse.ArgumentTypeError(f"expected 'default' or a positive integer, got {value}")
    if count < 1:
        raise argparse.ArgumentTypeError(f"expected 'default' or a positive integer, got {value}")
    return count


def parse_seed(value: str) -> int:
    try:
        seed = int(value, 0) if value.lower().startswith(("0x", "0o", "0b")) else int(value, 10)
    except ValueError:
        raise argparse.ArgumentTypeError(f"expected an unsigned 64-bit integer, got {value}")
    if not 0 <= seed < 2 ** 64:
        raise argparse.ArgumentTypeError(f"expected an unsigned 64-bit integer, got {value}")
    return seed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", nargs="+", choices=CASES, default=list(CASES))
    parser.add_argument("--width", type=int, default=1024)
    parser.add_argument("--height", type=int, default=768)
    parser.add_argument("--slices", type=int, default=4)
    parser.add_argument("--iterations", type=int, default=5)
    parser.add_argument("--warmup", type=int, default=2)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--profile", choices=PROFILES, default="release")
    parser.add_argument("--type", dest="type_bits", type=int, choices=TYPES, default=8,
                        help="pixel type of the generated input (default: 8)")
    parser.add_argument("--sigma", type=float, default=2.0,
                        help="Gaussian sigma for the gaussian case (default: 2.0)")
    parser.add_argument("--input", dest="input_name", choices=INPUTS, default="noise",
                        help="deterministic input generator (default: noise)")
    parser.add_argument("--seed", type=parse_seed, default=DEFAULT_SEED,
                        help=f"seed for --input noise/impulse (default: {DEFAULT_SEED:#x})")
    parser.add_argument("--java-threads", type=parse_threads, default=None,
                        help="ImageJ Prefs.setThreads value, or 'default' to leave it alone")
    parser.add_argument("--rust-threads", type=parse_threads, default=None,
                        help="ij-core worker budget for the gaussian case; default uses the"
                             " Session default")
    parser.add_argument("--out-dir", type=Path)
    args = parser.parse_args()
    try:
        settings = build_settings(args.cases, args.width, args.height, args.slices,
                                  args.iterations, args.warmup, args.runs, args.profile,
                                  args.type_bits, args.sigma, args.input_name, args.seed,
                                  args.java_threads, args.rust_threads)
    except ValueError as error:
        parser.error(str(error))
    system = platform.system().lower()
    if system not in ("darwin", "linux"):
        parser.error("peak RSS measurement supports macOS and Linux")

    samples = []
    fingerprints = {}
    with tempfile.TemporaryDirectory(prefix="ij-benchmark-") as temporary:
        java_command, rust_command = build_workers(settings["profile"], Path(temporary))
        worker_args = [str(settings["width"]), str(settings["height"]), str(settings["slices"]),
                       str(settings["iterations"]), str(settings["warmup"])]
        for case in settings["cases"]:
            for run in range(1, settings["runs"] + 1):
                pair = invoke_pair(java_command, rust_command, case, worker_args, settings,
                                   system, java_first=run % 2 == 1)
                for engine, result in pair.items():
                    result["run"] = run
                    samples.append(result)
                previous = fingerprints.setdefault(case, pair["java"]["fingerprint"])
                if pair["java"]["fingerprint"] != previous:
                    raise ValueError(f"{case} fingerprint changed between runs")
                print(f"{case} run {run}/{settings['runs']}: Java/Rust fingerprint "
                      f"{pair['java']['fingerprint']}", flush=True)
    summary = summarize(samples, settings["cases"])
    report = {"settings": settings,
              "environment": {"platform": platform.platform(),
                              "machine": platform.machine(),
                              "python": sys.version.split()[0],
                              "java": command_output(["java", "-version"]),
                              "rustc": command_output(["rustc", "--version"]),
                              "git_commit": command_output(["git", "rev-parse", "HEAD"]),
                              "git_dirty": bool(command_output(["git", "status", "--porcelain"]))},
              "summary": summary, "samples": samples}
    print(f"\ninput={settings['input']} type={settings['type']} sigma={settings['sigma']} "
          f"seed={settings['seed']} size={settings['width']}x{settings['height']}"
          f"x{settings['slices']}")
    print("case       Java ms/op   Rust ms/op   Java RSS MiB   Rust RSS MiB   Java/Rust time"
          "   Java thr   Rust thr")
    for row in summary:
        rust_threads = row["rust_threads_resolved"]
        rust_label = row["rust_thread_policy"] if rust_threads is None else str(rust_threads)
        print(f"{row['case']:<10} {row['java_ms_per_op']:>10.3f} {row['rust_ms_per_op']:>12.3f}"
              f" {row['java_peak_rss_mib']:>14.1f} {row['rust_peak_rss_mib']:>14.1f}"
              f" {row['speed_ratio_java_over_rust']:>16.2f}x"
              f" {row['java_threads_resolved']:>10} {rust_label:>10}")
    if settings["rust_threads"] is not None and any(
            row["rust_thread_policy"] == "sequential" for row in summary):
        print(f"\nnote: --rust-threads {settings['rust_threads']} only applies to the gaussian"
              " case; invert and rotate90 are sequential in both engines.")
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
