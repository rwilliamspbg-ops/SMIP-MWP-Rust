.PHONY: benchmark-report verify

benchmark-report:
	cargo flamegraph -p bench --bench datapath_bench --output flamegraph.svg -- --sample-size 10

verify:
	cargo test -p datapath --all-targets