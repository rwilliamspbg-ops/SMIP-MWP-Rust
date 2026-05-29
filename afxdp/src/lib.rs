//! AF_XDP UMEM and socket abstractions.

pub mod rings;
pub mod socket;
pub mod umem;

pub use socket::{AfXdpSocket, MockSocket};

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// Global counters exported for diagnostics and metrics.
pub static AF_XDP_RETRY_COUNT: AtomicU64 = AtomicU64::new(0);
pub static AF_XDP_BACKPRESSURE_COUNT: AtomicU64 = AtomicU64::new(0);

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
