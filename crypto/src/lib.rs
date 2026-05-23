pub mod kex;
pub mod security_hardening;
pub mod session;

pub use kex::HybridKEX;
pub use security_hardening::{
    check_sequence_number_overflow,
    increment_global_seq,
    DoSThrottle,
    HybridKEXState,
    SecurityConfig,
    SessionState,
    DEFAULT_SECURITY_CONFIG,
    HANDSHAKE_TIMEOUT,
    MAX_REPLAY_WINDOW,
};

#[cfg(test)]
mod tests {
    use crate::{DoSThrottle, HybridKEXState};
    use super::kex::*;
    use super::session::*;

    #[test]
    fn kex_and_session_flow() {
        let k = HybridKEX::new().expect("kex new");
        let pubk = k.public_key();
        let combined = k.handshake(&pubk).expect("handshake");
        let sess = HybridSession::new(&combined, b"session-info").expect("session");
        let ct = sess.encrypt(b"hello", 1).expect("encrypt");
        let pt = sess.decrypt(&ct, 1).expect("decrypt");
        assert_eq!(pt, b"hello");
    }

    #[test]
    fn security_state_and_throttle_flow() {
        let mut state = HybridKEXState::new([7u8; 16]);
        assert!(state.check_timeout().is_ok());
        let seq = state.increment_seq_counter().expect("seq");
        assert_eq!(seq, 1);
        state.reset_retry();
        assert!(state.check_retries().is_ok());

        let throttle = DoSThrottle::new(1_000_000);
        assert!(throttle.allow_packet());
    }
}
