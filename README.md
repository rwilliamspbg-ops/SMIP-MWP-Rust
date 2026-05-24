# SMIP-MWP (Rust rewrite)

[![CI](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/ci.yml/badge.svg)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/ci.yml)
[![Bench Harness](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/bench-harness.yml/badge.svg)](https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/actions/workflows/bench-harness.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)
[![Rust: stable](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![Edition: 2021](https://img.shields.io/badge/edition-2021-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2021/)
<a href="https://github.com/sponsors/rwilliamspbg-ops"><img src="https://img.shields.io/badge/Sponsor-❤-1EAEDB?style=social&logo=github" alt="Sponsor"></a>
[![Contributing](https://img.shields.io/badge/contributing-guidelines-brightgreen.svg)](CONTRIBUTING.md)

Rust rewrite of SMIP-MWP — safe, testable, and high-performance datapath, crypto, routing, and CLI components.

---

## Performance

All numbers from Criterion benchmarks on release builds (`cargo bench`). Two rounds of targeted optimisations have been applied since the initial scaffold.

### Datapath throughput (packets / second)

| Benchmark | Scaffold | After round 1 | After round 2 | After round 3 |
|---|---:|---:|---:|---:|
| Hit path — 16 pkts | 2.01 Mpps | 2.11 Mpps | 2.18 Mpps | 2.49 Mpps |
| Hit path — 64 pkts | 1.86 Mpps | 1.91 Mpps | 1.98 Mpps | 2.40 Mpps |
| Hit path — 256 pkts | 1.84 Mpps | 1.91 Mpps | 1.97 Mpps | 2.38 Mpps |
| Miss path — 16 pkts | 813 Kpps | 1.78 Mpps | 1.91 Mpps | 2.18 Mpps |
| Miss path — 64 pkts | 785 Kpps | 1.67 Mpps | 1.84 Mpps | 2.14 Mpps |
| Miss path — 256 pkts | 786 Kpps | 1.66 Mpps | 1.82 Mpps | 2.11 Mpps |

### Memory copy throughput (alloc + fill)

| Buffer size | Scaffold | Current |
|---|---:|---:|
| 1 KB | 23.0 GiB/s | ~33.0 GiB/s |
| 8 KB | 32.3 GiB/s | ~40.0 GiB/s |
| 64 KB | 32.3 GiB/s | ~45.1 GiB/s |

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

## Sponsorship

Support this work via GitHub Sponsors: https://github.com/sponsors/rwilliamspbg-ops
