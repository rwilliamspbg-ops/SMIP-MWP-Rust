# Workspace packages

This workspace contains the following crates (high-level summary):

- `crypto` — key exchange, HKDF session derivation, AEAD wrappers and tests
- `datapath` — forwarding hot path, forwarder tests and microbench fixtures
- `afxdp` — AF_XDP-specific integration and mocks for CI
- `routing` — route table, prediction helpers and policies
- `wire` — packet header views and parsing/marshalling helpers
- `bench` — Criterion benchmarks, smoke-run harness and utility benches
- `cli` — binary entrypoint and demo flags (mohawk-node)

Each crate is independently testable via `cargo test -p <crate>` and built through the workspace via `cargo build --workspace`.
