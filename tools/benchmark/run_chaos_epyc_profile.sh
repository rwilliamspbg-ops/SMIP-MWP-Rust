#!/usr/bin/env bash
set -euo pipefail

# EPYC-oriented chaos benchmark matrix runner.
# Produces a CSV with throughput and latency percentiles across core sets.

OUT=${1:-tools/bench_results/chaos_epyc_profile.csv}
PACKETS=${PACKETS:-50000}
PAYLOAD_LEN=${PAYLOAD_LEN:-1024}
LOSS_PERCENT=${LOSS_PERCENT:-3}
CORRUPT_PERCENT=${CORRUPT_PERCENT:-1}
DUPLICATE_PERCENT=${DUPLICATE_PERCENT:-1}
SEED_BASE=${SEED_BASE:-20260528}
CORE_SETS=${CORE_SETS:-2-3 2-5 2-7}

mkdir -p "$(dirname "$OUT")"

echo "timestamp,core_set,packets,payload_len,loss_percent,corrupt_percent,duplicate_percent,throughput_pkt_s,p50_ns,p99_ns,p99_9_ns" > "$OUT"

if ! command -v taskset >/dev/null 2>&1; then
  echo "taskset not found; install util-linux" >&2
  exit 1
fi

for core_set in $CORE_SETS; do
  seed=$((SEED_BASE + ${#core_set}))
  tmp=$(mktemp)

  echo "[chaos-epyc-profile] running core_set=$core_set packets=$PACKETS payload=$PAYLOAD_LEN"
  taskset -c "$core_set" cargo run --release -p benchmark -- \
    --packets "$PACKETS" \
    --payload-len "$PAYLOAD_LEN" \
    --loss-percent "$LOSS_PERCENT" \
    --corrupt-percent "$CORRUPT_PERCENT" \
    --duplicate-percent "$DUPLICATE_PERCENT" \
    --seed "$seed" | tee "$tmp"

  throughput=$(grep -Eo 'throughput_pkt_s=[0-9.]+' "$tmp" | head -n1 | cut -d= -f2)
  latency_line=$(grep -E 'latency_ns p50=' "$tmp" | head -n1)
  p50=$(echo "$latency_line" | sed -E 's/.*p50=([0-9]+).*/\1/')
  p99=$(echo "$latency_line" | sed -E 's/.*p99=([0-9]+).*/\1/')
  p999=$(echo "$latency_line" | sed -E 's/.*p99_9=([0-9]+).*/\1/')

  timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  echo "$timestamp,\"$core_set\",$PACKETS,$PAYLOAD_LEN,$LOSS_PERCENT,$CORRUPT_PERCENT,$DUPLICATE_PERCENT,$throughput,$p50,$p99,$p999" >> "$OUT"

  rm -f "$tmp"
done

echo "[chaos-epyc-profile] wrote $OUT"
