#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 [--cores 4,5] [--repeats 10] [--sample-size 300] [--outdir tools/bench_results/remote]

Runs CPU-pinned Criterion bench runs and collects outputs into an output directory.

Options:
  --cores        Comma-separated list of cores to pin (default: 4,5)
  --repeats      Number of repeats per core (default: 10)
  --sample-size  Criterion sample size per bench invocation (default: 300)
  --outdir       Output directory for raw bench outputs (default: tools/bench_results/remote)
  --commit       If set to "yes", will `git add` and `git commit` results
  --help         Show this message
EOF
}

CORES="4,5"
REPEATS=10
SAMPLE_SIZE=300
OUTDIR="tools/bench_results/remote"
COMMIT=no

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cores) CORES="$2"; shift 2;;
    --repeats) REPEATS="$2"; shift 2;;
    --sample-size) SAMPLE_SIZE="$2"; shift 2;;
    --outdir) OUTDIR="$2"; shift 2;;
    --commit) COMMIT="$2"; shift 2;;
    --help) usage; exit 0;;
    *) echo "Unknown arg: $1"; usage; exit 1;;
  esac
done

mkdir -p "$OUTDIR"

IFS=',' read -r -a CORE_LIST <<< "$CORES"

for core in "${CORE_LIST[@]}"; do
  for run in $(seq 1 "$REPEATS"); do
    T=$(date +%Y%m%d_%H%M%S)
    outfile="$OUTDIR/routing_miss_remote_core${core}_run${run}_$T.txt"
    echo "[bench] core=$core run=$run out=$outfile"
    taskset -c "$core" cargo bench --bench routing_miss_bench -- --sample-size "$SAMPLE_SIZE" --nocapture | tee "$outfile" || true
    sleep 1
  done
done

echo "Parsing and generating stats/plots (if scripts available)"
if command -v python3 >/dev/null 2>&1; then
  python3 tools/parse_bench.py || true
  python3 tools/compute_stats.py || true
  python3 tools/plot_bench.py || true
fi

if [[ "$COMMIT" == "yes" ]]; then
  git add "$OUTDIR" || true
  git add tools/bench_results/routing_miss_summary.csv tools/bench_results/routing_miss_stats.csv tools/bench_results/plots || true
  git commit -m "bench: remote pinned runs $(date +%Y%m%d)" || true
fi

echo "Done. Outputs in: $OUTDIR"
