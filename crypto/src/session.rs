use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm};
use chacha20poly1305::ChaCha20Poly1305;
use hkdf::Hkdf;
use parking_lot::RwLock;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::convert::TryInto;
use thiserror::Error;

pub const TAG_SIZE: usize = 16;
pub const KEY_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    #[error("buffer too small")]
    BufferTooSmall,
    #[error("payload exceeds per-packet limit")]
    PayloadTooLarge,
    #[error("buffer lacks capacity for auth tag")]
    InsufficientCapacity,
    #[error("ciphertext shorter than tag size")]
    CiphertextTooShort,
    #[error("aead error")]
    AeadError,
    #[error("AEAD authentication failed")]
    AuthenticationFailed,
}

#[derive(Clone)]
struct CacheEntry {
    key: [u8; KEY_SIZE],
    nonce_base: [u8; NONCE_SIZE],
    seq_mask: u64,
}

type CacheMap = RwLock<HashMap<[u8; 32], CacheEntry>>;

fn derive_cache_key(combined_secret: &[u8], session_info: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(combined_secret);
    h.update(session_info);
    let sum = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&sum);
    out
}

static HKDF_CACHE: once_cell::sync::Lazy<CacheMap> = once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));

fn derive_session_material(combined_secret: &[u8], session_info: &[u8]) -> Result<CacheEntry, SessionError> {
    let label = b"smip-mwp-session-v1";
    let mut info = Vec::with_capacity(label.len() + session_info.len());
    info.extend_from_slice(label);
    info.extend_from_slice(session_info);

    let hk = Hkdf::<Sha256>::new(None, combined_secret);

    let mut key = [0u8; KEY_SIZE];
    hk.expand(&info, &mut key).map_err(|_| SessionError::AeadError)?;

    let hk2 = Hkdf::<Sha256>::new(None, &key);
    let mut nonce_base = [0u8; NONCE_SIZE];
    hk2.expand(b"nonce", &mut nonce_base).map_err(|_| SessionError::AeadError)?;
    let mut mask_bytes = [0u8; 8];
    hk2.expand(b"mask", &mut mask_bytes).map_err(|_| SessionError::AeadError)?;

    Ok(CacheEntry {
        key,
        nonce_base,
        seq_mask: u64::from_be_bytes(mask_bytes),
    })
}

enum SessionAead {
    Aes(Aes256Gcm),
    ChaCha(ChaCha20Poly1305),
}

impl SessionAead {
    fn new(key: &[u8; KEY_SIZE]) -> Result<Self, SessionError> {
        if let Ok(aes) = Aes256Gcm::new_from_slice(key) {
            return Ok(Self::Aes(aes));
        }
        let chacha = ChaCha20Poly1305::new_from_slice(key).map_err(|_| SessionError::AeadError)?;
        Ok(Self::ChaCha(chacha))
    }

    fn encrypt(&self, nonce: &[u8; NONCE_SIZE], plaintext: &[u8]) -> Result<Vec<u8>, SessionError> {
        match self {
            SessionAead::Aes(aead) => aead.encrypt(nonce.as_ref().into(), plaintext),
            SessionAead::ChaCha(aead) => aead.encrypt(nonce.as_ref().into(), plaintext),
        }
        .map_err(|_| SessionError::AuthenticationFailed)
    }

    fn decrypt(&self, nonce: &[u8; NONCE_SIZE], ciphertext: &[u8]) -> Result<Vec<u8>, SessionError> {
        match self {
            SessionAead::Aes(aead) => aead.decrypt(nonce.as_ref().into(), ciphertext),
            SessionAead::ChaCha(aead) => aead.decrypt(nonce.as_ref().into(), ciphertext),
        }
        .map_err(|_| SessionError::AuthenticationFailed)
    }
}

pub fn prederive_session(combined_secret: &[u8], session_info: &[u8]) -> Result<(), SessionError> {
    if combined_secret.is_empty() || session_info.is_empty() {
        return Err(SessionError::BufferTooSmall);
    }
    let cache_key = derive_cache_key(combined_secret, session_info);
    if HKDF_CACHE.read().contains_key(&cache_key) {
        return Ok(());
    }
    let entry = derive_session_material(combined_secret, session_info)?;
    HKDF_CACHE.write().insert(cache_key, entry);
    Ok(())
}

