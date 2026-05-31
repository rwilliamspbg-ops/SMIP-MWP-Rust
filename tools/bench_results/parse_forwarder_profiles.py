#!/usr/bin/env python3
"""Parse forwarder profile sections from mcr smoke outputs and summarize hotspots."""
from pathlib import Path
import re
ROOT = Path("tools/bench_results")

def parse_profile(text):
    # look for lines like: 'handle: 10000 calls, 5597595 ns total, avg 559 ns'
    pattern = re.compile(r'^(?P<fn>\w+):\s+(?P<calls>[0-9,]+) calls,\s+(?P<total>[0-9,]+) ns total, avg\s+(?P<avg>[0-9,]+) ns', re.M)
    res = []
    for m in pattern.finditer(text):
        fn = m.group('fn')
        calls = int(m.group('calls').replace(',',''))
        total = int(m.group('total').replace(',',''))
        avg = int(m.group('avg').replace(',',''))
        res.append({'fn':fn,'calls':calls,'total_ns':total,'avg_ns':avg})
    return res

def summarize_file(p):
    txt = p.read_text()
    # find forwarder profile block
    if '--- Forwarder profile ---' in txt:
        after = txt.split('--- Forwarder profile ---',1)[1]
    else:
        after = txt
    entries = parse_profile(after)
    if not entries:
        return None
    # sort by total_ns desc
    entries.sort(key=lambda e: e['total_ns'], reverse=True)
    return entries

def main():
    out = []
    for name in ['mcr_smoke.txt','mcr_larger_smoke.txt']:
        p = ROOT / name
        if p.exists():
            entries = summarize_file(p)
            if entries:
                out.append((name, entries))

    # write report
    report = ROOT / 'forwarder_hotspots.md'
    lines = ['# Forwarder Hotspots']
    if not out:
        lines.append('No forwarder profiles found in smoke outputs.')
    for name, entries in out:
        lines.append('')
        lines.append(f'## {name}')
        lines.append('| function | calls | total_ns | avg_ns |')
        lines.append('|---|---:|---:|---:|')
        for e in entries:
            lines.append(f"| {e['fn']} | {e['calls']} | {e['total_ns']} | {e['avg_ns']} |")
    report.write_text('\n'.join(lines))
    print('Wrote', report)

if __name__ == '__main__':
    main()
