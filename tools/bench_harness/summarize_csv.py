#!/usr/bin/env python3
"""Summarize bench CSV into per-strategy+size mean/std CSV.

Usage: summarize_csv.py input.csv output_summary.csv
"""
import sys
import csv
from collections import defaultdict
import math

if len(sys.argv) < 3:
    print("Usage: summarize_csv.py input.csv output_summary.csv")
    sys.exit(2)

infile = sys.argv[1]
outfile = sys.argv[2]

data = defaultdict(list)
with open(infile, newline='') as f:
    reader = csv.reader(f)
    header = next(reader, None)
    for row in reader:
        # timestamp,commit,run_index,strategy,size,avg_ns,throughput_mib_s
        try:
            strategy = row[3]
            size = int(row[4])
            avg_ns = float(row[5])
            data[(strategy, size)].append(avg_ns)
        except Exception:
            continue

with open(outfile, 'w', newline='') as f:
    writer = csv.writer(f)
    writer.writerow(['strategy', 'size', 'runs', 'mean_avg_ns', 'stddev_avg_ns'])
    for (strategy, size), samples in sorted(data.items()):
        n = len(samples)
        mean = sum(samples) / n
        var = sum((x - mean) ** 2 for x in samples) / n
        std = math.sqrt(var)
        writer.writerow([strategy, size, n, f"{mean:.2f}", f"{std:.2f}"])

print(f"Wrote summary to {outfile}")
