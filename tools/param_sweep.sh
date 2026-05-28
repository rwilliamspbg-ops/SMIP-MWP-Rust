#!/usr/bin/env bash
set -euo pipefail

# Param sweep driver for routing fast-path tuning.
# This script patches `routing/src/lib.rs` constants temporarily, runs the
# `routing_miss_bench` bench for each parameter combination, and saves outputs
# to `tools/bench_results/param_sweep/`.

ORIG_FILE="routing/src/lib.rs"
BACKUP_FILE="/tmp/lib_rs.routing.orig"
OUTDIR="tools/bench_results/param_sweep"
SAMPLE_SIZE=200
REPEATS=3

FAST_SHARDS_LIST=(8 16 32)
HOT_CACHE_SIZE_LIST=(8 16 32)
HOT_CACHE_PROBE_LIST=(2 4)

mkdir -p "$OUTDIR"

if [[ ! -f "$ORIG_FILE" ]]; then
  echo "Cannot find $ORIG_FILE in repo root. Run from workspace root." >&2
  exit 1
fi

# backup original
cp "$ORIG_FILE" "$BACKUP_FILE"

cleanup() {
  echo "Restoring original $ORIG_FILE"
  cp "$BACKUP_FILE" "$ORIG_FILE"
}
trap cleanup EXIT

for shards in "${FAST_SHARDS_LIST[@]}"; do
  for cache in "${HOT_CACHE_SIZE_LIST[@]}"; do
    for probe in "${HOT_CACHE_PROBE_LIST[@]}"; do
      for run in $(seq 1 $REPEATS); do
        ts=$(date +%Y%m%d_%H%M%S)
        label="fs${shards}_cs${cache}_p${probe}_run${run}_$ts"
        outfile="$OUTDIR/routing_miss_sweep_${label}.txt"

        echo "[sweep] shards=$shards cache=$cache probe=$probe run=$run -> $outfile"

        # Replace constants in place using perl slurp
        perl -0777 -pe \
          "s/const\s+FAST_SHARDS\s*:\s*usize\s*=\s*\d+\s*;/const FAST_SHARDS: usize = ${shards};/s; s/const\s+HOT_CACHE_SIZE\s*:\s*usize\s*=\s*\d+\s*;/const HOT_CACHE_SIZE: usize = ${cache};/s; s/const\s+HOT_CACHE_PROBE\s*:\s*usize\s*=\s*\d+\s*;/const HOT_CACHE_PROBE: usize = ${probe};/s" \
          "$BACKUP_FILE" > "$ORIG_FILE"

        # Run bench (non-pinned by default; pin externally if desired)
        cargo bench --bench routing_miss_bench --release -- --sample-size $SAMPLE_SIZE --nocapture | tee "$outfile" || true

        sleep 1
      done
    done
  done
done

echo "Done. Results in $OUTDIR; original file restored."
