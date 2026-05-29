#!/usr/bin/env python3
"""Compute throughput (Gbps) from ethtool -S snapshots.

Usage:
  compute_throughput_from_ethtool.py before.txt after.txt time_before.txt time_after.txt [out.csv]

The script looks for TX byte counters (heuristic) and sums deltas.
"""
import csv
import re
import sys
from pathlib import Path


def parse_ethtool_stats(path: Path):
    stats = {}
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line or ':' not in line:
                continue
            k, v = line.split(':', 1)
            k = k.strip()
            v = v.strip().split()[0]
            try:
                stats[k] = int(v)
            except ValueError:
                try:
                    stats[k] = int(float(v))
                except Exception:
                    pass
    return stats


def choose_tx_keys(stats):
    # Heuristic: keys containing 'tx' and 'byte' or exact 'tx_bytes' or 'tx_bytes_<n>'
    candidates = [k for k in stats.keys() if re.search(r'tx.*byte|tx_bytes', k, re.I)]
    if candidates:
        return candidates

    # Fallback: keys starting with 'tx_' and containing 'bytes' or 'packets'
    candidates = [k for k in stats.keys() if k.lower().startswith('tx_')]
    return candidates


def main():
    if len(sys.argv) < 5:
        print("Usage: compute_throughput_from_ethtool.py before.txt after.txt time_before.txt time_after.txt [out.csv]")
        sys.exit(2)

    before_path = Path(sys.argv[1])
    after_path = Path(sys.argv[2])
    time_before_path = Path(sys.argv[3])
    time_after_path = Path(sys.argv[4])
    out_path = Path(sys.argv[5]) if len(sys.argv) > 5 else Path("tools/bench_results/throughput_from_ethtool.csv")

    before = parse_ethtool_stats(before_path)
    after = parse_ethtool_stats(after_path)

    if not before or not after:
        print("Failed to parse ethtool stats")
        sys.exit(1)

    tx_keys = choose_tx_keys(before)
    tx_keys = [k for k in tx_keys if k in after]
    if not tx_keys:
        print("No tx byte keys detected in ethtool output")
        sys.exit(1)

    bytes_before = sum(before.get(k, 0) for k in tx_keys)
    bytes_after = sum(after.get(k, 0) for k in tx_keys)
    bytes_delta = bytes_after - bytes_before

    # Read times
    try:
        t_before = float(time_before_path.read_text().strip())
        t_after = float(time_after_path.read_text().strip())
    except Exception as e:
        print(f"Failed to read time files: {e}")
        sys.exit(1)

    seconds = t_after - t_before if t_after > t_before else 1.0
    gbps = (bytes_delta * 8) / (seconds * 1e9)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    header = ["seconds", "bytes_delta", "gbps", "tx_keys"]
    write_header = not out_path.exists()
    with out_path.open("a", newline="", encoding="utf-8") as fh:
        writer = csv.writer(fh)
        if write_header:
            writer.writerow(header)
        writer.writerow([seconds, bytes_delta, f"{gbps:.6f}", ";".join(tx_keys)])

    print(f"Wrote throughput: {gbps:.6f} Gbps to {out_path}")


if __name__ == '__main__':
    main()
