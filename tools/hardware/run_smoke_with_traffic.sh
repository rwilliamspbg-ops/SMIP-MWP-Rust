#!/usr/bin/env bash
set -euo pipefail

# Orchestrate building and running the AF_XDP smoke binary and an optional
# traffic generator. Default is dry-run; pass --run and set RUN_REAL_SMOKE=1 to
# actually execute.

MODE=dry-run
if [ "${1-}" = "--run" ]; then
  MODE=run
fi

echo "[run_smoke_with_traffic] mode=$MODE"

if [ "$MODE" = "dry-run" ]; then
  echo "DRY RUN: This will build the smoke binary, start it, run the traffic generator (if provided), and collect logs.";
  echo "Env variables that affect behavior:";
  echo "  MOHAWK_IFACE (required when running)";
  echo "  RUN_REAL_SMOKE (must be set to 1 to confirm real run)";
  echo "  SMOKE_GEN_CMD (optional traffic generator command)";
  echo "  SMOKE_GEN_DURATION (seconds, default 10)";
  echo "To actually run: set MOHAWK_IFACE and RUN_REAL_SMOKE=1 and run with --run";
  exit 0
fi

if [ -z "${MOHAWK_IFACE-}" ]; then
  echo "MOHAWK_IFACE not set; aborting. Example: MOHAWK_IFACE=ens1f0 RUN_REAL_SMOKE=1 $0 --run";
  exit 2
fi

if [ "${RUN_REAL_SMOKE-0}" != "1" ]; then
  echo "RUN_REAL_SMOKE not set to 1; refusing to run smoke. Set RUN_REAL_SMOKE=1 to confirm.";
  exit 2
fi

BINARY=./tools/hardware/smoke/target/release/mohawk_hardware_smoke
if [ ! -x "$BINARY" ]; then
  echo "Building smoke binary..."
  cargo build --manifest-path tools/hardware/smoke/Cargo.toml --release
fi

LOGDIR=tools/hardware/smoke/logs
mkdir -p "$LOGDIR"
SMOKE_LOG="$LOGDIR/smoke.$(date -u +%Y%m%dT%H%M%SZ).log"

echo "Starting smoke binary; logging to $SMOKE_LOG";
"$BINARY" >"$SMOKE_LOG" 2>&1 &
SMOKE_PID=$!
echo "smoke pid=$SMOKE_PID"

GEN_CMD="${SMOKE_GEN_CMD-}"
GEN_DUR="${SMOKE_GEN_DURATION-10}"

if [ -n "$GEN_CMD" ]; then
  echo "Running traffic generator for $GEN_DUR seconds: $GEN_CMD";
  # Run generator under timeout to limit duration
  timeout "$GEN_DUR" bash -c "$GEN_CMD" || true
else
  echo "No SMOKE_GEN_CMD provided; sleeping $GEN_DUR seconds to let smoke run";
  sleep "$GEN_DUR"
fi

echo "Traffic phase complete; stopping smoke binary (pid=$SMOKE_PID)";
kill "$SMOKE_PID" || true
wait "$SMOKE_PID" 2>/dev/null || true

echo "Smoke run finished. Logs: $SMOKE_LOG"
exit 0
