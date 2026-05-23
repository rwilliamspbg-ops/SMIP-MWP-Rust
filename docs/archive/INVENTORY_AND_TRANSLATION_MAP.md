# Inventory and Translation Map (archived)

This document was moved to the `docs/archive/` directory as part of a
repository clean-up. Original content follows.

# Inventory and Translation Map

This document inventories Go packages in the repository and maps them to the Rust workspace crates created under `rust_rewrite/`.

## Summary mapping

- `internal/wire` -> `wire` crate
- `internal/crypto` (+ `kex.go`) -> `crypto` crate
- `internal/routing` -> `routing` crate
- `internal/datapath/afxdp` -> `datapath` + `afxdp` crates (core datapath in `datapath`, XDP-specific code in `afxdp`)
- `cmd/mohawk-node` -> `cli` crate (binary)
- `benchmarks` + `moongen` harness -> `bench` crate

## Inventory (high-level)

- `internal/wire`
  - Files: `header.go`, `header_test.go`
  - Responsibilities: packet header formats, parsing/marshaling
  - Translation notes: implement byte-level parsing, provide safe view/mutate helpers

- `internal/crypto`
  - Files: `hybrid.go`, `handshake.go`, tests, cache/bench tests
  - Responsibilities: KEX, session key derivation, AEAD encryption/decryption, caches
  - Translation notes: use `rand`, `aes-gcm` or `ring`/`rust-crypto` crates; port tests

- `internal/routing`
  - Files: `router.go`, `router_enhanced.go`, tests
  - Responsibilities: route table, lookup/prediction, sharded maps
  - Translation notes: port data structures and prediction logic; use `dashmap` or sharded mutexes

- `internal/datapath/afxdp`
  - Files: many (forwarder loop, UMEM, socket, pools, mock, tests)
  - Responsibilities: high-performance XDP loop, buffer pools, session lookups, in-place encryption
  - Translation notes: split hot path into `datapath` crate; implement AF_XDP integration in `afxdp` using `aya` or `libbpf-rs`; provide mock implementation for CI

- `cmd/mohawk-node`
  - Files: `main.go`, `main_withafxdp.go`
  - Responsibilities: CLI flags, forwarding startup, configuration
  - Translation notes: implement `structopt`/`clap` CLI, wire up runtime initialization

- Top-level files: `kex.go` (key exchange helpers) -> `crypto::kex`

## External dependencies to track

- cryptography: Go uses native Go crypto libs; Rust will use `ring`, `aes-gcm`, `hkdf`, and `rand` or `rand_core`.
- AF_XDP: in Rust use `aya` or `libbpf-rs`; include a pure-Rust mock for CI.
- Concurrency primitives: translate Go sharded maps and pools to Rust equivalents (`dashmap`, `crossbeam`, `parking_lot`).

## Immediate next steps

1. Translate `internal/wire/header.go` into `rust_rewrite/wire/src/lib.rs` with full parsing/marshalling tests.
2. Port `internal/routing/router.go` to `rust_rewrite/routing` to provide routing table API used by forwarder.
3. Port `internal/crypto` primitives to `rust_rewrite/crypto` (KEX, sessions).
