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

## Runbook — How to produce verifiable throughput results

1. Prepare DUT and traffic generator. Ensure NIC settings (MTU, RSS/queues, XDP modes) are consistent and recorded.

2. Collect baseline NIC counters and link speed on the DUT interface `IFACE`:

```bash
IFACE=ens1f0
# link speed
ethtool $IFACE | grep -i speed
# counters snapshot
ethtool -S $IFACE > /tmp/nic_before.json
date -u +%s > /tmp/time_before.txt
```

3. Run the real line-rate benchmark (hardware smoke) for a sustained interval (e.g., 60s):

```bash
MOHAWK_IFACE=$IFACE MOHAWK_QUEUE_ID=0 ./tools/benchmark/real_smoke.sh | tee /tmp/real_smoke.txt
```

4. Capture post-run counters and compute deltas:

```bash
ethtool -S $IFACE > /tmp/nic_after.json
date -u +%s > /tmp/time_after.txt
SECONDS=$(( $(cat /tmp/time_after.txt) - $(cat /tmp/time_before.txt) ))
# use a small python script or jq to compute byte deltas between before/after snapshots
```

5. Convert bytes delta to Gbps:

```bash
# bytes_delta is the sum of TX bytes across relevant queues
# Gbps = (bytes_delta * 8) / (SECONDS * 1e9)
```

6. Zero-loss verification: confirm RX/TX error counters and dropped counters did not increase unexpectedly (compare `ethtool -S` fields such as `rx_errors`, `tx_errors`, `rx_dropped`).

7. Record environment metadata (core pinning, NUMA, kernel version, NIC driver) and attach `real_smoke.txt`, `nic_before.json`, `nic_after.json` to the artifact bundle.

## CI steps (example)

Add the following to the self-hosted bench job (or reuse `chaos-validation`) to produce artifacts:

```yaml
- name: Run real-bench and collect NIC counters
   run: |
      IFACE=ens1f0
      date -u +%s > /tmp/time_before.txt
      ethtool -S $IFACE > tools/bench_results/nic_before.json
      MOHAWK_IFACE=$IFACE MOHAWK_QUEUE_ID=0 ./tools/benchmark/real_smoke.sh | tee tools/bench_results/real_smoke.txt
      ethtool -S $IFACE > tools/bench_results/nic_after.json
      date -u +%s > /tmp/time_after.txt

- name: Upload throughput artifacts
   uses: actions/upload-artifact@v4
   with:
      name: throughput-artifacts
      path: |
         tools/bench_results/real_smoke.txt
         tools/bench_results/nic_before.json
         tools/bench_results/nic_after.json
         tools/bench_results/chaos_epyc_profile.csv
```

## Interpreting results

- Prefer NIC byte counters to packet-rate conversions unless only packet-rate is available from the harness.
- Convert packet-rate to Gbps cautiously — account for all headers (Ethernet, IP, UDP, our application header). For a precise conversion use NIC byte deltas.
- Record both p99 latency and sustained Gbps in the final report and compare to the target (>95% link capacity).

## Artifact locations

- `tools/bench_results/real_smoke.txt` — console output from real-bench
- `tools/bench_results/nic_before.json` — NIC counters before run
- `tools/bench_results/nic_after.json` — NIC counters after run
- `tools/bench_results/chaos_epyc_profile.csv` — pinned chaos profile matrix

## Next actions to finalize

- Automate NIC counter delta computation into a small script `tools/benchmark/compute_throughput_from_ethtool.py` and upload its CSV output to `tools/bench_results/report_throughput.csv`.
- When available, embed measured NIC hardware counters and Gbps results in this document under `## Measured Results` and mark FINAL.

