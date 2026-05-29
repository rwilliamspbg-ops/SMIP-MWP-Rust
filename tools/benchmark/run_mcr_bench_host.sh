#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
Usage:
  MOHAWK_IFACE=ens1f0 [MOHAWK_QUEUE_ID=0] [METRICS_SOCKET=/tmp/mohawk.metrics.sock] \
  [PIN_CORES=2-3] [HUGE_PAGES=1024] [MCR_CHANNELS=3] [MCR_SPRAY_MODE=primary] \
  ./tools/benchmark/run_mcr_bench_host.sh

Required env vars:
  MOHAWK_IFACE        AF_XDP interface to use for the hardware smoke probe.

Optional env vars:
  MOHAWK_QUEUE_ID     Queue ID for the hardware smoke probe (default: 0).
  METRICS_SOCKET      Metrics socket path if you want the smoke probe/CLI to expose metrics.
  PIN_CORES           CPU pinning hint for benchmark-mode enforcement (default: 2-3).
  HUGE_PAGES          Huge page target for benchmark-mode enforcement (default: 1024).
  MCR_CHANNELS        Channel count for the benchmark matrix (default: 3).
  MCR_SPRAY_MODE      Spray mode for the benchmark matrix (default: primary).
  BENCH_OUT           Output CSV path for the benchmark matrix (default: tools/bench_results/chaos_epyc_profile.csv).
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

if [ -z "${MOHAWK_IFACE:-}" ]; then
  echo "MOHAWK_IFACE is required" >&2
  usage >&2
  exit 2
fi

MOHAWK_QUEUE_ID=${MOHAWK_QUEUE_ID:-0}
PIN_CORES=${PIN_CORES:-2-3}
HUGE_PAGES=${HUGE_PAGES:-1024}
MCR_CHANNELS=${MCR_CHANNELS:-3}
MCR_SPRAY_MODE=${MCR_SPRAY_MODE:-primary}
BENCH_OUT=${BENCH_OUT:-tools/bench_results/chaos_epyc_profile.csv}

mkdir -p tools/bench_results benchmark

export MOHAWK_IFACE
export MOHAWK_QUEUE_ID
export PIN_CORES
export HUGE_PAGES
export MCR_CHANNELS
export MCR_SPRAY_MODE

if [ -n "${METRICS_SOCKET:-}" ]; then
  export METRICS_SOCKET
fi

echo "[mcr-bench-host] enforcing benchmark-mode prerequisites"
make benchmark-mode-enforce

echo "[mcr-bench-host] building MCR stack"
make mcr-build

echo "[mcr-bench-host] running routing/datapath unit tests"
make mcr-test

echo "[mcr-bench-host] running AF_XDP hardware smoke probe"
./tools/benchmark/real_smoke.sh

echo "[mcr-bench-host] running MCR benchmark matrix"
MOHAWK_MCR_CHANNELS="$MCR_CHANNELS" MOHAWK_MCR_SPRAY_MODE="$MCR_SPRAY_MODE" \
  ./tools/benchmark/run_chaos_epyc_profile.sh "$BENCH_OUT"

echo "[mcr-bench-host] generating MCR report"
python3 tools/benchmark/generate_mcr_report.py \
  --input "$BENCH_OUT" \
  --output benchmark/mcr_chaos_report.md

echo "[mcr-bench-host] running bridge validation"
make verify-bridge

echo "[mcr-bench-host] complete"
