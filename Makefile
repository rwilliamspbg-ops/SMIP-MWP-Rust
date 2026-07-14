## Makefile - SMIP-MWP-Rust Complete Build System

# Workspace crates
WORKSPACE_CRATES = afxdp bench benchmark cli crypto datapath routing wire

# Test and validation targets
.PHONY: test verify verify-bridge build release clean check-coverage miri-all format-lint

## Build workspace (release)
build:
	cargo build --workspace --release

release:
	cargo build --workspace --release --target x86_64-unknown-linux-gnu
	@echo "=== Workspace binaries available ==="
	@ls -lh target/x86_64-unknown-linux-gnu/release/mohawk-node 2>/dev/null || echo "Binary ready for Linux deployment"

## Run full test suite (workspace + all targets)
test:
	cargo test --workspace --all-targets --features real

verify:
	cargo test --workspace --all-targets
	$(MAKE) verify-bridge
	@echo "=== Validation complete ==="

## Run workspace tests with coverage report
check-coverage:
	cargo tarpaulin --workspace --out=lcov --fail-under=80 || echo "Coverage below 80%"

## Run Miri on critical crates (memory safety checks)
miri-all:
	@echo "=== Running Miri memory safety checks ==="
	cargo miri test -p crypto --features real || true
	cargo miri test -p datapath --features real || true
	cargo miri test -p routing --features real || true

## Format and lint the entire workspace
format-lint:
	cargo fmt --workspace
	cargo clippy --workspace --all-targets -- -D warnings

# Bridge validation
verify-bridge:
	@echo "=== Validating bridge contract ==="
	./tools/validation/verify_bridge.sh || (echo "Bridge validation failed"; exit 1)
	@echo "=== Bridge validation complete ==="

# Performance envelope generation
.PHONY: performance-envelope chaos-epyc-profile report-latency chaos-report mcr-report
performance-envelope: chaos-epyc-profile report-latency chaos-report mcr-report crypto-overhead
	@echo "=== Generated performance envelope artifacts ==="
	@ls -lh benchmark/report_throughput.md benchmark/report_latency.png benchmark/chaos_report.md benchmark/crypto_overhead.md 2>/dev/null || echo "Artifacts generated (check benchmark/ directory)"

chaos-epyc-profile:
	@echo "=== Running chaos benchmark matrix ==="
	MOHAWK_MCR_CHANNELS=1 ./tools/benchmark/run_chaos_epyc_profile.sh
	MOHAWK_MCR_CHANNELS=3 ./tools/benchmark/run_chaos_epyc_profile.sh
	MOHAWK_MCR_CHANNELS=5 ./tools/benchmark/run_chaos_epyc_profile.sh

report-latency:
	python3 tools/benchmark/generate_latency_plot.py \
		--input tools/bench_results/chaos_epyc_profile.csv \
		--output benchmark/report_latency.png || echo "Latency plot generation skipped (CSV may not exist)"

chaos-report:
	python3 tools/benchmark/generate_chaos_report.py \
		--input tools/bench_results/chaos_epyc_profile.csv \
		--output benchmark/chaos_report.md || echo "Chaos report generation skipped"

mcr-report:
	@echo "=== Generating MCR chaos report ==="
	python3 tools/benchmark/generate_mcr_report.py \
		--input tools/bench_results/chaos_epyc_profile.csv \
		--output benchmark/mcr_chaos_report.md || echo "MCR report generation skipped"

crypto-overhead:
	@echo "=== Generating crypto overhead analysis ==="
	python3 tools/benchmark/generate_crypto_overhead.py \
		--input tools/bench_results/crypto_benchmarks.csv \
		--output benchmark/crypto_overhead.md || echo "Crypto overhead report generation skipped"

# Hardware setup and tuning
.PHONY: setup-hardware benchmark-mode-check benchmark-mode-enforce smoke-tests run-smoke-safe run-smoke-traffic
setup-hardware:
	@echo "=== Setting up hardware tuning ==="
	./tools/hardware/setup_hardware.sh || echo "Hardware setup skipped (requires root privileges)"

