#!/usr/bin/env bash
set -euo pipefail

# Usage: run_routing_miss_sweep.sh [OUTPUT_CSV] [SAMPLE_SIZE]
# OUTPUT_CSV: path to write CSV (default: ./routing_miss_sweep.csv)
# SAMPLE_SIZE: Criterion sample size (default: 10)

OUT=${1:-routing_miss_sweep.csv}
SAMPLE_SIZE=${2:-10}

if [[ "$SAMPLE_SIZE" -lt 10 ]]; then
  echo "Criterion requires sample size >= 10; using 10 instead of $SAMPLE_SIZE" >&2
  SAMPLE_SIZE=10
fi

mkdir -p /tmp/bench_harness

if command -v cargo >/dev/null 2>&1; then
  cargo bench -p bench --bench routing_miss_bench -- --noplot --sample-size "$SAMPLE_SIZE" > /tmp/bench_harness/routing_miss_bench.txt
else
  echo "cargo not found in PATH" >&2
  exit 1
fi

python3 tools/bench_harness/parse_routing_miss_criterion.py /tmp/bench_harness/routing_miss_bench.txt "$OUT"

echo "Finished. CSV saved to $OUT"