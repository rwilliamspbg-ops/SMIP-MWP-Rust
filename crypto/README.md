# crypto

Purpose: key exchange, session derivation, AEAD helpers used by other crates.

Build & test

```sh
cd crypto
cargo test
```

Notes

- Uses `ring`/`aes-gcm`/`chacha20poly1305` (see Cargo.toml).
- Typical CI runs clippy and unit tests from the workspace root.
