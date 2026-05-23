#!/usr/bin/env python3
import sys
import re
import csv
from datetime import datetime

if len(sys.argv) < 6:
    print("Usage: parse_and_append.py <bench_output.txt> <out.csv> <run_index> <strategy> <commit>")
    sys.exit(2)

bench_file = sys.argv[1]
out_csv = sys.argv[2]
run_index = sys.argv[3]
strategy = sys.argv[4]
commit = sys.argv[5]

pattern = re.compile(r"strategy=(?P<strategy>\w+) size=(?P<size>\d+) avg_ns=(?P<avg>[0-9.]+) throughput_mib_s=(?P<tps>[0-9.]+)")

with open(bench_file) as f:
    lines = f.readlines()

rows = []
for line in lines:
    m = pattern.search(line)
    if m:
        rows.append({
            'timestamp': datetime.utcnow().isoformat(),
            'commit': commit,
            'run_index': run_index,
            'strategy': m.group('strategy'),
            'size': m.group('size'),
            'avg_ns': m.group('avg'),
            'throughput_mib_s': m.group('tps')
        })

# append to CSV
with open(out_csv, 'a', newline='') as csvfile:
    writer = csv.writer(csvfile)
    for r in rows:
        writer.writerow([r['timestamp'], r['commit'], r['run_index'], r['strategy'], r['size'], r['avg_ns'], r['throughput_mib_s']])

print(f"Appended {len(rows)} rows from {bench_file} to {out_csv}")
