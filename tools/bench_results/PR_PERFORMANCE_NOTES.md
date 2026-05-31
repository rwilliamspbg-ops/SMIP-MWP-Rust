PR: Fine tuning Performance (feature/afxdp-control-metrics-pr)

Summary
- Eliminated per-packet Vec allocations in the datapath MCR paths.
- Inlined encryption into `self.arena` for serial MCR processing.
- Added a consuming parallel-path processor that performs in-place encryption
  and returns owned buffers to the main thread for append (avoids allocations
  inside `par_iter`).

Files changed
- `datapath/src/lib.rs` (inline encryption, `process_packet_owned_consuming`, serial + parallel MCR changes)
- `tools/bench_results/*` (aggregation, reports, hotspots, run logs)
- `tools/benchmark/run_flamegraphs.sh`, `benchmark/FLAMEGRAPH_RUN.md` (profiling helpers)

Key results (local, pinned runs)
- 5 pinned smoke runs (taskset cores 0-3) — throughput per run (pkt/s):
  - 2,357,031.32
  - 2,111,419.61
  - 2,757,422.64
  - 2,402,041.74
  - 2,443,795.75
  - Mean ≈ 2,414,342 pkt/s
- Forwarder `encrypt` average observed across runs: ~118–174 ns per call (mean ≈ 133 ns)

Notes and rationale
- The biggest low-hanging overhead was per-packet heap allocations and copies
  in the MCR path. By encrypting into pre-reserved aligned `arena` memory and
  avoiding ephemeral `Vec` allocations inside parallel maps, we reduce pressure
  on the allocator and lower scheduling jitter.
- These changes are low-risk: we preserve semantics (route-miss handling,
  encryption error handling) and keep the existing public API surface.

Reproduction
1. Build and run the smoke harness pinned to cores 0-3:

```bash
taskset -c 0-3 cargo run --release -p benchmark -- --packets 2000 --payload-len 64 --seed 201 --mcr-channels 3 --mcr-spray-mode primary | tee mcr_smoke_run.txt
```

2. Re-generate aggregation and hotspots:

```bash
python3 tools/bench_results/aggregate_results.py
python3 tools/bench_results/parse_forwarder_profiles.py
```

Next steps
- Update CI baselines with new stable medians (I can prepare a follow-up commit).
- Capture flamegraphs on a self-hosted bench host (`benchmark/FLAMEGRAPH_RUN.md`).
- Consider micro-optimizations in `encrypt_into_slice` usage if flamegraphs show hotspots inside crypto kernel.

Artifacts
- See `tools/bench_results/` for run logs and generated reports (`bench_report.md`, `forwarder_hotspots.md`).

If you want, I will (pick one):
- A) update CI baseline files and open a follow-up PR, or
- B) run flamegraph capture on a bench host and analyze stacks, or
- C) prepare a short PR description and reviewer checklist for this change.
