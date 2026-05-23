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

Contributing

Please open issues for design discussions and submit focused pull requests for changes. Large API or design changes should be discussed in an issue first.

License

This project is released under the GNU Affero General Public License v3 (AGPL-3.0). See the `LICENSE` file for details.

Archived documentation

Legacy inventory and translation notes were moved to `docs/archive/` during a cleanup to keep top-level docs focused.

Contact & Sponsorship

Support this work via GitHub Sponsors: https://github.com/sponsors/rwilliamspbg-ops
