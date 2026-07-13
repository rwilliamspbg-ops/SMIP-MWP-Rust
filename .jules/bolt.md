# Bolt's Journal

Critical learnings only.

## 2024-11-20 - Environment Variable Lookups in High-Frequency Hot Paths
**Learning:** Querying environment variables (`std::env::var`) within high-frequency datapath packet processing loops (such as batch polls) causes massive performance overhead due to process-wide locking, string formatting, and allocation.
**Action:** Cache configuration values at the struct/instance level during initialization instead of querying them dynamically inside processing functions or using global `OnceLock` caches that might complicate concurrent testing.
