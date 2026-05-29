#!/usr/bin/env python3
"""Generate a performance envelope markdown from available bench artifacts.

Collects:
- tools/bench_results/throughput_from_ethtool.csv
- tools/bench_results/chaos_epyc_profile.csv
- tools/bench_results/routing_miss_sweep.csv
- tools/bench_results/crypto_overhead.txt

Writes: benchmark/PERFORMANCE_ENVELOPE.md
"""
import csv
from pathlib import Path
import sys


def read_single_value_csv(path: Path):
    if not path.exists():
        return None
    with path.open(encoding="utf-8") as fh:
        reader = csv.reader(fh)
        rows = list(reader)
        if len(rows) < 2:
            return None
        return rows[1:]


def produce_report(out_path: Path):
    out_path.parent.mkdir(parents=True, exist_ok=True)
    lines = []
    lines.append("# Performance Envelope Report\n")

    # Throughput from ethtool
    tpath = Path("tools/bench_results/throughput_from_ethtool.csv")
    trows = read_single_value_csv(tpath)
    if trows:
        lines.append("## Throughput (from ethtool)\n")
        lines.append("seconds | bytes_delta | gbps | tx_keys\n")
        lines.append("--- | --- | --- | ---\n")
        for r in trows:
            lines.append(" | ".join(r) + "\n")
    else:
        lines.append("**Throughput CSV not found:** tools/bench_results/throughput_from_ethtool.csv\n")

    # Chaos epcy profile
    cpath = Path("tools/bench_results/chaos_epyc_profile.csv")
    if cpath.exists():
        lines.append("\n## Chaos EPYC Profile\n")
        lines.append("(timestamp, core_set, packets, payload_len, loss, corrupt, duplicate, throughput_pkt_s, p50, p99, p99_9)\n\n")
        with cpath.open(encoding="utf-8") as fh:
            lines.extend([l for l in fh.readlines()])
    else:
        lines.append("\n**Chaos EPYC profile not found:** tools/bench_results/chaos_epyc_profile.csv\n")

    # Routing miss sweep
    rpath = Path("tools/bench_results/routing_miss_sweep.csv")
    if rpath.exists():
        lines.append("\n## Routing Miss Sweep\n")
        with rpath.open(encoding="utf-8") as fh:
            lines.extend([l for l in fh.readlines()])
    else:
        lines.append("\n**Routing miss sweep CSV not found:** tools/bench_results/routing_miss_sweep.csv\n")

    # Crypto overhead console
    ctxt = Path("tools/bench_results/crypto_overhead.txt")
    if ctxt.exists():
        lines.append("\n## Crypto Overhead Console Output\n")
        lines.append("```")
        with ctxt.open(encoding="utf-8") as fh:
            lines.extend([l for l in fh.readlines()])
        lines.append("``"+"\n")
    else:
        lines.append("\n**Crypto overhead console not found:** tools/bench_results/crypto_overhead.txt\n")

    out_path.write_text("".join(lines), encoding="utf-8")
    print(f"Wrote performance envelope to {out_path}")


if __name__ == '__main__':
    out = Path('benchmark/PERFORMANCE_ENVELOPE.md')
    produce_report(out)
