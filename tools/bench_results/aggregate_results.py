#!/usr/bin/env python3
"""Aggregate bench outputs into CSV and a short markdown report.

Writes: tools/bench_results/bench_summary.csv and tools/bench_results/bench_report.md
"""
import re
import json
from pathlib import Path

OUT_CSV = Path("tools/bench_results/bench_summary.csv")
OUT_MD = Path("tools/bench_results/bench_report.md")
ROOT = Path("tools/bench_results")

def parse_mcr(path):
    text = path.read_text()
    res = {}
    m = re.search(r'throughput_pkt_s=([0-9.]+)', text)
    if m:
        res['throughput_pkt_s'] = float(m.group(1))
    m = re.search(r'latency_ns\s+p50=([0-9]+)\s+p99=([0-9]+)\s+p99_9=([0-9]+)', text)
    if m:
        res['p50_ns'] = int(m.group(1))
        res['p99_ns'] = int(m.group(2))
        res['p99_9_ns'] = int(m.group(3))
    return res

def parse_bench_generic(path):
    text = path.read_text()
    # try to find 'thrpt:' blocks like 'thrpt:  [15.546 GiB/s 15.978 GiB/s 16.497 GiB/s]'
    m = re.search(r'thrpt:\s*\[([^\]]+)\]', text)
    if m:
        parts = m.group(1).split()
        # pick middle numeric token (commonly the median)
        nums = [p for p in parts if re.search(r'[0-9]', p)]
        if nums:
            # strip non-numeric suffixes
            n = nums[len(nums)//2]
            n = re.sub(r"[^0-9.]+$", "", n)
            try:
                return { 'throughput': float(n), 'unit': parts[-1] if parts else '' }
            except:
                pass
    # try to parse 'time:' bracketed values like 'time:   [57.809 ns 59.685 ns 61.345 ns]'
    m = re.search(r'time:\s*\[([^\]]+)\]', text)
    if m:
        parts = m.group(1).split()
        # parts commonly like ['57.809', 'ns', '59.685', 'ns', '61.345', 'ns']
        nums_units = []
        i = 0
        while i < len(parts):
            token = parts[i]
            if re.search(r'[0-9]', token):
                # look ahead for unit
                unit = ''
                if i+1 < len(parts) and re.search(r'[a-zA-Zµ/]', parts[i+1]):
                    unit = parts[i+1]
                    i += 1
                nums_units.append((token, unit))
            i += 1
        if nums_units:
            mid = nums_units[len(nums_units)//2]
            val_str, unit = mid
            val = float(re.sub(r"[^0-9.]+$", "", val_str))
            # normalize to ns if needed
            unit = unit or ''
            unit = unit.replace('us', 'µs')
            if 'µs' in unit or 'us' in unit:
                val_ns = val * 1000.0
            elif 'ms' in unit:
                val_ns = val * 1_000_000.0
            else:
                # assume ns
                val_ns = val
            return { 'time_ns': float(val_ns) }
    # fallback: no metrics found
    return {}

def read_crypto_baselines():
    p = ROOT / 'crypto_baselines.json'
    if not p.exists():
        return {}
    # file may contain duplicate JSON blobs; parse the first object
    txt = p.read_text()
    try:
        objs = [json.loads(s) for s in re.findall(r'\{[^}]*\}', txt)]
        if objs:
            return objs[0]
    except Exception:
        pass
    try:
        return json.loads(txt)
    except Exception:
        return {}

def main():
    rows = []
    # MCR smoke files
    for name in ['mcr_smoke.txt','mcr_larger_smoke.txt']:
        p = ROOT / name
        if p.exists():
            r = parse_mcr(p)
            for k,v in r.items():
                rows.append((name,k,str(v)))

    # Generic benches
    bench_files = ['alloc_bench.txt','crypto_overhead_bench.txt','datapath_bench.txt','line_rate_bench.txt','packet_copy_bench.txt','poll_slices_bench.txt','routing_miss_bench.txt']
    for name in bench_files:
        p = ROOT / name
        if p.exists():
            r = parse_bench_generic(p)
            for k,v in r.items():
                rows.append((name,k,str(v)))

    OUT_CSV.parent.mkdir(parents=True, exist_ok=True)
    with OUT_CSV.open('w') as f:
        f.write('source,metric,value\n')
        for s,k,v in rows:
            f.write(f'{s},{k},{v}\n')

    # Generate markdown report
    lines = []
    lines.append('# Bench Summary')
    lines.append('')
    if rows:
        lines.append('| Source | Metric | Value |')
        lines.append('|---|---:|---:|')
        for s,k,v in rows:
            lines.append(f'| {s} | {k} | {v} |')
    else:
        lines.append('No bench outputs found.')

    # Baseline comparisons: MCR
    baseline_file = ROOT / 'ci_baseline_mcr.txt'
    if baseline_file.exists():
        try:
            baseline = float(baseline_file.read_text().strip())
            # pick larger smoke result if present else small
            res_file = ROOT / 'mcr_larger_smoke_result.txt'
            if not res_file.exists():
                res_file = ROOT / 'mcr_smoke_result.txt'
            if res_file.exists():
                val = float(res_file.read_text().strip().split('=')[-1])
                min_allowed = baseline * 0.8
                status = 'PASS' if val >= min_allowed else 'FAIL'
                lines.append('')
                lines.append('## MCR baseline check')
                lines.append(f'- baseline: {baseline}')
                lines.append(f'- measured: {val}')
                lines.append(f'- min allowed (80%): {min_allowed}')
                lines.append(f'- status: **{status}**')
        except Exception as e:
            lines.append(f'Could not parse baseline file: {e}')

    # Crypto baselines
    crypto_baselines = read_crypto_baselines()
    if crypto_baselines:
        # try to extract current medians from crypto_overhead_bench.txt
        p = ROOT / 'crypto_overhead_bench.txt'
        cur = {}
        if p.exists():
            txt = p.read_text()
            for key in crypto_baselines.keys():
                m = re.search(rf'{re.escape(key)}.*?\n\s*time:\s*\[([^\]]+)\]', txt, re.S)
                if m:
                    parts = m.group(1).split()
                    nums_units = []
                    i = 0
                    while i < len(parts):
                        token = parts[i]
                        if re.search(r'[0-9]', token):
                            unit = ''
                            if i+1 < len(parts) and re.search(r'[a-zA-Zµ/]', parts[i+1]):
                                unit = parts[i+1]
                                i += 1
                            nums_units.append((token, unit))
                        i += 1
                    if nums_units:
                        mid = nums_units[len(nums_units)//2]
                        val_str, unit = mid
                        val = float(re.sub(r"[^0-9.]+$", "", val_str))
                        unit = unit or ''
                        unit = unit.replace('us', 'µs')
                        if 'µs' in unit or 'us' in unit:
                            val_ns = val * 1000.0
                        elif 'ms' in unit:
                            val_ns = val * 1_000_000.0
                        else:
                            val_ns = val
                        try:
                            cur[key] = float(val_ns)
                        except:
                            pass
        if cur:
            lines.append('')
            lines.append('## Crypto baseline comparison')
            lines.append('| metric | baseline | current | delta % | status |')
            lines.append('|---|---:|---:|---:|---:|')
            for k,base in crypto_baselines.items():
                curv = cur.get(k)
                if curv is None:
                    lines.append(f'| {k} | {base} | - | - | MISSING |')
                    continue
                pct = (curv - base) / base * 100.0 if base != 0 else float('inf')
                status = 'OK' if abs(pct) < 15.0 else 'REGRESSED'
                lines.append(f'| {k} | {base:.6f} | {curv:.6f} | {pct:.2f}% | {status} |')

    OUT_MD.parent.mkdir(parents=True, exist_ok=True)
    OUT_MD.write_text('\n'.join(lines))
    print(f'Wrote summary CSV: {OUT_CSV}\nWrote report: {OUT_MD}')

if __name__ == '__main__':
    main()
