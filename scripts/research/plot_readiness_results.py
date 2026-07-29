#!/usr/bin/env python3
"""Plot readiness PoC outputs (no score computation — reads JSON/CSV only)."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

try:
    import matplotlib.pyplot as plt
    import pandas as pd
except ImportError as exc:
    print("Install dependencies: pip install matplotlib pandas", file=sys.stderr)
    raise SystemExit(1) from exc


def load_summary(output_dir: Path) -> dict:
    with (output_dir / "summary.json").open(encoding="utf-8") as f:
        return json.load(f)


def plot_readiness_rates(summary: dict, out: Path) -> None:
    doc = summary["documentary_summary"]
    strict = summary["strict_summary"]
    labels = ["Primary (documentary)", "Strict (full RAM)"]
    values = [doc["documentary_ready_pct"], strict["strict_ready_pct"]]
    fig, ax = plt.subplots(figsize=(7, 4))
    bars = ax.bar(labels, values, color=["#2563eb", "#dc2626"])
    ax.set_ylabel("% assets analysis-ready")
    ax.set_ylim(0, max(values + [5]) * 1.15)
    ax.set_title("FMUCD — analysis readiness by profile")
    for bar, v in zip(bars, values):
        ax.text(bar.get_x() + bar.get_width() / 2, bar.get_height() + 0.5, f"{v:.1f}%", ha="center", fontsize=10)
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    plt.close(fig)


def plot_blocking_breakdown(summary: dict, out: Path) -> None:
    strict = summary["strict_summary"]["blocking_breakdown"]
    if not strict:
        return
    codes = list(strict.keys())
    counts = [strict[c] for c in codes]
    fig, ax = plt.subplots(figsize=(7, 4))
    ax.barh(codes, counts, color="#7c3aed")
    ax.set_xlabel("Assets with blocking issue")
    ax.set_title("Strict profile — blocking issue breakdown")
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    plt.close(fig)


def plot_dimensions(summary: dict, out: Path) -> None:
    dims = summary["documentary_summary"]["mean_dimensions"]
    labels = ["Equipment ID", "Time interval", "Failure mode", "Corrective closure", "Global C"]
    values = [
        dims["dim_equipment_id_pct"],
        dims["dim_failure_interval_pct"],
        dims["dim_failure_mode_pct"],
        dims["dim_corrective_closure_pct"],
        dims["completeness_percent"],
    ]
    saturated_color = "#059669"
    interval_color = "#dc2626"
    aggregate_color = "#64748b"
    colors = [saturated_color, interval_color, saturated_color, saturated_color, aggregate_color]
    fig, ax = plt.subplots(figsize=(8, 4.5))
    bars = ax.bar(labels, values, color=colors, edgecolor="black", linewidth=0.6, zorder=2)
    bars[1].set_hatch("///")
    bars[1].set_edgecolor("#991b1b")
    for bar, v in zip(bars, values):
        ax.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height() + 1.5,
            f"{v:.1f}%",
            ha="center",
            va="bottom",
            fontsize=9,
            fontweight="bold" if bar is bars[1] else "normal",
        )
    ax.annotate(
        "Hidden by\naggregate score",
        xy=(4, values[4]),
        xytext=(1.15, 72),
        fontsize=8.5,
        ha="center",
        color="#334155",
        arrowprops=dict(arrowstyle="->", color="#334155", lw=1.1, connectionstyle="arc3,rad=-0.25"),
    )
    ax.set_ylabel("Mean %")
    ax.set_ylim(0, 108)
    ax.set_title("Mean ISO 14224-inspired completeness dimensions")
    plt.xticks(rotation=15, ha="right")
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    plt.close(fig)


def plot_university(csv_path: Path, out: Path) -> None:
    if not csv_path.exists():
        return
    df = pd.read_csv(csv_path)
    fig, ax = plt.subplots(figsize=(10, 5))
    x = range(len(df))
    labels = df["university_id"].astype(str)
    ax.bar([i - 0.2 for i in x], df["documentary_ready_pct"], width=0.4, label="Primary %", color="#2563eb")
    ax.bar([i + 0.2 for i in x], df["strict_ready_pct"], width=0.4, label="Strict %", color="#dc2626")
    ax.set_xticks(list(x))
    ax.set_xticklabels(labels)
    ax.set_xlabel("University ID")
    ax.set_ylabel("% ready (eligible assets)")
    ax.set_title("Readiness by university")
    ax.legend()
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    plt.close(fig)


def plot_sensitivity_nmin(csv_path: Path, out: Path) -> None:
    if not csv_path.exists():
        return
    df = pd.read_csv(csv_path)
    fig, ax = plt.subplots(figsize=(7, 4))
    ax.plot(df["n_min"], df["documentary_ready_pct"], marker="o", label="Primary %", color="#2563eb")
    ax.plot(df["n_min"], df["strict_ready_pct"], marker="s", label="Strict %", color="#dc2626")
    ax.set_xlabel("N_min (minimum sample size)")
    ax.set_ylabel("% ready (eligible assets)")
    ax.set_title("Sensitivity to minimum sample size")
    ax.legend()
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    plt.close(fig)


def plot_mapping_sensitivity(csv_path: Path, out: Path) -> None:
    if not csv_path.exists():
        return
    df = pd.read_csv(csv_path)
    fig, ax = plt.subplots(figsize=(8, 4))
    x = range(len(df))
    labels = df["mapping_variant"].astype(str)
    ax.bar([i - 0.2 for i in x], df["documentary_ready_pct"], width=0.4, label="Primary %", color="#2563eb")
    ax.bar([i + 0.2 for i in x], df["mean_interval_dim_pct"], width=0.4, label="Mean interval dim %", color="#059669")
    ax.set_xticks(list(x))
    ax.set_xticklabels(labels)
    ax.set_ylabel("%")
    ax.set_title("Sensitivity to mapping assumptions")
    ax.legend()
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    plt.close(fig)


def plot_baseline_comparison(json_path: Path, out: Path) -> None:
    if not json_path.exists():
        return
    with json_path.open(encoding="utf-8") as f:
        data = json.load(f)
    scenarios = data["scenarios"]
    labels = [s["scenario"].replace("_", "\n") for s in scenarios]
    values = [s["pct_of_eligible"] for s in scenarios]
    fig, ax = plt.subplots(figsize=(9, 4))
    bars = ax.bar(labels, values, color=["#94a3b8", "#64748b", "#2563eb", "#dc2626"])
    ax.set_ylabel("% of eligible assets")
    ax.set_title("Baseline comparison: naive entry vs framework gating")
    for bar, v in zip(bars, values):
        ax.text(bar.get_x() + bar.get_width() / 2, bar.get_height() + 0.5, f"{v:.1f}%", ha="center", fontsize=9)
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    plt.close(fig)


def plot_decomposition_waterfall(csv_path: Path, out: Path) -> None:
    if not csv_path.exists():
        return
    df = pd.read_csv(csv_path)
    label_map = {
        "DOCUMENTARY_READY": "ALLOW",
        "LOW_DQ": "LOW_DQ",
        "INTERVAL_GAP": "INTERVAL_GAP",
        "MULTIPLE_GATES": "MULTIPLE_GATES",
    }
    df = df.copy()
    df["label"] = df["reason"].map(label_map).fillna(df["reason"])
    df = df.sort_values("pct_of_eligible", ascending=True)
    fig, ax = plt.subplots(figsize=(8, 5))
    colors = ["#059669" if r == "DOCUMENTARY_READY" else "#dc2626" for r in df["reason"]]
    ax.barh(df["label"], df["pct_of_eligible"], color=colors)
    ax.set_xlabel("% of eligible assets")
    ax.set_title("Primary eligibility denial reasons")
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    plt.close(fig)


def main() -> None:
    parser = argparse.ArgumentParser(description="Plot readiness PoC results")
    parser.add_argument("output_dir", type=Path, help="Directory with summary.json")
    args = parser.parse_args()
    out_dir = args.output_dir
    figures = out_dir / "figures"
    figures.mkdir(parents=True, exist_ok=True)
    analysis = out_dir / "analysis"

    summary = load_summary(out_dir)
    plot_readiness_rates(summary, figures / "01_readiness_rates.png")
    plot_blocking_breakdown(summary, figures / "02_blocking_breakdown_strict.png")
    plot_dimensions(summary, figures / "03_completeness_dimensions.png")
    plot_university(out_dir / "by_university_documentary.csv", figures / "04_by_university.png")

    plot_sensitivity_nmin(analysis / "sensitivity_nmin.csv", figures / "05_sensitivity_nmin.png")
    plot_mapping_sensitivity(analysis / "sensitivity_mapping.csv", figures / "06_mapping_sensitivity.png")
    plot_baseline_comparison(analysis / "baseline_comparison.json", figures / "07_baseline_comparison.png")
    plot_decomposition_waterfall(analysis / "decomposition_waterfall.csv", figures / "08_decomposition_waterfall.png")

    print(f"Figures written to {figures}")


if __name__ == "__main__":
    main()
