#!/usr/bin/env bash
set -euo pipefail

HUGE_PAGES=${HUGE_PAGES:-1024}
PIN_CORES=${PIN_CORES:-2-3}
DRY_RUN=${DRY_RUN:-0}

run_cmd() {
  if [[ "$DRY_RUN" == "1" ]]; then
    echo "[dry-run] $*"
  else
    eval "$*"
  fi
}

echo "[setup-hardware] hugepages=$HUGE_PAGES pin_cores=$PIN_CORES dry_run=$DRY_RUN"

# Hugepages setup requires root; keep this explicit and opt-in.
if [[ "$EUID" -ne 0 && "$DRY_RUN" != "1" ]]; then
  echo "[setup-hardware] root privileges required for hugepages and IRQ tuning"
  echo "[setup-hardware] re-run with sudo or DRY_RUN=1"
  exit 1
fi

run_cmd "sysctl -w vm.nr_hugepages=$HUGE_PAGES"
run_cmd "sysctl -w kernel.numa_balancing=0"

# Keep benchmark process pinning explicit for callers.
echo "[setup-hardware] suggested benchmark prefix: taskset -c $PIN_CORES"

echo "[setup-hardware] complete"
