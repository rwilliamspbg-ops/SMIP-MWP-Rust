# Changelog

This file tracks notable repository-level changes on the current `main` branch.

## Unreleased / main

- `make verify-bridge` now uses the committed [tools/validation/verify_bridge.sh](tools/validation/verify_bridge.sh) wrapper so CI and local checkouts resolve the same path.
- The chaos gate is pinned to median aggregation with `REPS=7`, and CI uploads the resulting benchmark artifacts for inspection.
- The datapath serial hot path now encrypts payloads directly inside the output arena instead of bouncing through an intermediate ciphertext buffer.
- The README and benchmark docs were refreshed to match the current workspace, workflows, and artifact layout.

- Buffer-reuse and allocation-reduction work applied to the datapath hot path to reduce allocator pressure and mid-path Vec allocations; includes serial-path arena encryption and parallel-path consuming transformations.
- Updated test fixes: added `AlignedBuffer::as_ptr()` and a thread-local ciphertext buffer used by unit-tests.
- Added Multi-Channel Routing (MCR) spraying integration: routing table extensions, datapath hooks, and benchmark/reporting scripts. See [docs/mcr_architecture.md](docs/mcr_architecture.md).
- Bench reporting and aggregation scripts added under `tools/bench_results/` with CSV/MD outputs and CI baseline file `tools/bench_results/ci_baseline_mcr.txt` (requires perf review to merge).

### Performance notes (summary)

- Recent pinned smoke runs indicate median throughput used for CI MCR baseline: ~2,402,042 pkt/s. See `tools/bench_results/` for run artifacts and `PR_PERFORMANCE_NOTES.md` for details.
- Flamegraph capture remains pending on a self-hosted bench host (requires `perf` and a compatible rust toolchain); profiling scripts are present in `tools/benchmark/` and `benchmark/FLAMEGRAPH_RUN.md`.

## v1.0.0

- Stabilized workspace layout and core crate scaffolding.
- Added the benchmark harness and basic datapath microbench fixes.
- Validated the workspace test suite across crates.
- Refreshed benchmark artifacts in `docs/perf/` and `tools/bench_results/`.
- Added CLI metrics endpoints for local stress runs.
- Added stress harness scripts and Makefile targets for sustained load and perf collection.

## Notes

- Release notes and benchmark artifacts are generated from the current workspace state; always prefer the scripts in `tools/` over hand-editing outputs.
