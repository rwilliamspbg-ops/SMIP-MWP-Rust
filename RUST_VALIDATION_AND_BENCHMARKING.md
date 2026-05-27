# Rust Workspace Validation & Benchmarking

**Scope**: This workspace is a Rust rewrite scaffold for SMIP-MWP. The commands below validate the current Rust crates and binaries in this repository. They do not require the Go-specific tags or scripts from the upstream AF_XDP document.

## Quick Start

```bash
rustc --version
cargo --version
```

## Repository Verification

Run the same commands that the repository uses for CI and local smoke checks:

```bash
cargo build --release
cargo test --workspace --all-targets
```

Additional smoke checks:

```bash
cargo run -p cli
cargo run -p cli -- --demo
cargo run --release -p cli -- --metrics-http 127.0.0.1:9090
cargo run --release -p bench
```

## What These Checks Cover

- `cargo build --release` validates the full workspace compiles in optimized mode.
- `cargo test --workspace --all-targets` runs unit tests across all member crates.
- `cargo run -p cli` exercises the CLI entry point and the usage path.
- `cargo run -p cli -- --demo` runs the in-process forwarding demo.
- `cargo run --release -p cli -- --metrics-http 127.0.0.1:9090` starts the JSON metrics endpoint used by the stress harness.
- `cargo run --release -p bench` runs the synthetic allocation benchmark in `bench/src/lib.rs` and prints throughput-style numbers.

## Current Repository Notes

- The workspace CI currently checks `cargo build --release`.
- The repo has passing tests across `wire`, `crypto`, `routing`, `datapath`, `afxdp`, `cli`, and `bench`.
- The benchmark output currently reflects synthetic allocation/fill performance, not real AF_XDP packet latency or NIC throughput.
- The latest perf artifacts also include routing miss sweep CSV/SVG outputs under `docs/perf/`.
- `cargo` warnings about the workspace resolver are avoided by using `resolver = "2"` at the workspace root.

## AF_XDP / Hardware Validation Plan

The original AF_XDP document contains hardware-oriented steps that are not yet implemented in this Rust scaffold. When real AF_XDP support lands, the equivalent validation should look like this:

```bash
uname -r
ethtool -i <interface>
ethtool -l <interface>
```

Then validate on real hardware with:

- a NIC driver that supports AF_XDP
- the correct number of RX/TX queues
- hugepages configured if the implementation requires them
- root privileges for binding and socket setup

At that point, add a Rust-native AF_XDP smoke test and a benchmark harness that measures packet throughput and latency against the actual datapath implementation.

## Suggested Result Log

Record the following for each run:

- kernel version
- cargo version
- release build status
- unit test status
- CLI smoke status
- benchmark output
- hardware details for any AF_XDP test host

## Expected Baseline For This Workspace

For the current scaffold, a healthy local run should look like this:

```bash
cargo build --release
cargo test --workspace --all-targets
cargo run -p cli -- --demo
cargo run -p cli -- --metrics
cargo run --release -p bench
```

Each command should complete successfully without resolver warnings or failing tests.
