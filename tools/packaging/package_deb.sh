#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo-deb >/dev/null 2>&1; then
  echo "cargo-deb not found. Install with: cargo install cargo-deb"
  exit 1
fi

echo "Building .deb packages for workspace binaries"
cargo deb --no-strip --target x86_64-unknown-linux-gnu || true
cargo deb --no-strip --target aarch64-unknown-linux-gnu || true

echo "Deb artifacts will be under target/*/debian/"