#!/usr/bin/env python3
import json
from pathlib import Path
import sys
import os

THRESHOLD = float(os.environ.get("CRYPTO_PCT_THRESHOLD", "15"))
UPDATE = os.environ.get("UPDATE_CRYPTO_BASELINES", "0") in ("1", "true", "yes")

ROOT = Path("target/criterion/crypto_overhead")
BASELINE_FILE = Path("tools/bench_results/crypto_baselines.json")

BENCHES = [
    "symmetric_encrypt_in_place",
    "symmetric_decrypt_in_place",
    "worst_case_hybrid_kex",
]


def read_median(bench_name: str):
    p = ROOT / bench_name / "new" / "estimates.json"
    if not p.exists():
        raise FileNotFoundError(f"estimates.json not found for {bench_name}: {p}")
    with p.open() as f:
        data = json.load(f)
    # expect median.point_estimate
    med = data.get("median", {}).get("point_estimate")
    if med is None:
        raise KeyError(f"median.point_estimate missing in {p}")
    return float(med)


def load_baselines():
    if BASELINE_FILE.exists():
        with BASELINE_FILE.open() as f:
            return json.load(f)
    return {}


def write_baselines(b):
    BASELINE_FILE.parent.mkdir(parents=True, exist_ok=True)
    with BASELINE_FILE.open("w") as f:
        json.dump(b, f, indent=2)


def main():
    current = {}
    for b in BENCHES:
        try:
            current[b] = read_median(b)
        except Exception as e:
            print(f"ERROR reading {b}: {e}")
            sys.exit(3)

    baselines = load_baselines()
    if UPDATE or not baselines:
        write_baselines(current)
        print(f"Wrote baselines to {BASELINE_FILE}:")
        print(json.dumps(current, indent=2))
        return 0

    failed = False
    for b, cur in current.items():
        base = baselines.get(b)
        if base is None:
            print(f"No baseline for {b}; consider running with UPDATE_CRYPTO_BASELINES=1")
            failed = True
            continue
        pct = (cur - base) / base * 100.0 if base != 0 else float('inf')
        ok = abs(pct) < THRESHOLD
        status = "OK" if ok else "REGRESSED"
        print(f"{b}: baseline={base:.6f} ns current={cur:.6f} ns delta={pct:.2f}% => {status}")
        if not ok:
            failed = True

    if failed:
        print("One or more benchmarks exceeded threshold")
        return 2
    print("All crypto benchmarks within threshold")
    return 0


if __name__ == '__main__':
    sys.exit(main())
