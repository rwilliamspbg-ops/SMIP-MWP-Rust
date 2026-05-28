#!/usr/bin/env python3
import csv
from pathlib import Path
import re
import matplotlib.pyplot as plt

csvp = Path('tools/bench_results/routing_miss_summary.csv')
if not csvp.exists():
    print('CSV not found:', csvp)
    raise SystemExit(1)

rows = {}
with csvp.open() as f:
    r = csv.DictReader(f)
    for rec in r:
        var = rec['variant']
        size = int(rec['size'])
        thrpt = rec['thrpt']
        # extract middle throughput value (three numbers) and parse numeric value
        # e.g. '14.508 Melem/s 14.691 Melem/s 14.856 Melem/s'
        nums = re.findall(r"([0-9.]+)\s*Melem/s", thrpt)
        if not nums:
            val = 0.0
        else:
            # choose median value if 3
            if len(nums) >= 3:
                val = float(nums[1])
            else:
                val = float(nums[-1])
        rows.setdefault(var, [])
        rows[var].append((size, val))

outdir = Path('tools/bench_results/plots')
outdir.mkdir(parents=True, exist_ok=True)

# Sort sizes
for var, data in rows.items():
    data.sort(key=lambda x: x[0])

# Create a single plot with all variants
plt.figure(figsize=(8,5))
for var, data in sorted(rows.items()):
    sizes = [s for s,_ in data]
    vals = [v for _,v in data]
    plt.plot(sizes, vals, marker='o', label=var)

plt.xlabel('routing miss size')
plt.ylabel('Throughput (Melem/s, median)')
plt.title('Routing miss throughput by variant')
plt.grid(True)
plt.legend()
plt.xticks(sorted({s for data in rows.values() for s,_ in data}))
plt.tight_layout()
png = outdir / 'routing_miss_throughput.png'
plt.savefig(png)
print('Wrote', png)

# Also save per-variant small plots
for var, data in rows.items():
    plt.figure(figsize=(6,3))
    data.sort()
    sizes = [s for s,_ in data]
    vals = [v for _,v in data]
    plt.plot(sizes, vals, marker='o')
    plt.xlabel('routing miss size')
    plt.ylabel('Throughput (Melem/s)')
    plt.title(var)
    plt.grid(True)
    plt.tight_layout()
    p = outdir / f'{var}.png'
    plt.savefig(p)
    print('Wrote', p)
