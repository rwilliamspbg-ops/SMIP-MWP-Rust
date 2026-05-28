**PR Recommendation: perf/tune-routing-cache (PR #16)**

Summary
-------

- Implemented per-thread `HOT_CACHE` and sharded `fast_shards` fast-path while keeping the main `Table` fallback on `RwLock<TableInner>`.
- Collected extensive benchmark artifacts (criterion + CPU-pinned runs). Artifacts are in `tools/bench_results/` and include parsed CSVs and plots.
- Multi-core pinned sweeps (cores 2 & 3) were repeated 10× each; cores 4 & 5 experienced affinity/launch failures in this environment and have incomplete data.

Primary takeaway
----------------

Given the current dataset, the `rwlock` baseline variant with the per-thread hot-cache + sharded fast-path consistently shows higher median throughput across most tested route-table sizes. The `hybrid` lock-free variant regressed in multiple sizes under repeated pinned runs.

Recommendation
--------------

1. Merge `perf/tune-routing-cache` (PR #16) to bring the following benefits into `main`:
   - Stability improvement: `RwLock` baseline avoids regressions observed in lock-free prototypes.
   - Performance gains: per-thread `HOT_CACHE` and `fast_shards` reduce hot-path overheads in common cases.
   - Artifacts: bench CSVs, stats, and plots are included to support the decision and future tuning.

2. Post-merge next steps (follow-up PRs):
   - Complete systematic parameter sweep for `FAST_SHARDS`, `HOT_CACHE_SIZE`, and `HOT_CACHE_PROBE` on isolated hardware with pinned runs (5–10 repeats per sample) and add results to `docs/perf`.
   - If pursuing lock-free hybrid paths again, constrain changes to a single measurable variable per PR and include high-repeat pinned benchmarks.
   - Add a GitHub Actions bench job that runs the bench-harness on a repeatable runner (if available) and uploads parsed CSVs as workflow artifacts for reproducibility.

Notes for reviewers
------------------

- Many bench samples still have low `n` values for some variants; the included `routing_miss_stats.csv` marks `n` and `ci95` so reviewers can see where further runs are required.
- The pinned-run raw outputs are `tools/bench_results/routing_miss_pinned_core{2..3}_run{1..N}_<timestamp>.txt` and were committed with this branch for transparency.

If you approve, I will merge on maintainer approval and follow up with the parameter-sweep plan in a new branch/PR.
