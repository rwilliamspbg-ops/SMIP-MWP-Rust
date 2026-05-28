# Final Performance Expectation Envelope: SMIP-MWP-Rust

This document defines the validation contract for final dataplane performance claims.

## 1) Target Hardware Baseline Assumption

Deployment target is a modern high core-count server platform (for example, 2nd/3rd Gen AMD EPYC or Intel Sapphire Rapids), with:

- Linux
- Hugepages enabled
- CPU pinning enforced via cgroup/cpuset mechanisms
- Isolated benchmark cores for reproducibility

## 2) Performance Matrix by Operational Mode

| Metric | Goal value range | Measurement context/profile | Limiting factor | Validation artifact |
|---|---|---|---|---|
| Max Throughput (Ideal) | >95% of link capacity (for example >95 Gbps on 100G) | Sustained lossless flow test, zero-copy path only | NIC capability / kernel-bypass limits | `benchmark/report_throughput.md` |
| Latency (p99) | <2 µs | End-to-end packet path: ingress → process → egress | Cache misses / context switching | `benchmark/report_latency.png` |
| Packet Processing Rate | >50 Mpps | Minimal payload (header-only) sustained mode | Core clock and fast-path instruction efficiency | `benchmark/report_mpps.txt` |
| Crypto Overhead (worst case) | <15% p99-latency increase vs baseline | Hybrid KEX encrypt/decrypt on every packet header | Crypto implementation and HW acceleration | `benchmark/crypto_overhead.md` |
| Fault Tolerance Overhead | <5% throughput degradation and p99 increase <1 µs vs ideal mode | Sustained load under Byzantine faults (drop/malformed/divergence attempts) | Consensus/reconciliation complexity on fast path | `benchmark/chaos_report.md` |

## 3) Contract Interpretation

- Final performance is a **validated contract**, not a marketing claim.
- p99 latency and variance stability are first-class release gates.
- Any datapath dependency on Go control-plane interaction during forwarding must be flagged in deployment readiness artifacts.

## 4) Mandatory Next Step

Before any final performance figure is claimed, complete and review `benchmark/chaos_report.md` from reproducible hostile-network runs.
