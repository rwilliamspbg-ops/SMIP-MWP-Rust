# Chaos Benchmark Harness

This directory contains the chaos benchmark harness and the generated performance-envelope artifacts for the datapath.

## What it measures

- Throughput (`throughput_pkt_s`)
- Latency percentiles (`p50`, `p99`, `p99.9`) in nanoseconds
- Fault injection counters (`drop`, `corrupt`, `duplicate`)

## Primary Commands

```sh
cargo run --release -p benchmark -- \
  --packets 20000 \
  --batch-size 64 \
  --payload-len 512 \
  --loss-percent 5 \
  --corrupt-percent 2 \
  --duplicate-percent 1 \
  --seed 1337

make performance-envelope
REPS=7 AGG_METHOD=median ./tools/benchmark/ci_validate_chaos_report.sh
```

## CI Contract

The CI workflow runs the benchmark in two modes:

- A direct threshold assertion over a single benchmark sample via `tools/benchmark/assert_chaos_thresholds.py`
- A baseline-aware contract gate via `tools/benchmark/ci_validate_chaos_report.sh`

CI uploads these artifacts for debugging and review:

- `benchmark/chaos_report.md`
- `tools/bench_results/chaos_epyc_profile.csv`
- `tools/bench_results/datapath_profile.csv`
- `tools/bench_results/datapath_alloc_events.csv`
- `tools/bench_results/datapath_handle_events.csv`

## Generated Artifacts

The performance-envelope command updates the checked-in reporting files:

- `benchmark/PERFORMANCE_ENVELOPE.md`
- `benchmark/report_throughput.md`
- `benchmark/report_latency.png`
- `benchmark/report_mpps.txt`
- `benchmark/crypto_overhead.md`
- `benchmark/chaos_report.md`

## Supporting Scripts

- `tools/benchmark/run_chaos_epyc_profile.sh` - runs the pinned-core chaos matrix
- `tools/benchmark/generate_chaos_report.py` - formats the contract report
- `tools/benchmark/generate_latency_plot.py` - renders the latency plot
- `tools/benchmark/ci_validate_chaos_report.sh` - baseline-aware gate used by CI
