# bench

Purpose: Criterion microbench harness and smoke-run utilities.

Build & run

```sh
cd bench
cargo bench
```

Harness

- See `../tools/bench_harness` for automation scripts that produce CSVs and plots.
- For outlier-sensitive runs, start with `../tools/benchmark/benchmark_mode.sh --cores 2-3 --hugepages 1024 --strict` and then launch the bench under the same pinned cores.
