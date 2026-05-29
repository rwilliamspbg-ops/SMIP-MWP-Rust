//! AF_XDP UMEM and socket abstractions.

pub mod rings;
pub mod socket;
pub mod umem;

pub use socket::{AfXdpSocket, MockSocket};

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

/// Per-socket counters structure exposed for labeled metrics.
pub struct SocketCounters {
    pub retry: AtomicU64,
    pub backpressure: AtomicU64,
    pub alloc_from_freelist: AtomicU64,
    pub alloc_fallback: AtomicU64,
    pub free_push_drop: AtomicU64,
}

impl SocketCounters {
    pub fn new() -> Self {
        SocketCounters {
            retry: AtomicU64::new(0),
            backpressure: AtomicU64::new(0),
            alloc_from_freelist: AtomicU64::new(0),
            alloc_fallback: AtomicU64::new(0),
            free_push_drop: AtomicU64::new(0),
        }
    }
}

type SocketRegistry = HashMap<String, Arc<SocketCounters>>;

static SOCKET_REGISTRY: once_cell::sync::Lazy<Mutex<SocketRegistry>> = once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

/// Register per-socket counters under the given label. Overwrites existing.
pub fn register_socket_metrics(label: String, counters: Arc<SocketCounters>) {
    let mut reg = SOCKET_REGISTRY.lock().unwrap();
    reg.insert(label, counters);
}

/// Unregister per-socket metrics for a label.
pub fn unregister_socket_metrics(label: &str) {
    let mut reg = SOCKET_REGISTRY.lock().unwrap();
    reg.remove(label);
}

/// Snapshot all registered per-socket counters for consumption by the CLI.
pub fn snapshot_all_socket_metrics() -> Vec<(String, (u64, u64, u64, u64, u64))> {
    let reg = SOCKET_REGISTRY.lock().unwrap();
    reg.iter()
        .map(|(k, v)| {
            (
                k.clone(),
                (
                    v.retry.load(Ordering::Relaxed),
                    v.backpressure.load(Ordering::Relaxed),
                    v.alloc_from_freelist.load(Ordering::Relaxed),
                    v.alloc_fallback.load(Ordering::Relaxed),
                    v.free_push_drop.load(Ordering::Relaxed),
                ),
            )
        })
        .collect()
}

/// Global counters exported for diagnostics and metrics.
pub static AF_XDP_RETRY_COUNT: AtomicU64 = AtomicU64::new(0);
pub static AF_XDP_BACKPRESSURE_COUNT: AtomicU64 = AtomicU64::new(0);
pub static AF_XDP_ALLOC_FROM_FREELIST_COUNT: AtomicU64 = AtomicU64::new(0);
pub static AF_XDP_ALLOC_FALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);
pub static AF_XDP_FREE_PUSH_DROP_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn available() -> bool {
    cfg!(feature = "real")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smoke() {
        // smoke test should reflect whether the `real` feature is enabled
        assert_eq!(available(), cfg!(feature = "real"));
    }
}
