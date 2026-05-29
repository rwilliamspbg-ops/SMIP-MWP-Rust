#!/usr/bin/env bash
set -euo pipefail

# Usage: run_bench_harness.sh [ITERATIONS] [OUTPUT_CSV]
# ITERATIONS: how many times to run each strategy (default 20)
# OUTPUT_CSV: path to write CSV (default: ./bench_results.csv)

ITER=${1:-20}
OUT=${2:-bench_results.csv}
STRATS=(scalar tiled_64 tiled_128 tiled_256 tiled_512 tiled_256_padded tiled_avx2_256)

mkdir -p /tmp/bench_harness

if command -v cargo >/dev/null 2>&1; then
  cargo build -p bench --release
else
  echo "cargo not found in PATH — assuming ./target/release/bench already exists"
fi

if [ ! -f "$OUT" ]; then
  echo "timestamp,commit,run_index,strategy,size,avg_ns,throughput_mib_s" > "$OUT"
fi

COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")

for ((i=1;i<=ITER;i++)); do
  for s in "${STRATS[@]}"; do
    echo "Run $i strategy $s"
    BENCH_STRATEGY=$s ./target/release/bench > "/tmp/bench_harness/output_${s}.txt" 2>&1 || true
    python3 tools/bench_harness/parse_and_append.py \
      "/tmp/bench_harness/output_${s}.txt" \
      "$OUT" \
      "$i" \
      "$s" \
      "$COMMIT"
  done
done

echo "Finished. CSV saved to $OUT"
