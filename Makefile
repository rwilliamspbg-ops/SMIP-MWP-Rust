## Makefile - stress and profiling helpers

.PHONY: build stress-test real-bench profile setup-hardware benchmark-mode-check benchmark-mode-enforce verify verify-bridge chaos-epyc-profile chaos-report report-latency performance-envelope clean

build:
	cargo build --release

## Run the workspace validation gate after benchmarks or other perf-sensitive runs.
verify:
	cargo test --workspace --all-targets
	$(MAKE) verify-bridge

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

## Print or enforce the benchmark-mode CPU pinning and hugepages checklist.
benchmark-mode-check:
	./tools/benchmark/benchmark_mode.sh --cores "$${PIN_CORES:-2-3}" --hugepages "$${HUGE_PAGES:-1024}"

## Enforce the benchmark-mode CPU pinning and hugepages checklist.
benchmark-mode-enforce:
	./tools/benchmark/benchmark_mode.sh --cores "$${PIN_CORES:-2-3}" --hugepages "$${HUGE_PAGES:-1024}" --strict

## Validate bridge contract and cross-language compatibility checks.
verify-bridge:
	./tools/validation/verify_bridge.sh

.PHONY: verify-bridge-smoke
verify-bridge-smoke:
	@echo "Building hardware smoke test (does not run it). Set MOHAWK_IFACE and run manually on a host with the NIC."
	@cargo build --manifest-path tools/hardware/smoke/Cargo.toml --release || true

.PHONY: bench-harness
bench-harness:
	@echo "Run benchmark harness script"
	@bash tools/bench_harness/run_bench_harness.sh

.PHONY: verify-bridge-run-smoke
verify-bridge-run-smoke:
	@if [ -z "$$MOHAWK_IFACE" ]; then \
		echo "MOHAWK_IFACE is required to run smoke test"; exit 2; \
	fi
	@echo "Running hardware smoke test...";
	@sh tools/hardware/smoke/run_smoke.sh

.PHONY: run-smoke-safe
run-smoke-safe:
	@echo "Dry-run of hardware smoke test (no NIC actions)"
	@bash tools/hardware/run_smoke_safe.sh --dry-run

.PHONY: run-smoke-traffic
run-smoke-traffic:
	@echo "Dry-run of smoke+traffic orchestration"
	@bash tools/hardware/run_smoke_with_traffic.sh --dry-run

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
	@echo "Running AF_XDP hardware smoke test"
	./tools/benchmark/real_smoke.sh
	./tools/stress/run_stress.sh --dut "$$DUT_BIN" --gen "$$GEN_CMD" --iface "$$IFACE" --duration "$$DURATION" --out "$$OUT"

clean:
	cargo clean

.PHONY: mcr-build mcr-test mcr-benchmark mcr-report clean-mcr

mcr-build:
	@echo "Building MCR-enabled datapath stack"
	@cargo build --release -p routing -p datapath

mcr-test: mcr-build
	@echo "Testing MCR routing and forwarding logic"
	@cargo test -p routing --lib || true
	@cargo test -p datapath --lib || true

mcr-benchmark: mcr-build
	@echo "Running MCR chaos benchmark matrix"
	@MOHAWK_MCR_CHANNELS=1 ./tools/benchmark/run_chaos_epyc_profile.sh
	@MOHAWK_MCR_CHANNELS=3 ./tools/benchmark/run_chaos_epyc_profile.sh
	@MOHAWK_MCR_CHANNELS=5 ./tools/benchmark/run_chaos_epyc_profile.sh

mcr-report: mcr-benchmark
	@python3 tools/benchmark/generate_mcr_report.py \
		--input tools/bench_results/chaos_epyc_profile.csv \
		--output benchmark/mcr_chaos_report.md
	@echo "Generated: benchmark/mcr_chaos_report.md"

clean-mcr:
	@cargo clean -p routing -p datapath
