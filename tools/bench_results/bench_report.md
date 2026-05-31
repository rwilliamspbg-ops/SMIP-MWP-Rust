# Bench Summary

| Source | Metric | Value |
|---|---:|---:|
| mcr_smoke.txt | throughput_pkt_s | 3125610.47 |
| mcr_smoke.txt | p50_ns | 14938 |
| mcr_smoke.txt | p99_ns | 21490 |
| mcr_smoke.txt | p99_9_ns | 21490 |
| mcr_larger_smoke.txt | throughput_pkt_s | 1487866.52 |
| mcr_larger_smoke.txt | p50_ns | 34093 |
| mcr_larger_smoke.txt | p99_ns | 73226 |
| mcr_larger_smoke.txt | p99_9_ns | 89006 |
| alloc_bench.txt | throughput | 15.978 |
| alloc_bench.txt | unit | GiB/s |
| crypto_overhead_bench.txt | time_ns | 28.951 |
| datapath_bench.txt | throughput | 3.2546 |
| datapath_bench.txt | unit | Melem/s |

## MCR baseline check
- baseline: 800000.0
- measured: 1487866.52
- min allowed (80%): 640000.0
- status: **PASS**

## Crypto baseline comparison
| metric | baseline | current | delta % | status |
|---|---:|---:|---:|---:|
| symmetric_encrypt_in_place | 145.240791 | 142.810000 | -1.67% | OK |
| symmetric_decrypt_in_place | 192.050100 | 188.080000 | -2.07% | OK |
| worst_case_hybrid_kex | 398207.061344 | 387360.000000 | -2.72% | OK |