#!/usr/bin/env python3
import argparse
import csv
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate benchmark/chaos_report.md from chaos profile CSV")
    parser.add_argument("--input", default="tools/bench_results/chaos_epyc_profile.csv", help="Input CSV from chaos profile runner")
    parser.add_argument("--output", default="benchmark/chaos_report.md", help="Output markdown report")
    parser.add_argument("--baseline-throughput", type=float, default=None, help="Ideal-mode baseline throughput_pkt_s")
    parser.add_argument("--baseline-p99-ns", type=float, default=None, help="Ideal-mode baseline p99 latency in ns")
    parser.add_argument("--goal-max-throughput-drop-pct", type=float, default=5.0)
    parser.add_argument("--goal-max-p99-increase-ns", type=float, default=1000.0)
    return parser.parse_args()


def selected_row(rows: list[dict[str, str]]) -> dict[str, str]:
    if not rows:
        raise ValueError("no rows in input CSV")
    latest_ts = max(r.get("timestamp", "") for r in rows)
    latest_rows = [r for r in rows if r.get("timestamp", "") == latest_ts]
    return max(latest_rows, key=lambda r: float(r.get("p99_ns", "0") or 0.0))


def try_float(row: dict[str, str], key: str) -> float:
    value = row.get(key)
    if value is None or value == "":
        raise ValueError(f"missing column: {key}")
    return float(value)


def render_report(
    row: dict[str, str],
    baseline_throughput: float | None,
    baseline_p99_ns: float | None,
    goal_drop_pct: float,
    goal_p99_delta_ns: float,
) -> str:
    throughput = try_float(row, "throughput_pkt_s")
    p50 = try_float(row, "p50_ns")
    p99 = try_float(row, "p99_ns")
    p999 = try_float(row, "p99_9_ns")

    drop_eval = "N/A"
    p99_eval = "N/A"
    overall = "BLOCKED (missing ideal-mode baseline metrics)"

    if baseline_throughput is not None and baseline_throughput > 0:
        throughput_drop_pct = ((baseline_throughput - throughput) / baseline_throughput) * 100.0
        drop_eval = f"{throughput_drop_pct:.2f}% (goal < {goal_drop_pct:.2f}%)"
    else:
        throughput_drop_pct = None

    if baseline_p99_ns is not None and baseline_p99_ns > 0:
        p99_delta_ns = p99 - baseline_p99_ns
        p99_eval = f"{p99_delta_ns:.2f} ns (goal < {goal_p99_delta_ns:.2f} ns)"
    else:
        p99_delta_ns = None

    if throughput_drop_pct is not None and p99_delta_ns is not None:
        passed = throughput_drop_pct < goal_drop_pct and p99_delta_ns < goal_p99_delta_ns
        overall = "PASS" if passed else "FAIL"

    return f"""# Chaos Engineering Report

Status: **{overall}**

## Objective

Validate safety-invariant resilience under hostile traffic while keeping performance overhead bounded.

- Throughput degradation target: < {goal_drop_pct:.2f}% vs ideal mode
- p99 increase target: < {goal_p99_delta_ns:.2f} ns vs ideal mode

## Input Artifact

- Source CSV: `tools/bench_results/chaos_epyc_profile.csv`
- Latest sampled row timestamp: `{row.get('timestamp', 'unknown')}`
- Core set: `{row.get('core_set', 'unknown')}`

## Latest Chaos Metrics

- throughput_pkt_s: `{throughput:.2f}`
- latency_ns p50: `{p50:.0f}`
- latency_ns p99: `{p99:.0f}`
- latency_ns p99_9: `{p999:.0f}`

## Baseline Comparison

- Baseline throughput_pkt_s: `{baseline_throughput if baseline_throughput is not None else 'NOT PROVIDED'}`
- Baseline p99_ns: `{baseline_p99_ns if baseline_p99_ns is not None else 'NOT PROVIDED'}`
- Throughput degradation: `{drop_eval}`
- p99 increase: `{p99_eval}`

## Invariant Notes

- Byzantine fault injection includes packet drop, corruption/truncation, and duplication.
- Report must be re-generated for each release candidate on target hardware.
- If forwarding interacts with Go control-plane in fast path, mark `DEPLOYMENT.manifest.md` as **AT RISK**.

## Reproduction

```bash
make chaos-epyc-profile
python3 tools/benchmark/generate_chaos_report.py \
  --input tools/bench_results/chaos_epyc_profile.csv \
  --output benchmark/chaos_report.md
```
"""


def main() -> int:
    args = parse_args()
    input_path = Path(args.input)
    output_path = Path(args.output)

    if not input_path.exists():
        raise SystemExit(f"missing input CSV: {input_path}")

    with input_path.open("r", encoding="utf-8", newline="") as f:
        rows = list(csv.DictReader(f))

    row = selected_row(rows)
    text = render_report(
        row=row,
        baseline_throughput=args.baseline_throughput,
        baseline_p99_ns=args.baseline_p99_ns,
        goal_drop_pct=args.goal_max_throughput_drop_pct,
        goal_p99_delta_ns=args.goal_max_p99_increase_ns,
    )

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(text, encoding="utf-8")
    print(f"wrote {output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
