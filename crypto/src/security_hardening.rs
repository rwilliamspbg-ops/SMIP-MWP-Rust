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
    seq_window: std::collections::BTreeSet<u64>,
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
            seq_window: std::collections::BTreeSet::new(),
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
        if self.seq_window.contains(&seq) {
            return Err(format!(
                "crypto: replay attack detected for session {:02x?}",
                self.session_id
            ));
        }
        while self.seq_window.len() as u64 >= MAX_REPLAY_WINDOW {
            if let Some(oldest) = self.seq_window.iter().next().copied() {
                self.seq_window.remove(&oldest);
            } else {
                break;
            }
        }
        self.seq_window.insert(seq);
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
}
