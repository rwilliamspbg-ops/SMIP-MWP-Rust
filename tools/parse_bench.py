#!/usr/bin/env python3
import re
import csv
from pathlib import Path

PAT = re.compile(r"routing_miss_path/lookup_or_predict_miss/(\d+)")
THRPT_RE = re.compile(r"thrpt:\s+\[(?P<vals>[^\]]+)\]")
CHANGE_MARKERS = [
    ('improved', 'Performance has improved.'),
    ('regressed', 'Performance has regressed.'),
    ('no_change', 'No change in performance detected.'),
    ('within_noise', 'Change within noise threshold.'),
]

out_dir = Path('tools/bench_results')
files = sorted(out_dir.glob('routing_miss_sweep_*.txt'))

rows = []
for f in files:
    variant = f.stem.replace('routing_miss_sweep_','')
    text = f.read_text()
    # find each benchmark entry and extract size, thrpt, and change marker
    matches = list(PAT.finditer(text))
    for i, m in enumerate(matches):
        size = m.group(1)
        start = m.start()
        end = matches[i+1].start() if i+1 < len(matches) else len(text)
        block = text[start:end]
        thr = THRPT_RE.search(block)
        thrpt = thr.group('vals').strip() if thr else ''
        change = ''
        for key, marker in CHANGE_MARKERS:
            if marker in block:
                change = key
                break
        rows.append({'variant': variant, 'size': size, 'thrpt': thrpt, 'change': change})

out_csv = out_dir / 'routing_miss_summary.csv'
with out_csv.open('w', newline='') as csvfile:
    w = csv.DictWriter(csvfile, fieldnames=['variant','size','thrpt','change'])
    w.writeheader()
    for r in rows:
        w.writerow(r)

print(f'Wrote {out_csv} with {len(rows)} rows')
