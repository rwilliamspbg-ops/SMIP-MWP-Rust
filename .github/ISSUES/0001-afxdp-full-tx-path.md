Title: Implement full AF_XDP TX path and ring recycling

Description:
The `afxdp` crate contains a working `RealSocket` skeleton that allocates UMEM, mmap's the ring region and provides basic `poll`/`send` helpers that copy frames in/out of UMEM. However, the implementation is incomplete for production use:

- The TX path currently writes descriptors via `RingMmap::tx_push` but does not interact with kernel TX completion (`comp`) ring to recycle frames.
- There is no proper fill/comp ring handling to return frames to a free pool.
- No support for batching, headroom management, or zero-copy chaining across frames.
- No error handling or backpressure when TX rings are full.

Work items:
- Implement fill/comp ring enqueue/dequeue helpers and document expected offsets.
- Add a frame allocator with free-list semantics to recycle UMEM frames.
- Add backpressure handling in `send()` (return Err when cannot push all descriptors).
- Add unit tests that simulate kernel ring behavior (populate comp ring, check frames recycled).

Labels: enhancement, afxdp, perf
Assignees: @maintainers

Notes:
See `afxdp/src/socket.rs`, `afxdp/src/umem.rs`, and `afxdp/src/rings.rs` for current skeletons. Unit tests exist for basic ring reads/writes and should be expanded to cover full lifecycle.
