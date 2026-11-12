#!/usr/bin/env python3
"""Render the README figure from the checked-in benchmark samples.

Requires matplotlib. Run: python3 docs/benchmarks/gaussian/render.py
"""

import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Patch


ROOT = Path(__file__).resolve().parent
CASES = [
    ("noise", "8-bit stack", "1024×768 × 4 · σ=2"),
    ("gray16", "16-bit stack", "1024×768 × 4 · σ=12"),
    ("gray32", "32-bit float stack", "1024×768 × 4 · σ=2"),
    ("single2048", "8-bit single image", "2048×2048 · σ=2"),
]


def main():
    reports = [json.loads((ROOT / "data" / f"{name}.json").read_text())
               for name, _, _ in CASES]
    for report in reports:
        samples = report["samples"]
        assert len({s["fingerprint"] for s in samples}) == 1
    rows = [report["summary"][0] for report in reports]
    first = rows[0]
    speed = first["java_ms_per_op"] / first["rust_ms_per_op"]
    saving = 100 * (1 - first["rust_peak_rss_mib"] / first["java_peak_rss_mib"])

    plt.rcParams.update({"font.family": "DejaVu Sans", "font.size": 11,
                         "svg.fonttype": "none", "svg.hashsalt": "imagej-gaussian"})
    background, ink, muted = "#f8fafc", "#13243a", "#526377"
    colors = ["#71849c", "#e45d34"]
    fig, axes = plt.subplots(1, 2, figsize=(13, 8.2))
    fig.set_facecolor(background)
    fig.subplots_adjust(left=.225, right=.95, top=.69, bottom=.23, wspace=.25)
    fig.text(.045, .94, "ImageJ-rs  /  Gaussian Blur", fontsize=22, weight="bold", color=ink)
    fig.text(.045, .868, f"{speed:.1f}× faster", fontsize=29, weight="bold", color=colors[1])
    fig.text(.39, .868, f"{saving:.0f}% lower peak RSS", fontsize=29, weight="bold", color=ink)
    fig.text(.045, .823, "8-bit stack result vs ImageJ 1.54u · matching pixel fingerprints in every run",
             fontsize=12, color=muted)
    fig.legend(handles=[Patch(color=colors[0], label="Java · ImageJ 1.54u"),
                        Patch(color=colors[1], label="Rust · ImageJ-rs")],
               loc="upper right", bbox_to_anchor=(.95, .787), ncol=2, frameon=False,
               labelcolor=ink)

    for ax, suffix, title, units, limit in [
        (axes[0], "ms_per_op", "Processing time", "ms / operation · lower is better", 16),
        (axes[1], "peak_rss_mib", "Peak process memory", "MiB · lower is better", 290),
    ]:
        ax.set_facecolor(background)
        for index, row in enumerate(rows):
            for engine, offset, color in [("java", -.18, colors[0]), ("rust", .18, colors[1])]:
                value = row[f"{engine}_{suffix}"]
                ax.barh(index + offset, value, height=.28, color=color, zorder=3)
                label = f"{value:.2f}" if suffix == "ms_per_op" else f"{value:.1f}"
                ax.text(value + limit * .02, index + offset, label, va="center",
                        fontsize=11, color=ink, weight="bold" if engine == "rust" else "normal")
        ax.set_xlim(0, limit)
        ax.set_ylim(3.6, -.6)
        ax.set_yticks(range(4))
        ax.set_yticklabels([f"{title}\n{size}" for _, title, size in CASES]
                           if ax is axes[0] else [""] * 4, color=ink)
        ax.tick_params(axis="both", length=0, labelcolor=muted, pad=10)
        ax.set_title(title, loc="left", fontsize=15, color=ink, weight="bold", pad=17)
        ax.set_xlabel(units, color=muted, labelpad=12)
        ax.grid(axis="x", color="#dce3ec", linewidth=.7, zorder=0)
        for spine in ax.spines.values():
            spine.set_visible(False)

    fig.text(.045, .10, "Seeded noise · release build · worker budget 14 on both engines · Apple M4 Pro / macOS arm64",
             fontsize=10, color=muted)
    fig.text(.045, .07, "Median of 5 processes for the 8-bit stack, 3 for other cases · 3 warmups + 10 measured operations per process",
             fontsize=10, color=muted)
    fig.text(.045, .04, "RSS includes runtimes and temporary buffers. Parallel Rust uses more memory than sequential Rust. Measured 2026-11-12.",
             fontsize=10, color=muted)
    for extension in ("png", "svg"):
        fig.savefig(ROOT / f"gaussian-comparison.{extension}", dpi=180, facecolor=background,
                    metadata={"Date": None} if extension == "svg" else {})
    plt.close(fig)


if __name__ == "__main__":
    main()
