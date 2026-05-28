# SMIP-MWP-Rust

[![CI](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/ci.yml/badge.svg)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/ci.yml)
[![Bench Harness](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/bench-harness.yml/badge.svg)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/bench-harness.yml)
[![Remote Bench](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/remote-bench.yml/badge.svg)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/remote-bench.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)
[![Rust: stable](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![Edition: 2021](https://img.shields.io/badge/edition-2021-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2021/)

SMIP-MWP-Rust is the Rust workspace for the SMIP-MWP datapath stack: crypto, routing, AF_XDP integration, CLI control-plane glue, and benchmark tooling. The repository is currently in active development on `main`. CI builds the workspace, runs tests, validates the bridge contract, and exercises the chaos benchmark/report gates.

## Current State

- Workspace crates: `afxdp`, `bench`, `benchmark`, `cli`, `crypto`, `datapath`, `routing`, and `wire`.
- Validation entry points: `cargo test --workspace --all-targets`, `make verify-bridge`, and `make performance-envelope`.
- Generated validation artifacts live under `benchmark/`, `docs/perf/`, and `tools/bench_results/`.
- The bridge validation wrapper is committed at [tools/validation/verify_bridge.sh](tools/validation/verify_bridge.sh).
- Last verified locally: workspace tests, bridge validation, and the median/7-rep chaos gate configuration used by CI.

## Quick Start

```sh
git clone https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust.git
cd SMIP-MWP-Rust
source $HOME/.cargo/env
cargo test --workspace --all-targets
make verify-bridge
REPS=7 AGG_METHOD=median ./tools/benchmark/ci_validate_chaos_report.sh
```

## Repository Layout

- `crypto/` - key exchange, session derivation, and AEAD helpers
- `datapath/` - forwarding hot path and datapath tests
- `afxdp/` - AF_XDP ring integration and mocks
- `routing/` - route table and predictive routing
- `bench/` - Criterion microbench harness and smoke-run utilities
- `benchmark/` - chaos benchmark harness and performance envelope docs
- `cli/` - binary entrypoint and bridge contract wiring
- `wire/` - packet header marshal/parse and zero-copy view
- `tools/` - benchmark, validation, plotting, and stress scripts

## Validation And Performance

- [benchmark/README.md](benchmark/README.md) explains the chaos benchmark harness and CI contract gate.
- [docs/perf/README.md](docs/perf/README.md) indexes checked-in Criterion and routing-sweep artifacts.
- [RELEASE.md](RELEASE.md) is the changelog for notable repo-level changes.

## Notes

- The top-level CI workflow runs build, workspace tests, smoke validation, bridge validation, chaos report gating, and benchmark assertions.
- Validation runs may leave generated CSVs and reports in the working tree; those are derived artifacts, not source.
- The project is licensed under AGPL-3.0. See [LICENSE](LICENSE) for the full text.
