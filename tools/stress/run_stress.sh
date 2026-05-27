#!/usr/bin/env bash
set -euo pipefail

# Simple stress harness wrapper.
# Samples NIC statistics and (optionally) DUT process counters while running
# a traffic generator. Writes CSV: timestamp,rx_pkts,tx_pkts,rx_bytes,tx_bytes,dut_pid,cpu_user,cpu_sys

usage() {
  cat <<EOF
Usage: $0 --dut <path-to-dut-binary> --gen "<traffic-gen-cmd>" --iface <netif> --duration <s> --out <csv>

Environment:
  DUT_ARGS   Optional args passed to DUT binary
  PIN_CORES  Optional CPU core list for DUT (taskset format)
EOF
  exit 2
}

if [ "$#" -lt 1 ]; then usage; fi

# defaults
DURATION=60
OUT=${OUT:-/tmp/stress_pconf.csv}
RATE=${RATE:-}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dut) DUT_BIN="$2"; shift 2 ;;
    --gen) GEN_CMD="$2"; shift 2 ;;
    --iface) IFACE="$2"; shift 2 ;;
    --duration) DURATION="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --rate) RATE="$2"; shift 2 ;;
    --) shift; break ;;
    -h|--help) usage ;;
    *) echo "Unknown arg: $1"; usage ;;
  esac
done

if [ -z "${DUT_BIN:-}" ] || [ -z "${GEN_CMD:-}" ] || [ -z "${IFACE:-}" ]; then
  echo "--dut, --gen and --iface are required" >&2
  usage
fi

command -v date >/dev/null 2>&1 || { echo "date not found" >&2; exit 1; }
command -v awk >/dev/null 2>&1 || { echo "awk not found" >&2; exit 1; }

echo "Starting stress run: DUT=${DUT_BIN}, GEN='${GEN_CMD}', IFACE=${IFACE}, DURATION=${DURATION}, OUT=${OUT}"

mkdir -p "$(dirname "$OUT")"
echo "timestamp,rx_packets,tx_packets,rx_bytes,tx_bytes,dut_pid,dut_cpu_user_ns,dut_cpu_sys_ns" > "$OUT"

# start DUT
if [ -n "${PIN_CORES:-}" ]; then
  echo "Starting DUT pinned to ${PIN_CORES}"
  taskset -c ${PIN_CORES} ${DUT_BIN} ${DUT_ARGS:-} > /tmp/dut.log 2>&1 &
else
  ${DUT_BIN} ${DUT_ARGS:-} > /tmp/dut.log 2>&1 &
fi
DUT_PID=$!
echo "DUT pid=$DUT_PID"

sleep 1

# start traffic generator (must be able to run from shell)
echo "Starting traffic generator: ${GEN_CMD}"
bash -c "${GEN_CMD}" > /tmp/gen.log 2>&1 &
GEN_PID=$!

# sampling loop
END=$(( $(date +%s) + DURATION ))
while [ $(date +%s) -lt $END ]; do
  now=$(date --iso-8601=seconds)
  if [ -n "${METRICS_SOCKET:-}" ]; then
    # Query the DUT metrics unix socket for JSON: {"timestamp":..,"packets_processed": N}
    pkt_count=$(python3 - <<PY ${METRICS_SOCKET}
import socket,sys,json
sock=sys.argv[1]
try:
 s=socket.socket(socket.AF_UNIX)
 s.connect(sock)
 data=s.recv(4096).decode()
 j=json.loads(data)
 print(j.get('packets_processed',0))
except Exception as e:
 print(0)
PY
)
    rx_pkts=${pkt_count}
    tx_pkts=0
    rx_bytes=0
    tx_bytes=0
  else
    rx_pkts=$(cat /sys/class/net/${IFACE}/statistics/rx_packets 2>/dev/null || echo 0)
    tx_pkts=$(cat /sys/class/net/${IFACE}/statistics/tx_packets 2>/dev/null || echo 0)
    rx_bytes=$(cat /sys/class/net/${IFACE}/statistics/rx_bytes 2>/dev/null || echo 0)
    tx_bytes=$(cat /sys/class/net/${IFACE}/statistics/tx_bytes 2>/dev/null || echo 0)
  fi

  # gather per-process CPU times if available
  if [ -d "/proc/${DUT_PID}" ]; then
    utime=$(awk '{print $14}' /proc/${DUT_PID}/stat 2>/dev/null || echo 0)
    stime=$(awk '{print $15}' /proc/${DUT_PID}/stat 2>/dev/null || echo 0)
  else
    utime=0; stime=0
  fi

  echo "${now},${rx_pkts},${tx_pkts},${rx_bytes},${tx_bytes},${DUT_PID},${utime},${stime}" >> "$OUT"
  sleep 1
done

echo "Stopping traffic generator (pid ${GEN_PID})"
kill -TERM ${GEN_PID} 2>/dev/null || true
wait ${GEN_PID} 2>/dev/null || true

echo "Stopping DUT (pid ${DUT_PID})"
kill -TERM ${DUT_PID} 2>/dev/null || true
wait ${DUT_PID} 2>/dev/null || true

echo "Stress run complete. CSV: ${OUT}"
