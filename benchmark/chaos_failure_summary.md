# Chaos Gate Failure Summary

Status: **FAIL** (baseline-aware CI gate)

Run summary (local run):
- Baseline (ideal-mode): `throughput_pkt_s=1315406.62`, `p99_ns=62096`
- Aggregation: `trimmed mean` over 5 reps
- Aggregated chaos: `throughput_pkt_s=1335036.93`, `p99_ns=84605`
- Results: throughput drop ~ -1.49% (within envelope), p99 increase = 22509 ns (exceeds 1000 ns target)

Observed behavior:
- Median/trimmed aggregation reduces sensitivity to outlier runs, but p99 remains far above the 1 µs goal under Byzantine injection.
- Some runs show extreme p99_9 spikes (e.g., >400k ns), indicating tail events.

Immediate next actions (recommended):
1. Profile hot paths under pinned cores using `perf` during a chaos run that shows high p99. Capture `perf record` and flamegraph.
2. Investigate AVX2/unaligned memcpy paths in `datapath` and `afxdp` for variable latency; add alignment checks.
3. Reduce noise in harness: increase `--packets` and `--batch-size` to get more stable percentiles for p99/p99.9.
4. Add targeted microbench for the miss-path to isolate lock contention or cache-miss storms.
5. If Go control-plane interactions are present, ensure `DEPLOYMENT.manifest.md` flags them and isolate the forwarding fast path.

Suggested commands to collect profiling artifacts:

```sh
# run a chaos rep pinned to core 2 and record perf
taskset -c 2 perf record -F 99 -g -- ./target/release/benchmark --packets 50000 --batch-size 64 --payload-len 1024 --loss-percent 3 --corrupt-percent 1 --duplicate-percent 1 --seed 20260531
perf script | ./tools/flamegraph/stackcollapse-perf.pl > out.folded
./tools/flamegraph/flamegraph.pl out.folded > perf_flamegraph.svg
```

If you want, I can: run a pinned `perf` recording from here (requires `perf` and flamegraph tools in environment), or open a PR with a targeted datapath instrumentation patch. Which would you prefer me to do next?

Profiling artifacts collected (local run):

- `tools/bench_results/datapath_profile.csv` — per-batch timings (timestamp, received, handle_ns, send_ns, total_ns, mode).
- `tools/bench_results/datapath_handle_events.csv` — sampled slow per-packet events (seq, payload_len, ct_len, elapsed_ns, use_avx2).

These artifacts are saved in `tools/bench_results/` for inspection and CI upload.

- `tools/bench_results/datapath_profile.csv` — per-batch timings (timestamp, received, handle_ns, send_ns, total_ns, mode).
- `tools/bench_results/datapath_handle_events.csv` — sampled slow per-packet events (ct_len, elapsed_ns).
- `tools/bench_results/datapath_alloc_events.csv` — logged arena/ciphertext reserve requests and capacities.
- `tools/bench_results/datapath_packet_timing.csv` — per-packet stage timings (seq, payload, lookup_ns, encrypt_ns, copy_ns, use_avx2) when slow.

After applying a pre-reserve patch to `Forwarder::with_session`, I re-ran the gated chaos matrix with profiling enabled. Artifacts are saved above.