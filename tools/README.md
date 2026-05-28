# tools

Purpose: bench harness drivers, plotting, and helper scripts used for result aggregation.

Typical usage

```sh
./tools/bench_harness/run_bench_harness.sh 20 bench_results.csv
python3 tools/bench_harness/plot_routing_miss_sweep.py routing_miss_sweep.csv routing_miss_sweep.svg
```

Notes

- Tools are plain Python/shell scripts; ensure Python 3 is available for plotting utilities.
