// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright 2026 rwilliamspbg-ops (Translated to Rust)
// This file translates crypto/security_hardening.go, implementing critical security checks for SMIP operations.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::cmp::max;

// Global sequence counter, equivalent to Go's atomic.Uint64
static GLOBAL_SEQ_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Holds security parameters for SMIP operations.
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub max_replay_window: u64,
    // In Rust, time durations are handled with std::time::Duration
    pub handshake_timeout: std::time::Duration, 
    pub rate_limit_per_sec: i32,
}

/// Default security configuration values.
// NOTE TO IMPLEMENTER: The Go code used placeholder constants (MaxReplayWindow, HandshakeTimeout). 
// These must be defined as concrete defaults here.
pub const DEFAULT_SECURITY_CONFIG: SecurityConfig = SecurityConfig {
    max_replay_window: 1024, // Example default value
    handshake_timeout: std::time::Duration::from_secs(5), // Example default duration
    rate_limit_per_sec: 10_000_000, // Using a larger number for safety/placeholder
};

/// Checks if the sequence number has potentially wrapped around or exceeded the maximum allowed value.
pub fn check_sequence_number_overflow(seq: u64, max_seq: u64) -> bool {
    if max_seq == 0 {
        return false;
    }
    // Check if seq >= max_seq
    seq >= max_seq
}

/// Increments the global sequence counter and returns the new value.
pub fn increment_global_seq() -> u64 {
    GLOBAL_SEQ_COUNTER.fetch_add(1, Ordering::SeqCst) + 1
}

/// Rate limiting structure for DoS protection. Implements atomic tracking of last packet time.
pub struct DosThrottle {
    // last_packet_time: Unix timestamp in nanoseconds (Atomic for thread safety)
    last_packet_time: AtomicU64,
    rate_limit_ns: u64, // Time interval allowed between packets in nanoseconds
}

impl DosThrottle {
    /// Creates a new DoS throttle limiter based on the desired rate.
    pub fn new(rate_per_sec: i32) -> Self {
        let rate_limit_ns = 1_000_000_000 / (rate_per_sec as u64);
        DosThrottle {
            last_packet_time: AtomicU64::new(0), // Initial value 0 indicates never seen
            rate_limit_ns,
        }
    }

    /// Checks if packet processing is allowed under DoS protection.
    pub fn allow_packet(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_nanos() as u64;

        // Load the last seen time atomically
        let last_seen = self.last_packet_time.load(Ordering::Acquire);

        // If never seen, or if enough time has passed since the last packet:
        if last_seen == 0 || now.checked_sub(last_seen).unwrap_or(u64::MAX) >= self.rate_limit_ns {
            // Update and store the current time atomically
            self.last_packet_time.store(now, Ordering::Release);
            return true;
        }
        false
    }
}

// Note: In a larger application, you might implement Default for SecurityConfig 
// or use a dedicated configuration loader instead of hardcoding defaults.