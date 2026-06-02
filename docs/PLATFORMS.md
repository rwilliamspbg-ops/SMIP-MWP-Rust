# Supported Platforms & Quickstart Notes

This document explains supported platforms, portability considerations, and tips for running SMIP-MWP-Rust on a wider range of devices.

## Supported architectures

- x86_64 (linux)
- aarch64 (linux)

CI builds are performed for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`. We also provide optional `musl` builds for static binaries: `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`.

## AF_XDP and NIC requirements

- AF_XDP requires Linux and an AF_XDP-capable NIC. Root privileges are typically required for real AF_XDP runs and for adjusting hugepages and CPU pinning.
- For CI and development, use the `afxdp` crate mocks to run and test datapath logic on laptops and CI runners without hardware NICs.

## Running on ARM devices (Raspberry Pi, AWS Graviton)

- Use the `aarch64-unknown-linux-gnu` target for most builds. For fully static binaries (e.g., for minimal containers), use the `aarch64-unknown-linux-musl` target where dependencies permit.
- Recommended quick test:

```sh
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

## Cross-building & helpers

- We provide `tools/cross-build.sh` (or use `cross`) to simplify building for other targets and for musl.

## Mock/dev AF_XDP mode

- To run without a real NIC, enable the AF_XDP mock modes documented in `afxdp/README.md`. This allows running unit tests and smoke runs on developer machines.
 - To run without a real NIC, enable the AF_XDP mock modes documented in `afxdp/README.md`. This allows running unit tests and smoke runs on developer machines.

### Running with AF_XDP mocks

1. See [afxdp/README.md](afxdp/README.md) for available mock flags and build options.
2. Example: run the benchmark binary using the afxdp mock environment (no special privileges required):

```sh
MOCK_AFXDP=1 cargo run --release -p benchmark -- --packets 1000 --payload-len 64
```

3. Use the mock mode to validate logic on CI runners that don't have privileged NIC access.
## Packaging and artifacts

- We provide CI workflows to produce per-arch release artifacts and multi-arch container images.
- For platform packaging examples (deb/rpm/Homebrew), see `tools/packaging/`.

## Notes

- See `README.md` for quick start steps and additional validation/run notes.