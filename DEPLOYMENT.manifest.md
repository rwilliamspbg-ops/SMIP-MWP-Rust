# Deployment Performance Manifest

This manifest captures performance-risk flags that must be reviewed before production rollout.

## Dataplane Determinism Flags

- [ ] Forwarding path is isolated from Go control-plane calls during packet forwarding.
- [ ] No blocking lock contention on forwarding hot path under load.
- [ ] Worker CPU pinning and cgroup/cpuset isolation are enforced.
- [ ] Hugepages configured and verified on target host.
- [ ] p99 variance remains within accepted envelope under hostile traffic.

## Mandatory Risk Flag

If forwarding relies on Go control-plane interaction at runtime, mark release **AT RISK** and document expected context-switching/locking overhead with mitigation plan.

### Current Status: AT RISK

- Reason: chaos benchmark SLA failure observed during CI-style run: measured `p99_ns=56235` exceeds service-level `p99_ns=46000`.
- Action: Regenerate `benchmark/chaos_report.md` on target hardware for each release candidate and attach artifacts to the release PR. Investigate control-plane interaction on forwarding hot path and isolate or async the control calls.

See `benchmark/chaos_report.md` and CI artifacts for details.

## Validation Links

- Throughput: `benchmark/report_throughput.md`
- Latency artifact: `benchmark/report_latency.png`
- Mpps report: `benchmark/report_mpps.txt`
- Crypto overhead: `benchmark/crypto_overhead.md`
- Chaos report: `benchmark/chaos_report.md`
