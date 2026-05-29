#!/usr/bin/env bash
set -euo pipefail

if [ -z "${MOHAWK_IFACE-}" ]; then
  echo "MOHAWK_IFACE not set; set the interface and rerun. Example: MOHAWK_IFACE=ens1f0 $0"
  exit 2
fi

BINARY=./tools/hardware/smoke/target/release/mohawk_hardware_smoke
if [ ! -x "$BINARY" ]; then
  echo "Smoke binary not found; building..."
  cargo build --manifest-path tools/hardware/smoke/Cargo.toml --release
fi

echo "Running smoke test against $MOHAWK_IFACE"
"$BINARY"
