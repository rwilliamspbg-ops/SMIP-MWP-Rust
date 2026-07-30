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

## 2026-05-30 - [Eliminate Redundant XOR Hashing on Identical Keys and Optimize Slices]
**Learning:** Passing identical arrays to XOR-folded hash functions (such as `fast_flow_hash(&dst_id, &dst_id, flow_label)`) is completely redundant as identical elements cancel out to zero ($x \oplus x = 0$). This wastes clock cycles on unaligned reads and XOR fold logic. Additionally, replacing slice-level `copy_from_slice` on statically known sizes with array-reference assignments allows LLVM to emit register-level AVX/SSE moves in hot datapaths.
**Action:** Always inspect hash parameters for self-canceling inputs, and utilize try_from reference assignments for zero-overhead, statically-sized array copies.

## 2026-05-31 - [Defer Expensive 256-bit Hash Computations Until Thread-Local Cache Misses]
**Learning:** Performing a full 256-bit XOR fold of the 32-byte destination ID before checking the thread-local routing cache introduces avoidable ALU and memory load overhead. Checking the cache using a simplified hash (e.g., reading just the first 32 bits unaligned and masking) completely avoids loading the rest of the 32-byte array and performing the XOR folds on hot cache hits.
**Action:** Check hot-key caches using lightweight, partial-key indices, and defer any full key hashing or expensive calculations until after a cache miss is confirmed.

## 2026-06-01 - [Avoid Copying Large Values from Cell on Thread-Local Hits]
**Learning:** Storing hot thread-local caches using parallel arrays of `Cell` requires retrieving those values via `.get()`, which triggers redundant copies of large structures (such as 32-byte arrays) onto the stack on every single cache hit. Wrapping the entire `ThreadCache` in `std::cell::UnsafeCell` instead allows safe, synchronous, and direct reference comparisons/reads on plain arrays with zero copying or borrow-checking overhead.
**Action:** Use a single `std::cell::UnsafeCell` for hot, synchronous, non-yielding thread-local caches containing larger Copy types, allowing bounds-free direct element reads and in-place updates.

## 2026-06-02 - [Vectorized Unaligned u64 Writes for Nonce Construction]
**Learning:** Assigning elements of a fixed-size byte array byte-by-byte (e.g. `nonce[4] = bytes[0]; ...`) to construct cryptographic nonces introduces multiple bounds-check branches and indexing overhead. Doing an unaligned 64-bit big-endian write (`ptr.write_unaligned(mixed.to_be())`) allows LLVM to compile the entire nonce modification down to a single register-level store instruction without bounds checking.
**Action:** Use unaligned mut pointer writes (`write_unaligned`) to write multi-byte integer values to contiguous indices of fixed-size arrays on performance-critical paths.
