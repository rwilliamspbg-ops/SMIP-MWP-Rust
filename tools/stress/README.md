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

Notes & limitations
- These scripts sample NIC counters not internal application `pconf`. For accurate
  pconf you should expose an application metric endpoint or write a small
  instrumentation hook in the datapath to emit counts to stdout or a socket.
- Use a dedicated test host for high-rate tests; disable C-states and frequency
  scaling and pin IRQs/cores for deterministic results.
