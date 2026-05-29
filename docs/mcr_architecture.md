# MCR Spraying Architecture

This document summarizes the Multi-Channel Routing (MCR) spraying integration for SMIP-MWP-Rust.

Overview
- Adds `lookup_spray()` to the `routing` crate for hash-based channel selection.
- Adds MCR metadata to `RouteEntry` and per-destination `ChannelStats`.
- Introduces MCR-aware hooks in the `datapath` `Forwarder` with `process_batch_mcr` and `process_batch_spray_full` stubs.
- Benchmarks and report generation tools under `tools/benchmark`.

Configuration
- Use `MOHAWK_MCR_ENABLED`, `MOHAWK_MCR_SPRAY_MODE`, `MOHAWK_MCR_CHANNELS`, and `MOHAWK_MCR_HASH_SEED` to tune behavior.

Validation
- Routing and datapath unit tests updated to exercise the new structures.
- Use `make mcr-test` and `make mcr-benchmark` to run CI and chaos profiles.

See source for implementation details in `routing/src/lib.rs` and `datapath/src/lib.rs`.
