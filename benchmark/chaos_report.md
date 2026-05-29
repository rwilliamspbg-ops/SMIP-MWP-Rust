# Chaos Engineering Report

Status: **BLOCKED (missing ideal-mode baseline metrics)**

## Objective

Validate safety-invariant resilience under hostile traffic while keeping performance overhead bounded.

- Throughput degradation target: < 5.00% vs ideal mode
- p99 increase target: < 1000.00 ns vs ideal mode

## Input Artifact

- Source CSV: `tools/bench_results/chaos_epyc_profile.csv`
- Latest sampled row timestamp: `2026-05-28T22:34:02Z`
- Core set: `0-1`

## Latest Chaos Metrics

- throughput_pkt_s: `1429348.28`
- latency_ns p50: `34094`
- latency_ns p99: `95860`
- latency_ns p99_9: `119103`

## Baseline Comparison

- Baseline throughput_pkt_s: `NOT PROVIDED`
- Baseline p99_ns: `NOT PROVIDED`
- Throughput degradation: `N/A`
- p99 increase: `N/A`

## Invariant Notes

- Byzantine fault injection includes packet drop, corruption/truncation, and duplication.
- Report must be re-generated for each release candidate on target hardware.
- If forwarding interacts with Go control-plane in fast path, mark `DEPLOYMENT.manifest.md` as **AT RISK**.

## Reproduction

```bash
make chaos-epyc-profile
python3 tools/benchmark/generate_chaos_report.py   --input tools/bench_results/chaos_epyc_profile.csv   --output benchmark/chaos_report.md
```
