# Crypto Overhead Analysis

Status: **DRAFT / PARTIAL — RUNBOOK READY**

## Goal

Validate that enabling the hybrid KEX crypto path (encrypt/decrypt on every packet) does not increase p99 latency by more than 15% compared to the baseline (no-crypto) path.

## How to run (local)

1. Build and run the `crypto_overhead_bench` Criterion benchmark and capture console output:

```bash
cargo bench -p bench --bench crypto_overhead_bench -- --noplot --sample-size 20 | tee /tmp/crypto_overhead.txt
```

2. The run contains two measurements: `baseline_no_crypto` and `worst_case_hybrid_kex`. Extract the p99 values (Criterion prints time ranges in µs). Example quick parse (may need adjustments per Criterion version):

```bash
grep -E "crypto_overhead/(baseline_no_crypto|worst_case_hybrid_kex)" -n /tmp/crypto_overhead.txt -A2
# Or extract p99 numeric with awk/python depending on local formatting
```

3. Compute the percent delta (L_b baseline, L_w worst-case):

```bash
# Example using placeholder values
L_b=100.0 # baseline p99 (µs)
L_w=110.0 # worst-case p99 (µs)
echo "scale=2; ( ($L_w - $L_b) / $L_b ) * 100" | bc -l
```

Pass condition: delta < 15

## How to run (CI)

The CI job should run the same bench and upload the console output and any generated artifacts. Example CI step (already used in repository):

```yaml
- name: Run crypto overhead bench
	run: |
		cargo bench -p bench --bench crypto_overhead_bench -- --noplot --sample-size 20 | tee tools/bench_results/crypto_overhead.txt

- name: Upload crypto overhead artifacts
	uses: actions/upload-artifact@v4
	with:
		name: crypto-overhead-results
		path: tools/bench_results/crypto_overhead.txt
```

If a flamegraph is needed for hot-path analysis, run the failure-only flamegraph step used elsewhere in CI to capture `alloc_bench`/`routing_miss_bench` style flamegraphs and extend it to `crypto_overhead_bench`.

## Artifact locations

- Console capture: `tools/bench_results/crypto_overhead.txt`
- Optional flamegraph: `/tmp/crypto_overhead_flamegraph.svg` (CI failure-only upload)

## Interpretation & Reporting

- Record baseline (`baseline_no_crypto`) and worst-case (`worst_case_hybrid_kex`) p99 values in µs.
- Compute delta as percentage. If delta >= 15%, open a performance investigation (record perf/flamegraph, validate CPU pinning, and check for unnecessary allocations).
- Add measured pairs and raw artifacts (console output, flamegraphs) to this file under a `## Measured Results` section before marking FINAL.

## Next steps

- Automate extraction into a small parser that emits a CSV row for CI artifacts (recommended).
- Add a CI gating step to fail if delta >=15% (requires a measured baseline stored in `tools/bench_results/crypto_baselines.json` and the `perf-approval` gating pattern).

## Current State

- `crypto_overhead_bench` is present in `bench/benches` and registered in `bench/Cargo.toml`.
- This document contains run instructions and artifact guidance but needs the measured values and CI extraction automation to be FINAL.

**Fill in measured results here before release.**