pub struct HybridSession {
    aead: SessionAead,
    nonce_base: [u8; NONCE_SIZE],
    seq_mask: u64,
}

impl HybridSession {
    pub fn new(combined_secret: &[u8], session_info: &[u8]) -> Result<Self, SessionError> {
        let cache_key = derive_cache_key(combined_secret, session_info);
        if let Some(entry) = HKDF_CACHE.read().get(&cache_key).cloned() {
            return Ok(Self {
                aead: SessionAead::new(&entry.key)?,
                nonce_base: entry.nonce_base,
                seq_mask: entry.seq_mask,
            });
        }

        let entry = derive_session_material(combined_secret, session_info)?;
        let aead = SessionAead::new(&entry.key)?;
        HKDF_CACHE.write().insert(cache_key, entry.clone());

        Ok(Self {
            aead,
            nonce_base: entry.nonce_base,
            seq_mask: entry.seq_mask,
        })
    }

    fn build_nonce(&self, seq: u64) -> [u8; NONCE_SIZE] {
        let mut nonce = self.nonce_base;
        let existing = u64::from_be_bytes(nonce[4..12].try_into().unwrap());
        let mixed = existing ^ seq ^ self.seq_mask;
        nonce[4..12].copy_from_slice(&mixed.to_be_bytes());
        nonce
    }

    pub fn encrypt_in_place(&self, payload: &mut Vec<u8>, seq: u64) -> Result<(), SessionError> {
        if payload.len() > (1 << 24) {
            return Err(SessionError::PayloadTooLarge);
        }
        let ct = self.encrypt(payload, seq)?;
        *payload = ct;
        Ok(())
    }

    pub fn encrypt_to(&self, dst: &mut Vec<u8>, plaintext: &[u8], seq: u64) -> Result<(), SessionError> {
        if dst.capacity() < plaintext.len() + TAG_SIZE {
            return Err(SessionError::InsufficientCapacity);
        }
        let ct = self.encrypt(plaintext, seq)?;
        dst.clear();
        dst.extend_from_slice(&ct);
        Ok(())
    }

    pub fn decrypt_in_place(&self, payload: &mut Vec<u8>, seq: u64) -> Result<(), SessionError> {
        if payload.len() < TAG_SIZE {
            return Err(SessionError::CiphertextTooShort);
        }
        let pt = self.decrypt(payload, seq)?;
        *payload = pt;
        Ok(())
    }

    pub fn encrypt(&self, plaintext: &[u8], seq: u64) -> Result<Vec<u8>, SessionError> {
        if plaintext.len() > (1 << 24) {
            return Err(SessionError::PayloadTooLarge);
        }
        let nonce = self.build_nonce(seq);
        self.aead.encrypt(&nonce, plaintext)
    }

    pub fn decrypt(&self, ciphertext: &[u8], seq: u64) -> Result<Vec<u8>, SessionError> {
        if ciphertext.len() < TAG_SIZE {
            return Err(SessionError::CiphertextTooShort);
        }
        let nonce = self.build_nonce(seq);
        self.aead.decrypt(&nonce, ciphertext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_cache() {
        let combined = vec![0x42u8; 64];
        let info = b"session-info";
        prederive_session(&combined, info).expect("prederive");
        let sess = HybridSession::new(&combined, info).expect("session");
        let ct = sess.encrypt(b"hello", 1).expect("encrypt");
        let pt = sess.decrypt(&ct, 1).expect("decrypt");
        assert_eq!(pt, b"hello");
    }

    #[test]
    fn in_place_and_short_ciphertext_checks() {
        let combined = vec![0x24u8; 64];
        let info = b"inplace-info";
        let sess = HybridSession::new(&combined, info).expect("session");
        let mut payload = b"payload".to_vec();
        sess.encrypt_in_place(&mut payload, 7).expect("encrypt in place");
        let sess2 = HybridSession::new(&combined, info).expect("session2");
        let pt = sess2.decrypt(&payload, 7).expect("decrypt");
        assert_eq!(pt, b"payload");
        assert!(matches!(sess2.decrypt(&[1, 2, 3], 0), Err(SessionError::CiphertextTooShort)));
    }
}
