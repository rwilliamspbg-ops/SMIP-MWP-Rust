Perf / FlameGraph helper
========================

This folder contains a convenience script to record `perf` samples while
running the `benchmark` binary and produce a `flamegraph.svg` using
Brendan Gregg's FlameGraph tools.

Usage (on the host with perf privileges):

```bash
cd <repo-root>
tools/perf/run_flamegraph.sh
```

Notes:
- The script will clone `FlameGraph` into `tools/perf/FlameGraph` if
  not present.
- `perf` typically requires elevated privileges; the script uses `sudo`
  for `perf record` and `perf script`.
- Adjust the `taskset` CPU pin and benchmark arguments inside the
  script for consistent measurements.
