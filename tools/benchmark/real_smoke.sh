#!/usr/bin/env bash
set -euo pipefail

# Simple hardware smoke test runner for AF_XDP-enabled builds.
# Usage:
#   MOHAWK_IFACE=eth0 MOHAWK_QUEUE_ID=0 ./tools/benchmark/real_smoke.sh

ROOT_DIR=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT_DIR"

if [ -z "${MOHAWK_IFACE:-}" ]; then
  echo "MOHAWK_IFACE must be set to run hardware smoke tests"
  exit 2
fi

echo "Running hardware smoke test on iface ${MOHAWK_IFACE} (queue ${MOHAWK_QUEUE_ID:-0})"

# Run the CLI with real AF_XDP feature enabled. Adjust frame size and umem pages
# via environment variables if needed.
cargo run --release -p cli --features real -- --bridge-request bridge/examples/control_request.example.json

echo "Hardware smoke test complete"
