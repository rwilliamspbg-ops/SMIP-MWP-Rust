#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT_DIR"

echo "[verify-bridge] validating Rust bridge contract artifacts"
cargo test -p cli bridge_contract -- --nocapture

echo "[verify-bridge] validating bridge request example round-trip"
cargo run --release -p cli -- --bridge-request bridge/examples/control_request.example.json >/tmp/verify_bridge_cli.txt
if ! grep -q "bridge request accepted" /tmp/verify_bridge_cli.txt; then
  echo "[verify-bridge] expected bridge acceptance output not found"
  exit 1
fi

echo "[verify-bridge] scanning for Go boundary"
if find . -type f -name "*.go" -not -path "./target/*" | grep -q .; then
  echo "[verify-bridge] Go sources detected"
  if command -v go >/dev/null 2>&1; then
    go test ./...
  else
    echo "[verify-bridge] go toolchain missing while Go files are present"
    exit 1
  fi
else
  echo "[verify-bridge] no Go sources detected; Rust-side bridge checks only"
fi

echo "[verify-bridge] complete"
