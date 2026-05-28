## Makefile - stress and profiling helpers

.PHONY: build stress-test real-bench profile setup-hardware verify-bridge chaos-epyc-profile chaos-report report-latency performance-envelope clean

build:
	cargo build --release

## Run a stress test. Expects env vars: DUT_BIN, GEN_CMD, IFACE, RATE, DURATION, OUT
stress-test:
	@echo "Run: ./tools/stress/run_stress.sh --dut $$DUT_BIN --gen '$$GEN_CMD' --iface $$IFACE --rate $$RATE --duration $$DURATION --out $$OUT"

	@echo "Example with metrics socket:"
	@echo "  METRICS_SOCKET=/tmp/mohawk.metrics.sock DUT_BIN=./target/release/mohawk-node GEN_CMD=\"trex...\" IFACE=ens1f0 ./tools/stress/run_stress.sh --dut \"$$DUT_BIN\" --gen \"$$GEN_CMD\" --iface $$IFACE --duration $$DURATION --out $$OUT"

profile: build
	@echo "Run: sudo ./tools/stress/profile_stress.sh --dut $$DUT_BIN --gen '$$GEN_CMD' --iface $$IFACE --rate $$RATE --duration $$DURATION --out $$OUT"

## Setup hardware-oriented tuning knobs for reproducible local benchmarking.
## Expects optional env vars: HUGE_PAGES (default 1024), PIN_CORES (default 2-3), DRY_RUN (0|1)
setup-hardware:
	./tools/hardware/setup_hardware.sh

## Validate bridge contract and cross-language compatibility checks.
verify-bridge:
	./tools/validation/verify_bridge.sh

## Run EPYC-oriented chaos benchmark matrix and export CSV.
chaos-epyc-profile:
	./tools/benchmark/run_chaos_epyc_profile.sh

## Generate mandatory chaos engineering report from latest profile CSV.
chaos-report:
	python3 tools/benchmark/generate_chaos_report.py \
	  --input tools/bench_results/chaos_epyc_profile.csv \
	  --output benchmark/chaos_report.md

## Generate p99 latency visualization artifact (PNG) from latest profile CSV.
report-latency:
	python3 tools/benchmark/generate_latency_plot.py \
	  --input tools/bench_results/chaos_epyc_profile.csv \
	  --output benchmark/report_latency.png

## Build all envelope artifacts required before any final performance claim.
performance-envelope: chaos-epyc-profile report-latency chaos-report
	@echo "Generated: benchmark/report_throughput.md benchmark/report_latency.png benchmark/report_mpps.txt benchmark/crypto_overhead.md benchmark/chaos_report.md"

## Run a real hardware-backed benchmark using the stress harness.
## Expects env vars: DUT_BIN, GEN_CMD, IFACE, DURATION, OUT
real-bench: build
	./tools/stress/run_stress.sh --dut "$$DUT_BIN" --gen "$$GEN_CMD" --iface "$$IFACE" --duration "$$DURATION" --out "$$OUT"

clean:
	cargo clean
