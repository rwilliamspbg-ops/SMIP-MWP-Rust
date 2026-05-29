#!/usr/bin/env python3
"""Render simple benchmark plots from the summarized CSV.

Usage:
  plot_results.py [summary_csv]

Defaults to tools/bench_harness/bench_summary.csv if no path is provided.
Generates:
  docs/bench_performance.png
  docs/bench_market_compare.png
"""
from __future__ import annotations

import csv
import sys
from collections import defaultdict
from pathlib import Path

import matplotlib.pyplot as plt


def load_rows(path: Path):
    rows = []
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        for row in reader:
            try:
                rows.append(
                    {
                        "strategy": row["strategy"],
                        "size": int(row["size"]),
                        "runs": int(row["runs"]),
                        "mean_avg_ns": float(row["mean_avg_ns"]),
                        "stddev_avg_ns": float(row["stddev_avg_ns"]),
                    }
                )
            except (KeyError, TypeError, ValueError):
                continue
    rows.sort(key=lambda item: (item["strategy"], item["size"]))
    return rows


def plot_performance(rows, output_path: Path):
    grouped = defaultdict(list)
    for row in rows:
        grouped[row["strategy"]].append(row)

    plt.figure(figsize=(10, 6))
    for strategy, entries in sorted(grouped.items()):
        entries.sort(key=lambda item: item["size"])
        sizes = [entry["size"] for entry in entries]
        throughput_mib_s = [1_000_000_000.0 / entry["mean_avg_ns"] / (1024.0 * 1024.0) for entry in entries]
        plt.plot(sizes, throughput_mib_s, marker="o", label=strategy)

    plt.title("Benchmark throughput by strategy")
    plt.xlabel("Size")
    plt.ylabel("Throughput (MiB/s, derived from mean_avg_ns)")
    plt.grid(True, alpha=0.3)
    plt.legend(fontsize=8)
    plt.tight_layout()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    plt.savefig(output_path, dpi=160)
    plt.close()


def plot_market_compare(rows, output_path: Path):
    strategies = sorted({row["strategy"] for row in rows})
    if not strategies:
        return

    best_by_strategy = {}
    for strategy in strategies:
        entries = [row for row in rows if row["strategy"] == strategy]
        best = min(entries, key=lambda item: item["mean_avg_ns"])
        best_by_strategy[strategy] = best

    plt.figure(figsize=(10, 5))
    labels = list(best_by_strategy.keys())
    values = [best_by_strategy[label]["mean_avg_ns"] for label in labels]
    plt.bar(labels, values, color="#38bdf8")
    plt.xticks(rotation=30, ha="right")
    plt.title("Best mean_avg_ns by strategy")
    plt.ylabel("mean_avg_ns")
    plt.tight_layout()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    plt.savefig(output_path, dpi=160)
    plt.close()


def main() -> int:
    if len(sys.argv) > 1:
        summary_csv = Path(sys.argv[1])
    else:
        summary_csv = Path("bench_summary.csv")
        if not summary_csv.exists():
            summary_csv = Path("tools/bench_harness/bench_summary.csv")
    if not summary_csv.exists():
        print(f"Summary CSV not found: {summary_csv}")
        return 1

    rows = load_rows(summary_csv)
    if not rows:
        print(f"No usable rows found in {summary_csv}")
        return 1

    plot_performance(rows, Path("docs/bench_performance.png"))
    plot_market_compare(rows, Path("docs/bench_market_compare.png"))
    print("Wrote docs/bench_performance.png")
    print("Wrote docs/bench_market_compare.png")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