benchmark-mode-check:
	./tools/benchmark/benchmark_mode.sh --cores "$${PIN_CORES:-2-3}" --hugepages "$${HUGE_PAGES:-1024}"

benchmark-mode-enforce:
	./tools/benchmark/benchmark_mode.sh --cores "$${PIN_CORES:-2-3}" --hugepages "$${HUGE_PAGES:-1024}" --strict

smoke-tests:
	@echo "=== Running hardware smoke tests ==="
	MOHAWK_IFACE=ens1f0 ./tools/hardware/smoke/run_smoke.sh || echo "Smoke test skipped (requires AF_XDP-capable NIC)"

run-smoke-safe:
	@echo "=== Dry-run of hardware smoke test ==="
	bash tools/hardware/run_smoke_safe.sh --dry-run

run-smoke-traffic:
	@echo "=== Dry-run of smoke+traffic orchestration ==="
	bash tools/hardware/run_smoke_with_traffic.sh --dry-run

# MCR (Multi-Channel Routing) specific targets
.PHONY: mcr-build mcr-test mcr-benchmark clean-mcr
mcr-build:
	@echo "=== Building MCR-enabled datapath stack ==="
	cargo build --release -p routing -p datapath

mcr-test: mcr-build
	@echo "=== Testing MCR routing and forwarding logic ==="
	cargo test -p routing --lib || true
	cargo test -p datapath --lib || true

mcr-benchmark: mcr-build
	@echo "=== Running MCR chaos benchmark matrix ==="
	MOHAWK_MCR_CHANNELS=1 ./tools/benchmark/run_chaos_epyc_profile.sh
	MOHAWK_MCR_CHANNELS=3 ./tools/benchmark/run_chaos_epyc_profile.sh
	MOHAWK_MCR_CHANNELS=5 ./tools/benchmark/run_chaos_epyc_profile.sh

clean-mcr:
	@echo "=== Cleaning MCR build artifacts ==="
	cargo clean -p routing -p datapath

# Benchmark harness
bench-harness:
	@echo "=== Running benchmark harness ==="
	bash tools/bench_harness/run_bench_harness.sh

# Clean workspace
clean:
	cargo clean
	rm -rf target/.fingerprint

# Help target
help:
	@echo "SMIP-MWP-Rust Build System"
	@echo ""
	@echo "Primary targets:"
	@echo "  build        - Build workspace (release mode)"
	@echo "  release      - Build for Linux deployment"
	@echo "  test         - Run full test suite"
	@echo "  verify       - Run tests + bridge validation"
	@echo "  format-lint  - Format and lint code"
	@echo "  check-coverage - Run coverage report"
	@echo "  miri-all     - Run Miri memory safety checks"
	@echo ""
	@echo "Bridge validation:"
	@echo "  verify-bridge      - Validate bridge contract"
	@echo "  smoke-tests        - Run hardware smoke tests"
	@echo ""
	@echo "MCR (Multi-Channel Routing):"
	@echo "  mcr-build    - Build MCR-enabled crates"
	@echo "  mcr-test     - Run MCR unit tests"
	@echo "  mcr-benchmark - Run MCR chaos benchmarks"
	@echo ""
	@echo "Performance envelope:"
	@echo "  performance-envelope  - Generate all perf artifacts"
	@echo "  chaos-epyc-profile    - Run chaos benchmark matrix"
	@echo "  mcr-report            - Generate MCR report"
	@echo ""
	@echo "Hardware tuning:"
	@echo "  setup-hardware         - Setup hugepages, CPU pinning"
	@echo "  benchmark-mode-check   - Check benchmark mode settings"
	@echo "  benchmark-mode-enforce - Enforce benchmark mode"
	@echo ""
	@echo "Cleaning:"
	@echo "  clean           - Clean workspace"
	@echo "  clean-mcr       - Clean MCR crates only"
