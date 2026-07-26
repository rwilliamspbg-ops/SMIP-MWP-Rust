# Bolt's Journal

## 2024-10-24 - [Avoid Reading Environment Variables on Every Batch]
**Learning:** Calling `std::env::var` in high-throughput network datapath loops introduces severe overhead due to lock contention on the global environment lock and continuous heap allocation/deallocation of strings.
**Action:** Always cache configurations fetched from environment variables when values are stable throughout the process execution lifetime.

## 2024-10-24 - [Don't use OnceLock for tests mutating environment]
**Learning:** Caching environment variables globally using `OnceLock` breaks unit tests that mutate environment variables in the same process (e.g. using `env::set_var("MOHAWK_MCR_SPRAY_MODE", "full")`).
**Action:** Cache environment variables in the instance struct (e.g. `Forwarder`) upon construction instead of global `OnceLock` to support isolated unit tests that dynamically override variables.

## 2024-11-20 - [Optimize TLS Lookup via Direct-Mapped Cache & Array Reference Getters]
**Learning:** Sequential searches over a ring-buffer in thread-local storage (`THREAD_CACHE`) introduce non-trivial branch/loop overhead in high-throughput routing. Returning dynamically sized slices (`&[u8]`) for fixed-size fields causes redundant conversions/copies (`try_into().unwrap()`).
**Action:** Use a fast XOR-based direct-mapped cache for Thread-Local Storage lookups and return fixed-size array references (`&[u8; 32]`) for fixed-length packet fields.

## 2024-11-21 - [Fast Cache Index Folding and In-Place Arena Processing]
**Learning:** Performing a 32-bit loop-based XOR hash and a modulo operator (`%`) in the hot routing cache mapping path introduces minor but measurable ALU overhead. Also, recreating and resizing temporary `Vec<u8>` buffers per packet in `process_batch_mcr`'s primary path causes substantial dynamic memory allocation overhead.
**Action:** Replace the 32-bit loop with optimized 64-bit folding and use a bitwise AND mask (`& (HOT_CACHE_SIZE - 1)`) for O(1) index mapping. Process and encrypt packets directly inside the pre-allocated `self.arena` buffer to completely eliminate per-packet dynamic allocations.

## 2026-05-24 - [Avoid Slice Assertions and Copy Overhead in Hot Paths]
**Learning:** Constructing a `GenericArray` with `GenericArray::from_slice` and manipulating slices with `copy_from_slice` in packet processing loops introduces redundant runtime bounds checks and assert branches. Direct element indexing of fixed-size arrays and by-value construction of static types compiles to entirely bounds-free, branchless machine code.
**Action:** Use fixed-size array indices and by-value types like `GenericArray::from(*fixed_array)` instead of slice-level operations on hot execution paths.

## 2026-05-25 - [Coarse-Grained Batching of Hot Path Telemetry and Timing]
**Learning:** Performing multiple atomic fetch-add operations, `OnceLock::get` calls, and `Instant::now` / `elapsed` timings per packet in high-frequency data planes introduces substantial CPU overhead and cache line contention/bouncing in parallel execution threads.
**Action:** Always accumulate telemetry counters and durations locally in a register-backed or stack-allocated struct (e.g., `LocalMetrics`) during batch processing, and commit them in a single aggregated atomic write operation at the end of the batch.

## 2026-05-26 - [Un-aligned Pointer-based XOR Folding and Batch-level Profiling]
**Learning:** Even with local metrics accumulation, calling high-resolution timers (`Instant::now()`) per packet adds considerable VDSO/syscall overhead on high-frequency network datapath loops. Also, using generic hashing (`AHasher`) and manual slice slicing on fixed-size arrays inside hot routing caches causes significant bounds check, loop, and instruction overhead.
**Action:** Move timing profiling to the batch/loop level and use direct unaligned pointer casting (`read_unaligned()`) to perform bounds-free 64-bit XOR folding of packet arrays combined with fast power-of-two bitwise mask index mapping.

## 2026-05-27 - [Structure of Arrays Cache Optimization and Reusing Precomputed Hashes]
**Learning:** Storing hot-path thread-local caches as unified structures causes heavy and redundant copying on every lookup. Converting to a Structure of Arrays (SoA) layout lets us check fast fields (like epoch and destination ID) first, preventing expensive 32-byte array copies on cache misses. Furthermore, recalculating unaligned 64-bit XOR folds for the same destination ID in cache indexing, shard mapping, and fallback paths is wasteful. Precomputing the hash once and passing it down eliminates redundant computation.
**Action:** Decompose cache structs into parallel flat arrays and reuse precomputed hash values across consecutive hot-path lookups.

## 2026-05-27 - [Avoid Caching Flow-Dependent Routes Under Destination-Only Keys]
**Learning:** Caching multi-path (spray) forwarding routes (like in `lookup_spray_primary`) under destination-only keys in a flat thread-local cache breaks flow affinity. Subsequent packets with different flow labels will hit the cache and route to the same next-hop, completely disabling load balancing across alternate paths.
**Action:** Never cache flow-dependent routes using only the destination ID as the key; keep spray primary queries as non-cached O(1) shard map lookups.

## 2026-05-28 - [Avoid Redundant Cloning on Routing Table Queries]
**Learning:** Returning a cloned `RouteEntry` from the routing table lookup functions (`lookup_spray` and `lookup_spray_primary`) introduces severe heap allocation/deallocation and copying overhead under high throughput because `RouteEntry` contains a heap-allocated `Vec` for alternate routing channels.
**Action:** Avoid calling `.cloned()` on routing table map queries. Instead, acquire the map read lock, read/resolve routing directly from references, and return only the requested values (e.g. `[u8; 32]`) or build vectors in-place on the stack, maintaining a zero-allocation hot routing path.

## 2026-05-29 - [Safe Vectorized Array Assignments and Cache Scalability]
**Learning:** Generic slice-copy functions like `copy_from_slice` incur runtime bounds-checking and generic `memcpy` call overhead. Using safe Rust array-reference casts and assignments (e.g., `*<&mut [u8; 32]>::try_from(...).unwrap() = value`) allows LLVM to statically verify bounds safety and emit register-level vectorized moves without using `unsafe` blocks. Additionally, increasing thread-local cache capacity (from 16 to 256) using inline const array repetition prevents cache thrashing under dense routing tables.
**Action:** Always prefer safe array-reference conversions (`try_from`) over generic `copy_from_slice` when sizes are statically known, and use clean array repetition syntax (`[const { ... }; SIZE]`) to initialize larger thread-local caches.
