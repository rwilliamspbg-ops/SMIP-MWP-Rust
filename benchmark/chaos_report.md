# Chaos Engineering Report

Status: **FAIL**

## Objective

Validate safety-invariant resilience under hostile traffic while keeping performance overhead bounded.

- Throughput degradation target: < 5.00% vs ideal mode
- p99 increase target: < 1000.00 ns vs ideal mode

## Input Artifact

- Source CSV: `tools/bench_results/chaos_epyc_profile.csv`
- Latest sampled row timestamp: `2026-05-28T22:22:51Z`
- Core set: `0-1`

## Latest Chaos Metrics

- throughput_pkt_s: `1338660.33`
- latency_ns p50: `34063`
- latency_ns p99: `122338`
- latency_ns p99_9: `184164`

## Baseline Comparison

- Baseline throughput_pkt_s: `1378064.38`
- Baseline p99_ns: `77214.0`
- Throughput degradation: `2.86% (goal < 5.00%)`
- p99 increase: `45124.00 ns (goal < 1000.00 ns)`

## Invariant Notes

- Byzantine fault injection includes packet drop, corruption/truncation, and duplication.
- Report must be re-generated for each release candidate on target hardware.
- If forwarding interacts with Go control-plane in fast path, mark `DEPLOYMENT.manifest.md` as **AT RISK**.

## Reproduction

```bash
make chaos-epyc-profile
python3 tools/benchmark/generate_chaos_report.py   --input tools/bench_results/chaos_epyc_profile.csv   --output benchmark/chaos_report.md
```
