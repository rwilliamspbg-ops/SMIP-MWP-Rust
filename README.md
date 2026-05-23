# SMIP-MWP (Rust rewrite)

[![CI](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/ci.yml/badge.svg)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions)
[![License](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)
<a href="https://github.com/sponsors/rwilliamspbg-ops"><img src="https://img.shields.io/badge/Sponsor-❤-1EAEDB?style=social&logo=github" alt="Sponsor"></a>
[![Contributing](https://img.shields.io/badge/contributing-guidelines-brightgreen.svg)](CONTRIBUTING.md)

This repository contains a Rust rewrite of the SMIP-MWP project components: datapath, crypto, routing and CLI. The goal is a safe, testable, and high-performance implementation suitable for benchmarking and iteration.

Repository layout (high level)

- `crypto/` — key-exchange, session derivation, AEAD helpers
- `datapath/` — forwarding hot path and tests
- `afxdp/` — AF_XDP integration and mocks
- `routing/` — route table and prediction helpers
- `bench/` — Criterion microbench harness and smoke-run utilities
- `cli/` — binary entrypoint and demo flags

Quick start

```sh
git clone https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust.git
cd SMIP-MWP-Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustup update
cargo test --workspace --all-targets
cargo run --release -p bench
```

Bench harness (automated)

The repository includes an automated bench harness that runs multiple fill/processing
strategies (scalar, tiled with several tile sizes, and an AVX2-accelerated tiled
variant) and emits CSV results for reproducible comparison.

Files:
- `tools/bench_harness/run_bench_harness.sh` — driver script that runs each
	strategy multiple times and appends results to a CSV.
- `tools/bench_harness/parse_and_append.py` — parses bench output and appends
	rows to the CSV with columns: `timestamp,commit,run_index,strategy,size,avg_ns,throughput_mib_s`.

Quick usage (local)

1. Build the bench binary (if you have cargo):

```sh
cargo build -p bench --release
```

2. Run the harness (example: 20 iterations):

```sh
./tools/bench_harness/run_bench_harness.sh 20 bench_results.csv
```

3. The harness writes `bench_results.csv` which you can import into spreadsheets
	 or analysis tooling for comparison.

CI integration

There is a GitHub Actions workflow at `.github/workflows/bench-harness.yml`
that builds the `bench` binary and runs the harness, uploading the CSV as a
workflow artifact. The job is triggerable manually (workflow_dispatch) and
via tags that match `bench-*`.

Notes

- For low-noise hardware counters (cycles, cache-misses, branch-misses) run
	`perf stat` on a host machine or a self-hosted runner with `perf` enabled.
- The harness supports selecting a single strategy via the `BENCH_STRATEGY`
	environment variable for clearer per-strategy `perf stat` runs, e.g.

```sh
BENCH_STRATEGY=tiled_256 ./target/release/bench
```


Contributing

Please open issues for design discussions and submit focused pull requests for changes. Large API or design changes should be discussed in an issue first.

License

This project is released under the GNU Affero General Public License v3 (AGPL-3.0). See the `LICENSE` file for details.

Archived documentation

Legacy inventory and translation notes were moved to `docs/archive/` during a cleanup to keep top-level docs focused.

Contact & Sponsorship

Support this work via GitHub Sponsors: https://github.com/sponsors/rwilliamspbg-ops
