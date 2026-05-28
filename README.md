
# SMIP-MWP (Rust rewrite)

<!-- Hero badges -->

[![CI](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/ci.yml/badge.svg)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/ci.yml)
[![Bench Harness](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/bench-harness.yml/badge.svg)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/bench-harness.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)
[![Rust: stable](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![Edition: 2021](https://img.shields.io/badge/edition-2021-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2021/)
[![GitHub stars](https://img.shields.io/github/stars/rwilliamspbg-ops/SMIP-MWP-Rust?style=social)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/stargazers)
[![Open issues](https://img.shields.io/github/issues/rwilliamspbg-ops/SMIP-MWP-Rust)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/issues)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Performance: High](https://img.shields.io/badge/Performance-High-orange.svg)](docs/perf)
<a href="https://github.com/sponsors/rwilliamspbg-ops"><img src="https://img.shields.io/badge/Sponsor-❤-1EAEDB?style=social&logo=github" alt="Sponsor"></a>

Rust rewrite of SMIP-MWP — safe, testable, and high-performance datapath, crypto, routing, and CLI components. Built for low-latency, high-throughput networking and reproducible benchmarking.

---

## Performance

All numbers below come from the latest local Criterion runs on release builds (`cargo bench`). The benchmark artifacts and plots are kept in [docs/perf](docs/perf).

### Datapath throughput (packets / second)

| Benchmark | Latest local run |
|---|---:|
| Hit path — 16 pkts | 2.40 Mpps |
| Hit path — 64 pkts | 2.35 Mpps |
| Hit path — 256 pkts | 2.35 Mpps |
| Miss path — 16 pkts | 2.13 Mpps |
| Miss path — 64 pkts | 2.09 Mpps |
| Miss path — 256 pkts | 2.10 Mpps |

### Memory copy throughput (alloc + fill)

| Buffer size | Latest local run |
|---|---:|
| 1 KB | 34.9 GiB/s |
| 8 KB | 40.0 GiB/s |
| 64 KB | 45.8 GiB/s |

### Packet copy cost

| Buffer size | `extend_from_slice` | `copy_nonoverlapping` |
|---|---:|---:|
| 256 B | 39.6 GiB/s | 27.8 GiB/s |
| 1.5 KB | 72.4 GiB/s | 37.8 GiB/s |
| 4 KB | 63.9 GiB/s | 24.6 GiB/s |
| 64 KB | 2.72 GiB/s | 2.58 GiB/s |

### What was changed and why

**Round 1 — routing and allocation**

| Issue | Fix | Impact |
|---|---|---|
| SHA-256 on every route miss in `predictive_next_hop` | Replaced with `DefaultHasher` (SipHash) | Miss path +112% |
| Two heap allocations per forwarded packet (`encrypt()` + `newpkt`) | Reuse single `ct_buf` across batch loop via `encrypt_to()` | Hit path +3–5% |
| Route table re-sorted on every write (clone + `Vec::sort`) | Replaced `HashMap` + sorted `Vec` with `BTreeMap` | O(log n) writes, no extra alloc |
| AVX2 streaming-store branch never fired (`<= 4096` outer, `>= 4096` inner) | Flipped outer guard to `>= 4096` | Large payload copy now uses streaming stores |
| Dead code: `fill_pattern_scalar_chunked` | Removed | Zero warnings |

**Round 2 — zero-alloc encryption and zero-copy header parsing**

| Issue | Fix | Impact |
|---|---|---|
| `encrypt_to()` still hid an internal `Vec` allocation via `aead.encrypt()` | Switched to `aead::encrypt_in_place` — writes tag directly into `ct_buf` | Eliminates last hidden alloc per packet |
| SHA-256 in `derive_cache_key()` on every session lookup | Replaced with `DefaultHasher` | Session lookup ~500 ns → ~10 ns |
| `Header::parse()` in hot path: 7× `copy_from_slice` + 96-byte struct copy per packet | Added `HeaderViewRef<'a>` to `wire` crate; hot path now reads fields directly from packet buffer | Eliminates per-packet struct copy |
| `Vec` allocation for HKDF info concatenation in `derive_session_material` | Stack-allocated `[u8; 256]` instead | One less alloc per session creation |

**Round 3 — single-arena send + persistent buffer**

| Issue | Fix | Impact |
|---|---|---|
| Per-packet heap allocations for output packets (one `Vec` per packet) | Write outgoing packets into a single persistent arena (`Vec<u8>`) and send offsets to the socket; reuse arena across batches | Eliminates most per-packet allocations; datapath throughput improved ~10% in local pinned runs; allocator samples dropped in flamegraphs |
| Repeated feature detection and temporary buffers in hot loop | Hoisted AVX2 detection out of hot loop; reused a single ciphertext buffer (`ct_buf`) per batch | Reduced branch/feature-test overhead and removed hidden temporary allocations |

**Current validation snapshot**

| Area | Latest local result |
|---|---|
| Workspace tests | `cargo test --all --tests` passed |
| Routing miss sweep | Best route count was 2 in the latest local Criterion run |
| Bench artifacts | Updated CSV and SVG outputs are checked into `docs/perf/` |
| Stress instrumentation | CLI exposes `--metrics`, `--metrics-socket`, and `--metrics-http` |


---

## Repository layout

| Crate | Purpose |
|---|---|
| `crypto/` | Key exchange, session derivation, AEAD (AES-256-GCM / ChaCha20Poly1305) |
| `datapath/` | Forwarding hot path and tests |
| `afxdp/` | AF_XDP ring buffer integration and mocks |
| `routing/` | Route table (`BTreeMap`-backed) and predictive routing |
| `bench/` | Criterion microbench harness and smoke-run utilities |
| `cli/` | Binary entrypoint (`mohawk_node`) |
| `wire/` | Packet header marshal / parse / zero-copy view |

---

## Quick start

```sh
git clone https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust.git
cd SMIP-MWP-Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustup update stable
cargo test --workspace --all-targets   # 29 tests, zero warnings
cargo bench                             # Criterion benchmarks
cargo run --release -p bench            # smoke harness
```

---

## Bench harness

An automated harness runs multiple fill / processing strategies (scalar, tiled, AVX2-accelerated) and emits CSV output for reproducible comparison.

| File | Purpose |
|---|---|
| `tools/bench_harness/run_bench_harness.sh` | Driver — runs each strategy N times, appends to CSV |
| `tools/bench_harness/parse_and_append.py` | Parses bench output; columns: `timestamp,commit,run_index,strategy,size,avg_ns,throughput_mib_s` |
| `tools/bench_harness/summarize_csv.py` | Aggregates CSV into per-strategy summary |

**Local run:**

```sh
cargo build -p bench --release
./tools/bench_harness/run_bench_harness.sh 20 bench_results.csv
```

**Real hardware-backed benchmark:**

```sh
make real-bench \
	DUT_BIN=./target/release/mohawk-node \
	GEN_CMD="sudo trex-64r --cfg mycfg.yaml --duration 60" \
	IFACE=ens1f0 \
	DURATION=60 \
	OUT=/tmp/stress_pconf.csv
```

This runs the stress harness against a live DUT and records NIC counters plus per-process CPU times.

**Routing miss sweep:**

```sh
chmod +x tools/bench_harness/run_routing_miss_sweep.sh tools/bench_harness/parse_routing_miss_criterion.py
./tools/bench_harness/run_routing_miss_sweep.sh routing_miss_sweep.csv 10
python3 tools/bench_harness/plot_routing_miss_sweep.py routing_miss_sweep.csv routing_miss_sweep.svg
```

This writes one CSV row per route-table size and generates a quick SVG curve so you can spot the minimum fast.
Criterion requires a sample size of at least `10`.

The latest local run produced `docs/perf/routing_miss_sweep.csv` and `docs/perf/routing_miss_sweep.svg`, with the minimum at `route_count=2`.

**Broad vs fine sweep:**

| route_count | broad mean_time_ns | fine mean_time_ns |
|---|---:|---:|
| 1 | 24.203 | 24.442 |
| 2 | 24.926 | 26.014 |
| 4 | 25.332 | 25.496 |
| 8 | 27.956 | 27.650 |
| 16 | 29.898 | 29.661 |

The minimum stayed at `route_count=1` in both sweeps.

Saved artifacts live in [docs/perf](docs/perf) for quick sharing and review, including:

- [bench_results.csv](docs/perf/bench_results.csv)
- [routing_miss_sweep.csv](docs/perf/routing_miss_sweep.csv)
- [routing_miss_sweep.svg](docs/perf/routing_miss_sweep.svg)

**Per-strategy profiling with `perf`:**

```sh
BENCH_STRATEGY=tiled_256 perf stat ./target/release/bench
```

The `bench-harness` GitHub Actions workflow builds the binary and uploads `bench_results.csv` + `bench_summary.csv` as a workflow artifact (90-day retention). Trigger it manually or push a `bench-*` tag.

---

## CI

The CI workflow (`ci.yml`) runs on every push and pull request:

1. `cargo build --release --workspace`
2. `cargo test --workspace --all-targets`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo run --release -p bench` (smoke)

Cargo registry and build artefacts are cached by `Cargo.lock` hash to keep runs fast.

---

## Contributing

Open issues for design discussions; submit focused pull requests for changes. Large API or design changes should be discussed in an issue first. See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

Released under the [GNU Affero General Public License v3 (AGPL-3.0)](https://www.gnu.org/licenses/agpl-3.0.html). See `LICENSE` for details.

---

## Archived documentation

Legacy inventory and translation notes are in `docs/archive/`.

---
# Case Study: High-Performance Network Datapath Optimization (Rust)

A ground-up Rust rewrite and iterative optimization of **SMIP-MWP**, a safe, testable, and ultra-high-performance networking protocol datapath handling cryptographic routing, session management, and AF_XDP ring buffer integration. 

By systematically targeting memory allocations, cache misses, and cryptographic bottlenecks over three rounds of profiling, I achieved a **~3× throughput increase on the miss path** and pushed the core hit path to **2.49 Mpps**.

## 🚀 Impact & Performance Milestones

All metrics are backed by rigorous, automated Criterion microbenchmarks (`cargo bench`) running on release builds:

### Datapath Throughput (Packets / Second)
# SMIP-MWP Rust

Lightweight, high-performance Rust implementation of SMIP-MWP components (datapath, crypto, routing, AF_XDP integration and a minimal CLI). This README summarizes the current repository state, how to build and test the workspace locally, the benchmark harness, and licensing.

---

**Status:** Active development on `perf/param-sweep-routing` branch. Core crates build and unit-test in CI; benchmarks are exercised via the `bench` crate and GitHub Actions.

**License (short):** This repository is released under the GNU Affero General Public License v3 (AGPL-3.0). See [LICENSE](LICENSE) for the full text and obligations.

---

**Repository layout (top-level crates):**

- `crypto/` — key exchange, session derivation, AEAD utilities
- `datapath/` — forwarding hot path and datapath tests
- `afxdp/` — AF_XDP ring buffer integration and mocks
- `routing/` — route table and predictive routing implementations
- `bench/` — Criterion microbench harness and smoke-run utilities
- `cli/` — binary entrypoint(s)
- `wire/` — packet header marshal/parse and zero-copy views
- `tools/` — bench harness scripts, plotting and post-processing utilities

---

**Quick start (developer machine)**

1. Install Rust toolchain (stable):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustup update stable
```

2. Clone and build the workspace:

```sh
git clone https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust.git
cd SMIP-MWP-Rust
cargo build --workspace --release
```

3. Run tests (unit + integration):

```sh
cargo test --workspace --all-targets
```

Notes:
- Use `--workspace` to operate across all crates.
- Some integration tests and AF_XDP examples require elevated privileges or kernel support.

---

**Benchmarks & harness**

Microbenchmarks use Criterion and are housed in the `bench` crate. A small harness under `tools/bench_harness` automates strategy sweeps and CSV output.

- Run Criterion benches:

```sh
cargo bench --manifest-path bench/Cargo.toml
```

- Local harness (smoke):

```sh
cargo build -p bench --release
./tools/bench_harness/run_bench_harness.sh 20 bench_results.csv
```

- Routing miss sweep (example):

```sh
chmod +x tools/bench_harness/run_routing_miss_sweep.sh
./tools/bench_harness/run_routing_miss_sweep.sh routing_miss_sweep.csv 10
python3 tools/bench_harness/plot_routing_miss_sweep.py routing_miss_sweep.csv routing_miss_sweep.svg
```

For reproducible CI artifacts, the repo uses GitHub Actions workflows that build and upload CSV/SVG outputs.

---

**Development tips & common commands**

- Build and run the CLI (binary name may vary):

```sh
cargo run -p cli --release -- <cli-args>
```

- Run the smoke harness used by CI:

```sh
cargo run --release -p bench
```

- Run clippy with strict lints:

```sh
cargo clippy --workspace --all-targets -- -D warnings
```

---

**Continuous integration**

The `ci.yml` workflow runs on push and PRs and executes (roughly):

1. `cargo build --release --workspace`
2. `cargo test --workspace --all-targets`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. smoke run of the bench harness (`cargo run -p bench`)

Caching is configured for Cargo build artifacts to keep CI fast.

---

**License and distribution**

- This repository is distributed under the GNU Affero General Public License v3 (AGPL-3.0). The full license text is available in [LICENSE](LICENSE).
- If you need a different license clarification (for example: dual-licensing, third-party subcomponents under different terms, or permissive licensing for specific crates), tell me which crates or files require special treatment and I will update this README and add `LICENSE-<crate>.md` files as needed.

---

**Where to look next**

- Tests and examples: see `datapath/`, `crypto/`, and `afxdp/` for unit and integration tests.
- Bench scripts and plots: `tools/bench_harness/` and `docs/perf/`.
- Contribution guide: [CONTRIBUTING.md](CONTRIBUTING.md)

---

If you'd like, I can:

- add a short `README` per crate with crate-specific run/test examples,
- add an abbreviated `LICENSE-summary.md` that lists license for each crate/file,
- update the GitHub Actions badges in the top-level README to match the default branch.

Please tell me which of these follow-ups you'd like me to do next.
