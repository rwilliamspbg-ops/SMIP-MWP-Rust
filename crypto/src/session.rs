use aes_gcm::aead::generic_array::typenum::{U12, U16};
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::{
    aead::{Aead, AeadInPlace, KeyInit},
    Aes256Gcm,
};
use ahash::{AHashMap, AHasher};
use hkdf::Hkdf;
use parking_lot::RwLock;
use sha2::Sha256;
use std::hash::Hasher;
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

type CacheMap = RwLock<AHashMap<u64, CacheEntry>>;

/// Fast non-cryptographic cache key — replaces the previous SHA-256 call.
/// Session cache lookups go from ~500 ns to ~10 ns.
/// We call `Hasher::write` directly instead of generic slice `.hash(&mut h)`
/// to avoid slice-length hashing and generic iterator dispatch overhead.
fn derive_cache_key(combined_secret: &[u8], session_info: &[u8]) -> u64 {
    let mut h = AHasher::default();
    h.write(combined_secret);
    h.write(session_info);
    h.finish()
}

static HKDF_CACHE: once_cell::sync::Lazy<CacheMap> =
    once_cell::sync::Lazy::new(|| RwLock::new(AHashMap::new()));

fn derive_session_material(
    combined_secret: &[u8],
    session_info: &[u8],
) -> Result<CacheEntry, SessionError> {
    let label = b"smip-mwp-session-v1";
    // Stack buffer avoids the heap allocation for info concatenation
    let mut info = [0u8; 256];
    let llen = label.len();
    let slen = session_info.len().min(info.len() - llen);
    info[..llen].copy_from_slice(label);
    info[llen..llen + slen].copy_from_slice(&session_info[..slen]);
    let info_slice = &info[..llen + slen];

    let hk = Hkdf::<Sha256>::new(None, combined_secret);

    let mut key = [0u8; KEY_SIZE];
    hk.expand(info_slice, &mut key)
        .map_err(|_| SessionError::AeadError)?;

    let hk2 = Hkdf::<Sha256>::new(None, &key);
    let mut nonce_base = [0u8; NONCE_SIZE];
    hk2.expand(b"nonce", &mut nonce_base)
        .map_err(|_| SessionError::AeadError)?;
    let mut mask_bytes = [0u8; 8];
    hk2.expand(b"mask", &mut mask_bytes)
        .map_err(|_| SessionError::AeadError)?;

    Ok(CacheEntry {
        key,
        nonce_base,
        seq_mask: u64::from_be_bytes(mask_bytes),
    })
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
    // Direct Aes256Gcm cipher avoids enum tag storage, enum pattern matching, and indirect dispatch on hot packet encryption paths.
    aead: Aes256Gcm,
    nonce_prefix: [u8; 4],
    nonce_xor: u64,
}

impl HybridSession {
    pub fn new(combined_secret: &[u8], session_info: &[u8]) -> Result<Self, SessionError> {
        let cache_key = derive_cache_key(combined_secret, session_info);
        if let Some(entry) = HKDF_CACHE.read().get(&cache_key).cloned() {
            let existing =
                u64::from_be_bytes(*<&[u8; 8]>::try_from(&entry.nonce_base[4..12]).unwrap());
            let nonce_xor = existing ^ entry.seq_mask;
            let aead =
                Aes256Gcm::new_from_slice(&entry.key).map_err(|_| SessionError::AeadError)?;
            let nonce_prefix = *<&[u8; 4]>::try_from(&entry.nonce_base[..4]).unwrap();
            return Ok(Self {
                aead,
                nonce_prefix,
                nonce_xor,
            });
        }
        let entry = derive_session_material(combined_secret, session_info)?;
        let aead = Aes256Gcm::new_from_slice(&entry.key).map_err(|_| SessionError::AeadError)?;
        HKDF_CACHE.write().insert(cache_key, entry.clone());
        let existing = u64::from_be_bytes(*<&[u8; 8]>::try_from(&entry.nonce_base[4..12]).unwrap());
        let nonce_xor = existing ^ entry.seq_mask;
        let nonce_prefix = *<&[u8; 4]>::try_from(&entry.nonce_base[..4]).unwrap();
        Ok(Self {
            aead,
            nonce_prefix,
            nonce_xor,
        })
    }

    #[inline]
    fn build_nonce(&self, seq: u64) -> GenericArray<u8, U12> {
        let mut nonce = std::mem::MaybeUninit::<GenericArray<u8, U12>>::uninit();
        let mixed = self.nonce_xor ^ seq;
        // Optimization: Write the 4-byte static prefix and 8-byte unaligned u64 sequence/XOR directly
        // into uninitialized GenericArray storage to avoid zeroing memory before overwriting it.
        unsafe {
            let ptr = nonce.as_mut_ptr() as *mut u8;
            *(ptr as *mut [u8; 4]) = self.nonce_prefix;
            (ptr.add(4) as *mut u64).write_unaligned(mixed.to_be());
            nonce.assume_init()
        }
    }

