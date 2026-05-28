#!/usr/bin/env python3
import csv
from pathlib import Path
import re
import math

csvp = Path('tools/bench_results/routing_miss_summary.csv')
if not csvp.exists():
    print('CSV not found:', csvp)
    raise SystemExit(1)

rows = []
with csvp.open() as f:
    r = csv.DictReader(f)
    for rec in r:
        variant = rec['variant']
        # strip trailing timestamp pattern _YYYYMMDD_HHMMSS
        base = re.sub(r'_[0-9]{8}_[0-9]{6}$', '', variant)
        size = int(rec['size'])
        thrpt = rec['thrpt']
        nums = re.findall(r'([0-9]+\.?[0-9]*)\s*Melem/s', thrpt)
        if not nums:
            continue
        # take median value (middle of triple) when present
        if len(nums) >= 3:
            val = float(nums[1])
        else:
            val = float(nums[-1])
        rows.append((base, size, val))

# group
from collections import defaultdict
groups = defaultdict(list)
for base, size, val in rows:
    groups[(base, size)].append(val)

out = []
for (base, size), vals in sorted(groups.items()):
    n = len(vals)
    mean = sum(vals)/n
    # sample stddev
    if n > 1:
        var = sum((x-mean)**2 for x in vals)/(n-1)
        sd = math.sqrt(var)
    else:
        sd = 0.0
    se = sd/math.sqrt(n) if n>0 else 0.0
    ci95 = 1.96*se
    out.append({'variant': base, 'size': size, 'n': n, 'mean_Melem_s': mean, 'sd': sd, 'se': se, 'ci95': ci95})

outp = Path('tools/bench_results/routing_miss_stats.csv')
with outp.open('w', newline='') as f:
    w = csv.DictWriter(f, fieldnames=['variant','size','n','mean_Melem_s','sd','se','ci95'])
    w.writeheader()
    for r in out:
        w.writerow(r)

print('Wrote', outp)

# Print a concise summary per variant
by_variant = defaultdict(list)
for r in out:
    by_variant[r['variant']].append(r)

for variant, entries in by_variant.items():
    print('\nVariant:', variant)
    for e in sorted(entries, key=lambda x: x['size']):
        print(f" size={e['size']:2} n={e['n']:2} mean={e['mean_Melem_s']:.3f} sd={e['sd']:.3f} ci95=±{e['ci95']:.3f}")
