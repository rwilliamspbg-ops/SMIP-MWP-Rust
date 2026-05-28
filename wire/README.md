# wire

Purpose: packet header marshal/parse and zero-copy views used by the datapath.

Build & test

```sh
cd wire
cargo test
```

Notes

- `HeaderViewRef<'a>` provides zero-copy parsing for hot-path performance.
