# Changelog

This file tracks notable repository-level changes on the current `main` branch.

## Unreleased / main

- `make verify-bridge` now uses the committed [tools/validation/verify_bridge.sh](tools/validation/verify_bridge.sh) wrapper so CI and local checkouts resolve the same path.
- The chaos gate is pinned to median aggregation with `REPS=7`, and CI uploads the resulting benchmark artifacts for inspection.
- The datapath serial hot path now encrypts payloads directly inside the output arena instead of bouncing through an intermediate ciphertext buffer.
- The README and benchmark docs were refreshed to match the current workspace, workflows, and artifact layout.

## v1.0.0

- Stabilized workspace layout and core crate scaffolding.
- Added the benchmark harness and basic datapath microbench fixes.
- Validated the workspace test suite across crates.
- Refreshed benchmark artifacts in `docs/perf/` and `tools/bench_results/`.
- Added CLI metrics endpoints for local stress runs.
- Added stress harness scripts and Makefile targets for sustained load and perf collection.

## Notes

- Release notes and benchmark artifacts are generated from the current workspace state; always prefer the scripts in `tools/` over hand-editing outputs.
