#!/usr/bin/env python3
"""Generate MCR spraying performance report."""

import argparse
import csv
from collections import defaultdict


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True)
    parser.add_argument("--output", default="benchmark/mcr_chaos_report.md")
    return parser.parse_args()


def main():
    args = parse_args()

    with open(args.input, 'r') as f:
        reader = csv.DictReader(f)
        rows = list(reader)

    # Group by channel count
    stats_by_channels = defaultdict(list)
    for row in rows:
        key = f"{row.get('mcr_channels', '1')}ch"
        stats_by_channels[key].append(row)

    # Generate markdown report
    with open(args.output, 'w') as f:
        f.write("# MCR Spraying Chaos Report\n\n")

        for channels in sorted(stats_by_channels.keys()):
            subset = stats_by_channels[channels]
            avg_throughput = sum(float(r.get('throughput_pkt_s', 0) or 0) for r in subset) / max(len(subset), 1)
            p99_values = [float(r.get('p99_ns', 0) or 0) for r in subset if r.get('p99_ns')]

            f.write(f"## {channels} Channel Spraying\n\n")
            f.write(f"- **Median Throughput**: {avg_throughput:.2f} pkt/s\n")
            f.write(f"- **P99 Latency**: {min(p99_values) if p99_values else 'N/A'} ns\n")
            drops = [float(r.get('drop', 0) or 0) for r in subset]
            drop_rate = (sum(drops) / len(drops) * 100) if drops else 0
            f.write(f"- **Drop Rate**: {drop_rate:.2f}%\n\n")


if __name__ == "__main__":
    main()
