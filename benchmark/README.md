# Chaos Benchmark Harness

This directory contains a benchmark harness focused on high-speed datapath validation under Byzantine packet-loss scenarios.

Performance contract details are defined in `benchmark/PERFORMANCE_ENVELOPE.md`.

## What it measures

- Pkt/s throughput (`throughput_pkt_s`)
- Latency distribution (`p50`, `p99`, `p99.9`) in nanoseconds
- Fault injection counters (`drop`, `corrupt`, `duplicate`)

## Run

```sh
cargo run --release -p benchmark -- \
  --packets 20000 \
  --batch-size 64 \
  --payload-len 512 \
  --loss-percent 5 \
  --corrupt-percent 2 \
  --duplicate-percent 1 \
  --seed 1337
```

## Chaos model

For each generated packet, the harness can:

1. Drop it (simulated Byzantine loss)
2. Corrupt/truncate it (simulated malformed traffic)
3. Duplicate it (simulated replay/duplication)

This is intentionally synthetic and reproducible so regressions can be tracked in CI and local runs.

## EPYC profile matrix

Run pinned-core profiles and export CSV artifacts:

```sh
chmod +x tools/benchmark/run_chaos_epyc_profile.sh
./tools/benchmark/run_chaos_epyc_profile.sh tools/bench_results/chaos_epyc_profile.csv
```

The CSV contains throughput and latency percentiles per core set.

## CI threshold gate

The CI workflow runs this harness and validates minimum throughput and latency ceilings using:

```sh
python3 tools/benchmark/assert_chaos_thresholds.py --input /tmp/chaos_benchmark_ci.txt
```

The CI workflow also runs a baseline-aware contract gate that requires `benchmark/chaos_report.md` status to be `PASS`:

```sh
./tools/benchmark/ci_validate_chaos_report.sh
```

CI uploads these artifacts for inspection:

- `benchmark/chaos_report.md`
- `tools/bench_results/chaos_epyc_profile.csv`
- `/tmp/chaos_benchmark_ci.txt`

## Required envelope artifacts

Generate mandatory validation artifacts before claiming final performance figures:

```sh
make performance-envelope
```

This produces/updates:

- `benchmark/report_throughput.md`
- `benchmark/report_latency.png`
- `benchmark/report_mpps.txt`
- `benchmark/crypto_overhead.md`
- `benchmark/chaos_report.md`
