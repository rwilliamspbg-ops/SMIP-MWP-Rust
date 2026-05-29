Hardware smoke test notes
=========================

This folder contains helpers to build and (optionally) run a small AF_XDP
smoke test on real NIC hardware. These tools are intentionally conservative:
- default to dry-run
- require explicit `RUN_REAL_SMOKE=1` opt-in to execute

Quick steps (dry-run)
---------------------

```sh
# shows what would be executed; safe by default
bash tools/hardware/run_smoke_with_traffic.sh --dry-run
```

Run the smoke binary (manual, safe)
----------------------------------

1. Build the smoke binary:

   ```sh
   cargo build --manifest-path tools/hardware/smoke/Cargo.toml --release
   ```

2. Run the smoke binary (requires root and appropriate NIC support):

   ```sh
   MOHAWK_IFACE=ens1f0 RUN_REAL_SMOKE=1 \ 
     ./tools/hardware/run_smoke_with_traffic.sh --run
   ```

Optional traffic generator
--------------------------

Set `SMOKE_GEN_CMD` to a command that generates traffic for the test duration.
Examples (pick the tool you have available):

- `tcpreplay` (replay a pcap):

  ```sh
  SMOKE_GEN_CMD='tcpreplay --intf1=ens1f0 -l 0 sample.pcap' SMOKE_GEN_DURATION=30 \
    MOHAWK_IFACE=ens1f0 RUN_REAL_SMOKE=1 ./tools/hardware/run_smoke_with_traffic.sh --run
  ```

- `hping3` (send crafted UDP packets):

  ```sh
  SMOKE_GEN_CMD='hping3 -i u1000 -d 1400 --udp -c 1000000 192.0.2.1' SMOKE_GEN_DURATION=30 \
    MOHAWK_IFACE=ens1f0 RUN_REAL_SMOKE=1 ./tools/hardware/run_smoke_with_traffic.sh --run
  ```

Notes and safety
----------------
- Running the smoke test requires a NIC with AF_XDP support and usually
  elevated privileges. Do not run on shared CI runners.
- The scripts log to `tools/hardware/smoke/logs/` and will not modify system
  network configuration.
- If you need help wiring an automated traffic generator for your hardware,
  tell me the generator you prefer and I can add a vetted example command.
