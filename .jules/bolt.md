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
