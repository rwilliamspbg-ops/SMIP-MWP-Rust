#!/usr/bin/env bash
set -euo pipefail

# Safe wrapper to build and optionally run the AF_XDP hardware smoke test.
# Usage:
#   ./tools/hardware/run_smoke_safe.sh --dry-run
#   RUN_REAL_SMOKE=1 MOHAWK_IFACE=ens1f0 ./tools/hardware/run_smoke_safe.sh --run

MODE=dry-run
if [ "${1-}" = "--run" ]; then
  MODE=run
fi

echo "[run_smoke_safe] mode=$MODE"

if [ "$MODE" = "dry-run" ]; then
  echo "DRY RUN: The following steps would be executed when running in real mode:";
  echo "  - Verify MOHAWK_IFACE is set";
  echo "  - Build smoke binary: cargo build --manifest-path tools/hardware/smoke/Cargo.toml --release";
  echo "  - Execute smoke binary: tools/hardware/smoke/target/release/mohawk_hardware_smoke";
  echo "To actually run: set MOHAWK_IFACE and RUN_REAL_SMOKE=1 and re-run with --run";
  exit 0
fi

if [ -z "${MOHAWK_IFACE-}" ]; then
  echo "MOHAWK_IFACE not set; aborting. Example: MOHAWK_IFACE=ens1f0 $0 --run";
  exit 2
fi

if [ -z "${RUN_REAL_SMOKE-}" ]; then
  echo "RUN_REAL_SMOKE not set; refusing to run smoke test. Set RUN_REAL_SMOKE=1 to confirm.";
  exit 2
fi

BINARY=./tools/hardware/smoke/target/release/mohawk_hardware_smoke
if [ ! -x "$BINARY" ]; then
  echo "Building smoke binary..."
  cargo build --manifest-path tools/hardware/smoke/Cargo.toml --release
fi

LOGFILE=tools/hardware/smoke/run_smoke.log
echo "Running smoke test against $MOHAWK_IFACE; logging to $LOGFILE"
"$BINARY" >"$LOGFILE" 2>&1 || {
  echo "Smoke test failed; see $LOGFILE for details";
  exit 3
}

echo "Smoke test completed successfully; output in $LOGFILE"
exit 0
