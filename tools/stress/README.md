Stress harness helpers
======================

This folder provides simple orchestration scripts to run sustained traffic
against a DUT (device under test) and capture NIC counters and perf profiles.

Files
- `run_stress.sh` — run the DUT binary and a traffic-generator command, sample
  `/sys/class/net/<iface>/statistics/*` once per second and write CSV.
- `profile_stress.sh` — wrapper that runs `run_stress.sh` and records `perf`
  samples (requires sudo).

Usage examples

Local quick run (TRex or MoonGen must be available and `GEN_CMD` must be valid):

```sh
make build
DUT_BIN=./target/release/mohawk-node \
GEN_CMD="sudo trex-64r --cfg mycfg.yaml --duration 60" \
IFACE=ens1f0 \
PIN_CORES=2-5 \
./tools/stress/run_stress.sh --dut "$${DUT_BIN}" --gen "$${GEN_CMD}" --iface $${IFACE} --duration 60 --out /tmp/pconf.csv
```

Profile run (records perf for DUT process):

```sh
sudo ./tools/stress/profile_stress.sh --dut ./target/release/mohawk-node --gen "trex-64r --cfg ..." --iface ens1f0 --duration 60 --out /tmp/pconf.csv
```

Real benchmark run (records NIC counters and DUT CPU time):

```sh
make real-bench \
  DUT_BIN=./target/release/mohawk-node \
  GEN_CMD="sudo trex-64r --cfg ... --duration 60" \
  IFACE=ens1f0 \
  DURATION=60 \
  OUT=/tmp/stress_pconf.csv
```

Notes & limitations
- These scripts can sample NIC counters and, when `--metrics-http` or
  `--metrics-socket` is enabled on the DUT, they can also capture the
  application-level `packets_processed` counter.
- For bridge-request runs, `MOHAWK_WORKER_CORES=0-3` pins `cli` worker threads to specific cores when `num_workers > 1`.
- Use a dedicated test host for high-rate tests; disable C-states and frequency
  scaling and pin IRQs/cores for deterministic results.
