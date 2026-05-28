# datapath

Purpose: forwarding hot path, packet processing and dataplane-focused tests.

Build & test

```sh
cd datapath
cargo test
```

Notes

- Hot-path optimizations are performance-sensitive; run benches under `bench` for microbenchmarks.