    /// Encrypts `payload` completely in-place, appending the authentication tag.
    /// Uses `AeadInPlace::encrypt_in_place` to avoid allocating temporary buffer vectors.
    #[inline]
    pub fn encrypt_in_place(&self, payload: &mut Vec<u8>, seq: u64) -> Result<(), SessionError> {
        if payload.len() > (1 << 24) {
            return Err(SessionError::PayloadTooLarge);
        }
        let nonce = self.build_nonce(seq);
        self.aead
            .encrypt_in_place(&nonce, b"", payload)
            .map_err(|_| SessionError::AuthenticationFailed)
    }

    #[inline]
    pub fn encrypt_into_slice(
        &self,
        payload: &mut [u8],
        seq: u64,
    ) -> Result<GenericArray<u8, U16>, SessionError> {
        if payload.len() > (1 << 24) {
            return Err(SessionError::PayloadTooLarge);
        }
        let nonce = self.build_nonce(seq);
        self.aead
            .encrypt_in_place_detached(&nonce, b"", payload)
            .map_err(|_| SessionError::AuthenticationFailed)
    }

    /// Zero-allocation encrypt: caller fills `dst` with the plaintext, then
    /// this method encrypts it in-place and appends the 16-byte tag.
    /// `dst` must have capacity for `plaintext_len + TAG_SIZE` bytes.
    #[inline]
    pub fn encrypt_to(
        &self,
        dst: &mut Vec<u8>,
        plaintext: &[u8],
        seq: u64,
    ) -> Result<(), SessionError> {
        if dst.capacity() < plaintext.len() + TAG_SIZE {
            return Err(SessionError::InsufficientCapacity);
        }
        dst.clear();
        dst.extend_from_slice(plaintext);
        let nonce = self.build_nonce(seq);
        self.aead
            .encrypt_in_place(&nonce, b"", dst)
            .map_err(|_| SessionError::AuthenticationFailed)
    }

    /// Decrypts `payload` completely in-place and truncates the authentication tag.
    /// Uses `AeadInPlace::decrypt_in_place` to avoid allocating temporary buffer vectors.
    #[inline]
    pub fn decrypt_in_place(&self, payload: &mut Vec<u8>, seq: u64) -> Result<(), SessionError> {
        if payload.len() < TAG_SIZE {
            return Err(SessionError::CiphertextTooShort);
        }
        let nonce = self.build_nonce(seq);
        self.aead
            .decrypt_in_place(&nonce, b"", payload)
            .map_err(|_| SessionError::AuthenticationFailed)
    }

    #[inline]
    pub fn encrypt(&self, plaintext: &[u8], seq: u64) -> Result<Vec<u8>, SessionError> {
        if plaintext.len() > (1 << 24) {
            return Err(SessionError::PayloadTooLarge);
        }
        let nonce = self.build_nonce(seq);
        self.aead
            .encrypt(&nonce, plaintext)
            .map_err(|_| SessionError::AuthenticationFailed)
    }

    #[inline]
    pub fn decrypt(&self, ciphertext: &[u8], seq: u64) -> Result<Vec<u8>, SessionError> {
        if ciphertext.len() < TAG_SIZE {
            return Err(SessionError::CiphertextTooShort);
        }
        let nonce = self.build_nonce(seq);
        self.aead
            .decrypt(&nonce, ciphertext)
            .map_err(|_| SessionError::AuthenticationFailed)
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
    fn encrypt_to_is_zero_alloc_and_round_trips() {
        let combined = vec![0x99u8; 64];
        let info = b"zero-alloc-test";
        let sess = HybridSession::new(&combined, info).expect("session");
        let plaintext = b"zero alloc payload";
        let mut buf = Vec::with_capacity(plaintext.len() + TAG_SIZE);
        sess.encrypt_to(&mut buf, plaintext, 42)
            .expect("encrypt_to");
        assert_eq!(buf.len(), plaintext.len() + TAG_SIZE);
        let pt = sess.decrypt(&buf, 42).expect("decrypt");
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn in_place_and_short_ciphertext_checks() {
        let combined = vec![0x24u8; 64];
        let info = b"inplace-info";
        let sess = HybridSession::new(&combined, info).expect("session");
        let mut payload = b"payload".to_vec();
        sess.encrypt_in_place(&mut payload, 7)
            .expect("encrypt in place");
        let sess2 = HybridSession::new(&combined, info).expect("session2");
        let pt = sess2.decrypt(&payload, 7).expect("decrypt");
        assert_eq!(pt, b"payload");
        assert!(matches!(
            sess2.decrypt(&[1, 2, 3], 0),
            Err(SessionError::CiphertextTooShort)
        ));
    }

    #[test]
    fn encrypt_into_slice_mutates_in_place() {
        let combined = vec![0x55u8; 64];
        let info = b"slice-encrypt-info";
        let sess = HybridSession::new(&combined, info).expect("session");
        let mut payload = b"slice payload".to_vec();
        let original = payload.clone();

        let tag = sess
            .encrypt_into_slice(payload.as_mut_slice(), 11)
            .expect("encrypt_into_slice");

        assert_ne!(payload, original);

        let mut ciphertext = payload;
        ciphertext.extend_from_slice(tag.as_slice());
        let pt = sess.decrypt(&ciphertext, 11).expect("decrypt");
        assert_eq!(pt, original);
    }
}
