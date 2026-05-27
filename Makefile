## Makefile - stress and profiling helpers

.PHONY: build stress-test profile clean

build:
	cargo build --release

## Run a stress test. Expects env vars: DUT_BIN, GEN_CMD, IFACE, RATE, DURATION, OUT
stress-test:
	@echo "Run: ./tools/stress/run_stress.sh --dut $$DUT_BIN --gen '$$GEN_CMD' --iface $$IFACE --rate $$RATE --duration $$DURATION --out $$OUT"

profile: build
	@echo "Run: sudo ./tools/stress/profile_stress.sh --dut $$DUT_BIN --gen '$$GEN_CMD' --iface $$IFACE --rate $$RATE --duration $$DURATION --out $$OUT"

clean:
	cargo clean
