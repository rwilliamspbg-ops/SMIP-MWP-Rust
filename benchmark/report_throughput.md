# Throughput Validation Report

Status: **DRAFT / NOT FINAL**

## Objective

Validate ideal-mode throughput against target:

- Goal: >95% of link capacity
- Profile: sustained, lossless, zero-copy path

## Method

1. Configure hardware baseline (`make setup-hardware`).
2. Run pinned chaos profile matrix for comparable dataplane statistics:
   - `PACKETS=12000 CORE_SETS='2-3 2-5' make chaos-epyc-profile`
3. For line-rate validation, run real traffic generation against DUT (`make real-bench`) and collect NIC counters.

## Latest Local Snapshot (from `tools/bench_results/chaos_epyc_profile.csv`)

- Core set `2-3`: `throughput_pkt_s=792003.11`
- Core set `2-5`: `throughput_pkt_s=932108.75`

These are packet-rate micro-harness numbers and are **not** direct line-rate Gbps proof.

## Required Evidence to Mark FINAL

- NIC/link speed and queue config
- Sustained run duration and zero-loss verification
- Measured Gbps and % of link-capacity
- Host topology/pinning notes
