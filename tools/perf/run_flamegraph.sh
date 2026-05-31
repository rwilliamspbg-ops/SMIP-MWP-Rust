#!/usr/bin/env bash
set -euo pipefail

# Run a perf sampling profile and produce a FlameGraph SVG for the
# `benchmark` binary. Intended to be run on the host (not inside a
# restricted container) where `perf` has sufficient privileges.

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"

FG_DIR="$ROOT/tools/perf/FlameGraph"
if [ ! -d "$FG_DIR" ]; then
  echo "Cloning FlameGraph into $FG_DIR"
  git clone https://github.com/brendangregg/FlameGraph.git "$FG_DIR"
fi

echo "Building release benchmark..."
cargo build --release -p benchmark

BINARY=target/release/benchmark
OUT_PERF=out.perf
FOLDED=out.folded
FLAME=flamegraph.svg

if ! command -v perf >/dev/null 2>&1; then
  echo "perf not found. Install linux-tools/perf on the host and re-run."
  exit 1
fi

echo "Recording perf (press Ctrl-C to stop early)..."
# Adjust core and sample frequency as needed
taskset -c 2 sudo perf record -F 200 -g -- $BINARY --packets 20000 --batch-size 64 --payload-len 512 --loss-percent 5 --corrupt-percent 2 --duplicate-percent 1 --seed 1337

echo "Converting perf.data to folded stacks..."
sudo perf script > "$OUT_PERF"
"$FG_DIR/stackcollapse-perf.pl" "$OUT_PERF" > "$FOLDED"
"$FG_DIR/flamegraph.pl" "$FOLDED" > "$FLAME"

echo "Wrote $FLAME"
