# afxdp

Purpose: AF_XDP ring buffer integration and mocks for unit testing.

Build & test

```sh
cd afxdp
cargo test
```

Notes

- Some examples and integration tests may require elevated privileges and a kernel with AF_XDP support.

Configuration & Metrics

- Environment variables used by the CLI and `RealSocket` initializer:
	- `MOHAWK_IFACE` — network interface name to bind AF_XDP sockets to (required for `--real`).
	- `MOHAWK_QUEUE_ID` — queue id (defaults to `0`).
	- `MOHAWK_FRAME_SIZE` — UMEM frame size in bytes (default `2048`).
	- `MOHAWK_UMEM_PAGES` — number of UMEM pages/frames (default `1024`).
	- `MOHAWK_FREELIST_HEADROOM` — optional absolute headroom (frames) added to the free-list capacity; defaults to `max(8, frames/8)`.

- Metrics exposed by the CLI `/metrics` Prometheus endpoint:
	- `afxdp_retry_total` (counter): count of send retry attempts when TX is full.
	- `afxdp_backpressure_total` (counter): count of TX backpressure events.
	- `afxdp_alloc_from_freelist_total` (counter): allocations served from the free-list.
	- `afxdp_alloc_fallback_total` (counter): allocations that fell back to the bump allocator.
	- `afxdp_free_push_drop_total` (counter): number of times returning frames to the free-list failed (drops).

The CLI registers these gauges and syncs them from in-process atomics once per second when `--metrics-http` is enabled.
