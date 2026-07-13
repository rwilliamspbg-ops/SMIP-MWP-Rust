# Bolt's Journal

## 2024-10-24 - [Avoid Reading Environment Variables on Every Batch]
**Learning:** Calling `std::env::var` in high-throughput network datapath loops introduces severe overhead due to lock contention on the global environment lock and continuous heap allocation/deallocation of strings.
**Action:** Always cache configurations fetched from environment variables when values are stable throughout the process execution lifetime.

## 2024-10-24 - [Don't use OnceLock for tests mutating environment]
**Learning:** Caching environment variables globally using `OnceLock` breaks unit tests that mutate environment variables in the same process (e.g. using `env::set_var("MOHAWK_MCR_SPRAY_MODE", "full")`).
**Action:** Cache environment variables in the instance struct (e.g. `Forwarder`) upon construction instead of global `OnceLock` to support isolated unit tests that dynamically override variables.
