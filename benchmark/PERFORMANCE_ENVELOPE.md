# Performance Envelope Report
**Throughput CSV not found:** tools/bench_results/throughput_from_ethtool.csv

## Current Local Session Smoke

- Synthetic smoke benchmark: `throughput_pkt_s=2284453.27`, `p50=20789 ns`, `p99=42710 ns`, `p99_9=42710 ns`
- Synthetic bench micro-run: `1024 -> 66297.52 MiB/s`, `8192 -> 98085.37 MiB/s`, `65536 -> 80954.35 MiB/s`

## Chaos EPYC Profile
(timestamp, core_set, packets, payload_len, loss, corrupt, duplicate, throughput_pkt_s, p50, p99, p99_9)

timestamp,core_set,packets,payload_len,loss_percent,corrupt_percent,duplicate_percent,throughput_pkt_s,p50_ns,p99_ns,p99_9_ns
2026-05-29T12:49:47Z,"2-3",50000,1024,3,1,1,1074884.04,49913,81712,94235
2026-05-29T12:49:47Z,"2-5",50000,1024,3,1,1,1109765.37,49783,74659,103864
2026-05-29T12:49:47Z,"2-7",50000,1024,3,1,1,1101614.50,49803,73878,85831

**Routing miss sweep CSV not found:** tools/bench_results/routing_miss_sweep.csv

**Crypto overhead console not found:** tools/bench_results/crypto_overhead.txt
