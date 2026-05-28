# Release v1.0.0

Tag: `v1.0.0`

This release marks the initial v1.0 milestone for the Rust rewrite workspace. It contains:

- Stabilized workspace layout and core crate scaffolding
- Bench harness and basic datapath microbench fixes
- Test-suite validation across crates
- Refreshed benchmark artifacts in `docs/perf/` with the latest routing miss sweep and bench outputs
- CLI metrics endpoints for local stress runs (`--metrics`, `--metrics-socket`, and `--metrics-http`)
- Stress harness scripts and Makefile targets for sustained load and perf collection

For release notes and changelog, see the project GitHub releases page.

Checklist (current release state):

- [x] Stabilized workspace layout and core crate scaffolding
- [x] Bench harness and basic datapath microbench fixes
- [x] Test-suite validation across crates
- [x] Refreshed benchmark artifacts included (see `tools/bench_results/`)
- [x] CPU-pinned bench sweeps captured for routing tuning (cores 2 & 3, multi-run artifacts)

Notes:
- A performance tuning PR (perf/tune-routing-cache, PR #16) includes updated bench artifacts and plots. The current recommendation from these local runs is to merge the `RwLock` baseline with the per-thread hot-cache and fast-shards; further hybrid tuning is optional and should follow a structured parameter sweep with pinned repeats for statistical confidence.
