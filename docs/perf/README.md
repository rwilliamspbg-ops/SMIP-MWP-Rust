# Performance Artifacts

This directory tracks checked-in Criterion outputs and routing-sweep artifacts for the current workspace state.

## Contents

- `bench_results.csv` - parsed benchmark results
- `routing_miss_sweep.csv` - main routing sweep output
- `routing_miss_sweep.svg` - main routing sweep plot
- `routing_miss_sweep_broad.csv` / `routing_miss_sweep_broad.svg` - broad parameter sweep outputs
- `routing_miss_sweep_fine.csv` / `routing_miss_sweep_fine.svg` - fine parameter sweep outputs

## How It Is Used

- The top-level `bench` crate and scripts in `tools/bench_harness/` generate these files.
- Recent raw pinned-run outputs and aggregate summaries are kept under `tools/bench_results/` for deeper inspection.
- The files in this directory are historical artifacts; regenerate them from the scripts when the benchmark configuration changes.
