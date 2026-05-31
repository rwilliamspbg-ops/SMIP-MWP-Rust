#!/usr/bin/env bash
set -euo pipefail

# Run flamegraph collection for selected benches on a self-hosted bench host.
# Usage: ./tools/benchmark/run_flamegraphs.sh [--cores 2,3] [--sample-size 100]

CORES=${CORES:-}
SAMPLE=${SAMPLE:-100}
OUTDIR=${OUTDIR:-tools/bench_results}

usage(){
  cat <<'EOF'
Usage: run_flamegraphs.sh [--cores 2,3] [--sample-size 100] [--outdir PATH]

This script expects to run on a self-hosted bench host with `perf` available
and a compatible `cargo flamegraph` installation. It will build benches and
produce SVG flamegraphs in `--outdir`.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cores)
      CORES="$2"; shift 2;;
    --sample-size)
      SAMPLE="$2"; shift 2;;
    --outdir)
      OUTDIR="$2"; shift 2;;
    --help)
      usage; exit 0;;
    *) echo "Unknown arg: $1" >&2; usage; exit 1;;
  esac
done

mkdir -p "$OUTDIR"

echo "Checking for perf..."
if ! command -v perf >/dev/null 2>&1; then
  echo "perf not found. On Debian/Ubuntu run: sudo apt update && sudo apt install -y linux-tools-common linux-tools-$(uname -r) perf" >&2
  exit 2
fi

echo "Ensure you have a cargo flamegraph available. If not, install a compatible version:\n  cargo install flamegraph --version 0.6.11 --locked"

echo "Building bench profile..."
cargo build -p bench --release

RUN_FLAMEGRAPH(){
  bench_name="$1"
  out_svg="$OUTDIR/${bench_name}_flamegraph.svg"
  echo "Running flamegraph for $bench_name -> $out_svg"
  if [[ -n "$CORES" ]]; then
    taskset -c "$CORES" cargo flamegraph -p bench --bench "$bench_name" --output "$out_svg" -- --sample-size "$SAMPLE"
  else
    cargo flamegraph -p bench --bench "$bench_name" --output "$out_svg" -- --sample-size "$SAMPLE"
  fi
}

# Bench list: adjust as needed
BENCHES=(alloc_bench routing_miss_bench datapath_bench)

for b in "${BENCHES[@]}"; do
  RUN_FLAMEGRAPH "$b" || echo "flamegraph for $b failed (check perf/cargo flamegraph)" >&2
done

echo "Flamegraph run complete. Output dir: $OUTDIR"
