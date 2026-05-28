## Makefile - stress and profiling helpers

.PHONY: build stress-test real-bench profile clean

build:
	cargo build --release

## Run a stress test. Expects env vars: DUT_BIN, GEN_CMD, IFACE, RATE, DURATION, OUT
stress-test:
	@echo "Run: ./tools/stress/run_stress.sh --dut $$DUT_BIN --gen '$$GEN_CMD' --iface $$IFACE --rate $$RATE --duration $$DURATION --out $$OUT"

	@echo "Example with metrics socket:"
	@echo "  METRICS_SOCKET=/tmp/mohawk.metrics.sock DUT_BIN=./target/release/mohawk-node GEN_CMD=\"trex...\" IFACE=ens1f0 ./tools/stress/run_stress.sh --dut \"$$DUT_BIN\" --gen \"$$GEN_CMD\" --iface $$IFACE --duration $$DURATION --out $$OUT"

profile: build
	@echo "Run: sudo ./tools/stress/profile_stress.sh --dut $$DUT_BIN --gen '$$GEN_CMD' --iface $$IFACE --rate $$RATE --duration $$DURATION --out $$OUT"

## Run a real hardware-backed benchmark using the stress harness.
## Expects env vars: DUT_BIN, GEN_CMD, IFACE, DURATION, OUT
real-bench: build
	./tools/stress/run_stress.sh --dut "$$DUT_BIN" --gen "$$GEN_CMD" --iface "$$IFACE" --duration "$$DURATION" --out "$$OUT"

clean:
	cargo clean
