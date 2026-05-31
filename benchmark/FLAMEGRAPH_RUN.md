# Flamegraph collection on self-hosted bench hosts

Prerequisites:

- A self-hosted bench host with `perf` installed (Debian/Ubuntu):

```bash
sudo apt update && sudo apt install -y linux-tools-common linux-tools-$(uname -r) perf
```

- A compatible Rust toolchain. If your system rustc < 1.86, install `flamegraph` v0.6.11:

```bash
cargo install flamegraph --version 0.6.11 --locked
```

How to run (example):

```bash
# pin to cores 2 and 3, sample size 200, write outputs to tools/bench_results
CORES=2,3 SAMPLE=200 OUTDIR=tools/bench_results ./tools/benchmark/run_flamegraphs.sh
```

Generated files:

- `tools/bench_results/alloc_bench_flamegraph.svg`
- `tools/bench_results/routing_miss_bench_flamegraph.svg`
- `tools/bench_results/datapath_bench_flamegraph.svg`

Notes:

- Flamegraph collection requires privileges to use `perf` on some kernels — you may need to run as root or enable perf_event_paranoid=1 appropriately.
- Run the script on the same pinned CPU/core configuration you used for the baseline runs to reduce variance.
