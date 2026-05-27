#!/usr/bin/env bash
set -euo pipefail

# Profile a stress run with perf. Usage mirrors run_stress.sh but requires sudo

usage() {
  cat <<EOF
Usage: $0 --dut <path-to-dut-binary> --gen "<traffic-gen-cmd>" --iface <netif> --duration <s> --out <csv>
Example: sudo $0 --dut ./target/release/mohawk-node --gen "trex-64r" --iface eth0 --duration 60 --out /tmp/pconf.csv
EOF
  exit 2
}

if [ "$#" -lt 1 ]; then usage; fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dut) DUT_BIN="$2"; shift 2 ;;
    --gen) GEN_CMD="$2"; shift 2 ;;
    --iface) IFACE="$2"; shift 2 ;;
    --duration) DURATION="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --) shift; break ;;
    -h|--help) usage ;;
    *) echo "Unknown arg: $1"; usage ;;
  esac
done

if [ -z "${DUT_BIN:-}" ] || [ -z "${GEN_CMD:-}" ] || [ -z "${IFACE:-}" ]; then
  echo "--dut, --gen and --iface are required" >&2
  usage
fi

PERF_OUT=${PERF_OUT:-/tmp/perf.data}
SVG_OUT=${SVG_OUT:-/tmp/perf.svg}

echo "Starting profile run: perf -> ${PERF_OUT}"

# Run stress run in bg
./run_stress.sh --dut "${DUT_BIN}" --gen "${GEN_CMD}" --iface "${IFACE}" --duration "${DURATION}" --out "${OUT}" &
STRESS_PID=$!

sleep 1

# Find DUT pid by matching binary name
DUT_PID=$(pgrep -f "${DUT_BIN}" | head -n1 || true)
if [ -z "$DUT_PID" ]; then
  echo "Could not find DUT pid; running perf record for entire system"
  sudo perf record -F 99 -a -g -- sleep ${DURATION}
else
  echo "Recording perf for pid ${DUT_PID}"
  sudo perf record -F 99 -p ${DUT_PID} -g -- sleep ${DURATION}
fi

echo "Generating perf report/svg (if FlameGraph tools present)"
if command -v perf >/dev/null 2>&1; then
  sudo perf report --stdio > ${PERF_OUT}.report || true
fi

if [ -x ./FlameGraph/flamegraph.pl ] && [ -f out.perf ]; then
  ./FlameGraph/stackcollapse-perf.pl out.perf > out.folded || true
  ./FlameGraph/flamegraph.pl out.folded > ${SVG_OUT} || true
  echo "wrote flamegraph to ${SVG_OUT}"
fi

wait ${STRESS_PID} 2>/dev/null || true
echo "Profile run complete"
