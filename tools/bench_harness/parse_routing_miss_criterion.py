#!/usr/bin/env python3
"""Parse Criterion routing miss-path output into CSV.

Usage: parse_routing_miss_criterion.py <criterion_output.txt> <out.csv>
"""
import csv
import re
import sys


if len(sys.argv) < 3:
    print("Usage: parse_routing_miss_criterion.py <criterion_output.txt> <out.csv>")
    sys.exit(2)

input_path = sys.argv[1]
output_path = sys.argv[2]

bench_line = re.compile(r"^routing_miss_path/lookup_or_predict_miss/(?P<routes>\d+)$")
time_line = re.compile(r"time:\s+\[(?P<low>[0-9.]+)\s+µs\s+(?P<mid>[0-9.]+)\s+µs\s+(?P<high>[0-9.]+)\s+µs\]\s*$")
thrpt_line = re.compile(r"thrpt:\s+\[(?P<low>[0-9.]+)\s+Melem/s\s+(?P<mid>[0-9.]+)\s+Melem/s\s+(?P<high>[0-9.]+)\s+Melem/s\]")

rows = []
current_routes = None

with open(input_path, encoding="utf-8") as handle:
    for raw_line in handle:
        line = raw_line.strip()
        bench_match = bench_line.match(line)
        if bench_match:
            current_routes = int(bench_match.group("routes"))
            continue

        if current_routes is None:
            continue

        time_match = time_line.search(line)
        if time_match:
            rows.append({
                "route_count": current_routes,
                "mean_time_ns": time_match.group("mid"),
            })
            continue

        thrpt_match = thrpt_line.search(line)
        if thrpt_match and rows and rows[-1]["route_count"] == current_routes:
            rows[-1]["throughput_melem_s"] = thrpt_match.group("mid")

with open(output_path, "w", newline="", encoding="utf-8") as handle:
    writer = csv.writer(handle)
    writer.writerow(["route_count", "mean_time_ns", "throughput_melem_s"])
    for row in rows:
        writer.writerow([
            row["route_count"],
            row["mean_time_ns"],
            row.get("throughput_melem_s", ""),
        ])

print(f"Wrote {len(rows)} rows to {output_path}")