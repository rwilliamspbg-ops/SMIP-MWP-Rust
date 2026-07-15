#!/usr/bin/env bash
set -euo pipefail

TARGETS=(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-musl)

echo "Ensure Rust toolchain is installed (1.81.0 recommended)"
for t in "${TARGETS[@]}"; do
  echo "Adding target: $t"
  rustup target add "$t" || true
done

if command -v cross >/dev/null 2>&1; then
  echo "Using cross to build all targets"
  for t in "${TARGETS[@]}"; do
    cross build --release --target "$t"
  done
else
  echo "Cross not found; falling back to native cargo builds for compatible targets"
  for t in "${TARGETS[@]}"; do
    cargo build --release --target "$t" || true
  done
  echo "For reliable cross-builds consider installing 'cross' (https://github.com/rust-embedded/cross)"
fi

echo "Build artifacts are available under target/<target>/release/"