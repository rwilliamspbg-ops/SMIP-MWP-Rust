use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_REPLAY_WINDOW: u64 = 64;

static GLOBAL_SEQ_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityConfig {
    pub max_replay_window: u64,
    pub handshake_timeout: Duration,
    pub rate_limit_per_sec: i32,
}

pub const DEFAULT_SECURITY_CONFIG: SecurityConfig = SecurityConfig {
    max_replay_window: MAX_REPLAY_WINDOW,
    handshake_timeout: HANDSHAKE_TIMEOUT,
    rate_limit_per_sec: 10_000_000,
};

pub fn check_sequence_number_overflow(seq: u64, max_seq: u64) -> bool {
    if max_seq == 0 {
        return false;
    }
    seq >= max_seq
}

pub fn increment_global_seq() -> u64 {
    GLOBAL_SEQ_COUNTER.fetch_add(1, Ordering::SeqCst) + 1
}

#[derive(Debug)]
pub struct DoSThrottle {
    last_packet_time: AtomicI64,
    rate_limit_ns: i64,
    window_ns: i64,
}

impl DoSThrottle {
    pub fn new(rate_per_sec: i32) -> Self {
        let safe_rate = rate_per_sec.max(1) as i64;
        Self {
            last_packet_time: AtomicI64::new(0),
            rate_limit_ns: 1_000_000_000_i64 / safe_rate,
            window_ns: 1_000_000_000,
        }
    }

    pub fn allow_packet(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        let last_seen = self.last_packet_time.load(Ordering::Acquire);
        let elapsed = now.saturating_sub(last_seen);
        if last_seen == 0 || elapsed >= self.rate_limit_ns.min(self.window_ns) {
            self.last_packet_time.store(now, Ordering::Release);
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Uninitialized,
    AwaitingPeerPubkey,
    ReadyForAuth,
    Established,
    TimedOut,
}

#[derive(Debug)]
pub struct HybridKEXState {
    pub session_id: [u8; 16],
    pub kex_started: SystemTime,
    pub timeout: SystemTime,
    pub retry_count: usize,
    pub handshake_done: bool,
    pub seq_counter: u64,
    seq_window: [u64; MAX_REPLAY_WINDOW as usize],
    seq_window_len: usize,
}

impl HybridKEXState {
    pub fn new(session_id: [u8; 16]) -> Self {
        let now = SystemTime::now();
        Self {
            session_id,
            kex_started: now,
            timeout: now + HANDSHAKE_TIMEOUT,
            retry_count: 0,
            handshake_done: false,
            seq_counter: 0,
            seq_window: [0; MAX_REPLAY_WINDOW as usize],
            seq_window_len: 0,
        }
    }

    pub fn check_timeout(&mut self) -> Result<(), String> {
        if !self.handshake_done && SystemTime::now() > self.timeout {
            return Err(format!(
                "crypto: handshake timeout for session {:02x?}",
                self.session_id
            ));
        }
        if !self.handshake_done {
            self.timeout = SystemTime::now() + HANDSHAKE_TIMEOUT;
        }
        Ok(())
    }

    pub fn increment_seq_counter(&mut self) -> Result<u64, String> {
        self.seq_counter = self.seq_counter.saturating_add(1);
        let seq = self.seq_counter;

        // binary_search expects the slice to be sorted.
        // We maintain seq_window in sorted order.
        let window = &self.seq_window[..self.seq_window_len];
        if window.binary_search(&seq).is_ok() {
            return Err(format!(
                "crypto: replay attack detected for session {:02x?}",
                self.session_id
            ));
        }

        if self.seq_window_len >= MAX_REPLAY_WINDOW as usize {
            // Reached window capacity. Evict the oldest (smallest) sequence number.
            // Since seq_window is sorted, the smallest is at index 0.
            // Shift remaining elements left to overwrite index 0.
            self.seq_window.copy_within(1..self.seq_window_len, 0);
            self.seq_window_len -= 1;
        }

        // Insert seq in sorted order. Since seq is strictly increasing (under normal increments),
        // we can check if it is greater than the last element and append, or perform binary search and insert.
        let insert_idx = match self.seq_window[..self.seq_window_len].binary_search(&seq) {
            Ok(idx) => idx,
            Err(idx) => idx,
        };

        if insert_idx < self.seq_window_len {
            self.seq_window
                .copy_within(insert_idx..self.seq_window_len, insert_idx + 1);
        }
        self.seq_window[insert_idx] = seq;
        self.seq_window_len += 1;

        Ok(seq)
    }

    pub fn check_retries(&mut self) -> Result<(), String> {
        if self.retry_count >= 3 {
            return Err(format!(
                "crypto: handshake retry limit exceeded for session {:02x?}",
                self.session_id
            ));
        }
        self.retry_count += 1;
        Ok(())
    }

    pub fn reset_retry(&mut self) {
        self.retry_count = 0;
    }

    pub fn cleanup(&mut self) {
        self.kex_started = SystemTime::UNIX_EPOCH;
        self.timeout = SystemTime::UNIX_EPOCH;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_and_throttle_helpers_work() {
        assert!(!check_sequence_number_overflow(1, 0));
        assert!(check_sequence_number_overflow(10, 10));
        assert!(increment_global_seq() > 0);

        let throttle = DoSThrottle::new(1_000_000);
        assert!(throttle.allow_packet());
    }

    #[test]
    fn kex_state_tracks_retries_and_sequences() {
        let mut state = HybridKEXState::new([1u8; 16]);
        assert!(state.check_timeout().is_ok());
        assert_eq!(state.increment_seq_counter().unwrap(), 1);
        assert!(state.check_retries().is_ok());
        state.reset_retry();
        assert!(state.check_retries().is_ok());
        state.cleanup();
        assert_eq!(state.kex_started, SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn test_kex_state_sliding_window_eviction_and_replay_detection() {
        let mut state = HybridKEXState::new([2u8; 16]);

        // Fill the window up to max replay window size (64)
        for i in 1..=MAX_REPLAY_WINDOW {
            let seq = state.increment_seq_counter().unwrap();
            assert_eq!(seq, i);
        }

        // Try to increment again, which will trigger eviction of '1' because we reached capacity
        let next_seq = state.increment_seq_counter().unwrap();
        assert_eq!(next_seq, MAX_REPLAY_WINDOW + 1);

        // Check replay attack detection on the sliding window.
        // Let's manually trigger duplicates of already recorded sequences.
        // We can do this by setting state.seq_counter to something already in the window
        state.seq_counter = 10;
        assert!(state.increment_seq_counter().is_err());

        // Evicted '1' shouldn't cause replay detection anymore if we force seq_counter to 1
        state.seq_counter = 0;
        assert_eq!(state.increment_seq_counter().unwrap(), 1);
    }
}
