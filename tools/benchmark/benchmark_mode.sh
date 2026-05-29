#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: benchmark_mode.sh [--cores 2-3] [--hugepages 1024] [--strict] [--no-irqbalance-check] [--] [command ...]

Prints a benchmark-mode checklist for CPU pinning, hugepages, IRQ balancing,
and NUMA balancing. When a command is provided after `--`, it runs the command
under `taskset -c <cores>` if `--cores` is set.

Options:
  --cores                 Expected CPU core list in taskset format.
  --hugepages             Minimum hugepage count to require (default: 1024).
  --strict                Exit non-zero if any checklist item fails.
  --no-irqbalance-check   Skip the irqbalance status check.
  --help                  Show this message.
EOF
}

CORES=${PIN_CORES:-}
HUGE_PAGES=${HUGE_PAGES:-1024}
STRICT=0
CHECK_IRQBALANCE=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cores)
      CORES="$2"
      shift 2
      ;;
    --hugepages)
      HUGE_PAGES="$2"
      shift 2
      ;;
    --strict)
      STRICT=1
      shift
      ;;
    --no-irqbalance-check)
      CHECK_IRQBALANCE=0
      shift
      ;;
    --help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

COMMAND=("$@")
warnings=0

pass() { echo "[benchmark-mode] PASS: $*"; }
warn() {
  echo "[benchmark-mode] WARN: $*" >&2
  warnings=$((warnings + 1))
}

echo "[benchmark-mode] checklist"
echo "[benchmark-mode] pid=$$ cwd=$(pwd)"

if [[ -n "$CORES" ]]; then
  current_affinity=$(taskset -pc $$ 2>/dev/null | awk -F: '{print $2}' | tr -d '[:space:]')
  if [[ "$current_affinity" == "$CORES" ]]; then
    pass "current shell affinity matches requested cores ($CORES)"
  else
    warn "current shell affinity is '$current_affinity'; expected '$CORES'. Run under: taskset -c $CORES <command>"
  fi
else
  warn "no core list provided; set PIN_CORES or pass --cores to pin benchmark runs"
fi

nr_hugepages=""
if [[ -r /proc/sys/vm/nr_hugepages ]]; then
  nr_hugepages=$(< /proc/sys/vm/nr_hugepages)
  if (( nr_hugepages >= HUGE_PAGES )); then
    pass "vm.nr_hugepages=$nr_hugepages meets target >= $HUGE_PAGES"
  else
    warn "vm.nr_hugepages=$nr_hugepages is below target $HUGE_PAGES; raise it with sudo sysctl -w vm.nr_hugepages=$HUGE_PAGES"
  fi
else
  warn "unable to read /proc/sys/vm/nr_hugepages"
fi

if [[ -r /proc/meminfo ]]; then
  hugepages_total=$(awk '/^HugePages_Total:/ {print $2}' /proc/meminfo)
  hugepages_free=$(awk '/^HugePages_Free:/ {print $2}' /proc/meminfo)
  echo "[benchmark-mode] HugePages_Total=${hugepages_total:-unknown} HugePages_Free=${hugepages_free:-unknown}"
fi

if [[ -r /proc/sys/kernel/numa_balancing ]]; then
  numa_balancing=$(< /proc/sys/kernel/numa_balancing)
  if [[ "$numa_balancing" == "0" ]]; then
    pass "kernel.numa_balancing is disabled"
  else
    warn "kernel.numa_balancing=$numa_balancing; consider disabling it for benchmarking"
  fi
fi

if [[ "$CHECK_IRQBALANCE" == "1" ]]; then
  if command -v pgrep >/dev/null 2>&1 && pgrep -x irqbalance >/dev/null 2>&1; then
    warn "irqbalance is running; disable it when manually pinning IRQs for benchmarks"
  else
    pass "irqbalance is not running"
  fi
fi

echo "[benchmark-mode] checklist reminder: pin NIC IRQs with /proc/interrupts and /proc/irq/*/smp_affinity, then verify no noisy processes are saturating your benchmark CPUs."

if [[ ${#COMMAND[@]} -gt 0 ]]; then
  if [[ -n "$CORES" ]]; then
    echo "[benchmark-mode] running: taskset -c $CORES ${COMMAND[*]}"
    exec taskset -c "$CORES" "${COMMAND[@]}"
  fi

  echo "[benchmark-mode] running: ${COMMAND[*]}"
  exec "${COMMAND[@]}"
fi

if [[ "$STRICT" == "1" && "$warnings" -gt 0 ]]; then
  exit 1
fi
