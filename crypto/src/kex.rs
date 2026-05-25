//! Hybrid x25519 + ML-KEM-768 key exchange.
//!
//! Protocol (two-phase, initiator/responder):
//!
//! ```text
//! Initiator                                 Responder
//! ─────────────────────────────────────────────────────
//! HybridKEX::new() → kex
//! kex.public_key() ──────────────────────▶  initiator_pub (32 + 1184 bytes)
//!                                            HybridKEX::new() → kex
//!                                            kex.respond(&initiator_pub)
//!                                              → (responder_msg, shared_secret)
//! responder_msg   ◀──────────────────────    send responder_msg (32 + 1088 bytes)
//! kex.finish(&responder_msg)
//!   → shared_secret
//! ```
//!
//! Both peers derive the same 64-byte session secret via a two-stage HKDF combiner
//! keyed by the handshake transcript (prevents cross-session attacks without a salt
//! round-trip):
//!
//! ```text
//! transcript     = SHA-256(x25519_init_pub || x25519_resp_pub || mlkem_init_pub || mlkem_ct)
//! prk_classical  = HKDF-Extract(transcript, x25519_ss)
//! prk_pqc        = HKDF-Extract(transcript, mlkem_ss)
//! combined_prk   = HKDF-Extract(transcript, prk_classical || prk_pqc)
//! session_secret = HKDF-Expand(combined_prk, "smip-mwp-kex-v1", 64)
//! ```

use hkdf::Hkdf;
use ml_kem::{KemCore, MlKem768, MlKem768Params, EncodedSizeUser};
use ml_kem::kem::{EncapsulationKey, DecapsulationKey};
use kem::{Encapsulate, Decapsulate};
use rand_core::OsRng;
use sha2::Sha256;
use x25519_dalek::{EphemeralSecret, PublicKey as X25519PublicKey};
use zeroize::Zeroize;

/// Wire sizes for MlKem768.
pub const MLKEM768_EK_LEN: usize = 1184;
pub const MLKEM768_CT_LEN: usize = 1088;
pub const MLKEM768_SS_LEN: usize = 32;
/// Combined public key wire size: x25519_pub (32) || mlkem_pub (1184).
pub const INITIATOR_PUB_LEN: usize = 32 + MLKEM768_EK_LEN;
/// Responder message wire size: x25519_resp_pub (32) || mlkem_ciphertext (1088).
pub const RESPONDER_MSG_LEN: usize = 32 + MLKEM768_CT_LEN;
/// Session secret length in bytes.
pub const SESSION_SECRET_LEN: usize = 64;

/// Errors returned by [`HybridKEX`] operations.
#[derive(Debug)]
pub enum KexError {
    KeyGenFailed,
    BadInitiatorPub,
    BadResponderMsg,
    EncapsulateFailed,
    DecapsulateFailed,
    HkdfExpandFailed,
    BadPublicKey(String),
}

impl std::fmt::Display for KexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KexError::KeyGenFailed     => write!(f, "kex: key generation failed"),
            KexError::BadInitiatorPub  => write!(f, "kex: initiator public key invalid"),
            KexError::BadResponderMsg  => write!(f, "kex: responder message invalid"),
            KexError::EncapsulateFailed => write!(f, "kex: ml-kem encapsulation failed"),
            KexError::DecapsulateFailed => write!(f, "kex: ml-kem decapsulation failed"),
            KexError::HkdfExpandFailed => write!(f, "kex: hkdf expand failed"),
            KexError::BadPublicKey(s)  => write!(f, "kex: bad public key: {s}"),
        }
    }
}

impl std::error::Error for KexError {}

/// Hybrid x25519 + ML-KEM-768 key exchange.
///
/// Holds ephemeral key material for one handshake.
/// After the handshake completes, private key bytes are zeroized on drop.
pub struct HybridKEX {
    /// Ephemeral x25519 private key — consumed during DH.
    x25519_priv: Option<EphemeralSecret>,
    x25519_pub:  X25519PublicKey,

    /// ML-KEM-768 decapsulation key (initiator side).
    mlkem_dk: DecapsulationKey<MlKem768Params>,
    /// ML-KEM-768 encapsulation key (shared as public key).
    mlkem_ek: EncapsulationKey<MlKem768Params>,
}

impl HybridKEX {
    /// Generate a fresh ephemeral keypair.
    pub fn new() -> Result<Self, KexError> {
        let x25519_priv = EphemeralSecret::random_from_rng(OsRng);
        let x25519_pub  = X25519PublicKey::from(&x25519_priv);

        let (mlkem_dk, mlkem_ek) = MlKem768::generate(&mut OsRng);

        Ok(Self {
            x25519_priv: Some(x25519_priv),
            x25519_pub,
            mlkem_dk,
            mlkem_ek,
        })
    }

    /// Returns the initiator's combined public key: x25519_pub (32) || mlkem_pub (1184).
    pub fn public_key(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(INITIATOR_PUB_LEN);
        out.extend_from_slice(self.x25519_pub.as_bytes());
        out.extend_from_slice(self.mlkem_ek.as_bytes().as_slice());
        out
    }

    /// **Responder side**: receive the initiator's public key, encapsulate, and derive the
    /// shared secret. Returns `(responder_msg, session_secret)`.
    ///
    /// `responder_msg` wire format: x25519_resp_pub (32) || mlkem_ciphertext (1088).
    pub fn respond(&mut self, initiator_pub: &[u8]) -> Result<(Vec<u8>, Vec<u8>), KexError> {
        if initiator_pub.len() < INITIATOR_PUB_LEN {
            return Err(KexError::BadInitiatorPub);
        }

        // --- Classical: X25519 DH ---
        let peer_x25519_bytes: [u8; 32] = initiator_pub[..32].try_into()
            .map_err(|_| KexError::BadInitiatorPub)?;
        let peer_x25519_pub = X25519PublicKey::from(peer_x25519_bytes);

        let x25519_priv = self.x25519_priv.take().ok_or(KexError::KeyGenFailed)?;
        let mut x25519_ss = x25519_priv.diffie_hellman(&peer_x25519_pub);

        // --- PQC: ML-KEM-768 encapsulation against initiator's public key ---
        let init_mlkem_pub_bytes = &initiator_pub[32..32 + MLKEM768_EK_LEN];
        let init_ek_arr: hybrid_array::Array<u8, _> =
            hybrid_array::Array::try_from(init_mlkem_pub_bytes)
                .map_err(|_| KexError::BadInitiatorPub)?;
        let init_ek = EncapsulationKey::<MlKem768Params>::from_bytes(&init_ek_arr);
        let (ct, mlkem_ss) = init_ek.encapsulate(&mut OsRng)
            .map_err(|_| KexError::EncapsulateFailed)?;
        let ct_bytes: Vec<u8> = ct.as_slice().to_vec();

        // Wire message: responder x25519 pub || mlkem ciphertext
        let mut msg = Vec::with_capacity(RESPONDER_MSG_LEN);
        msg.extend_from_slice(self.x25519_pub.as_bytes());
        msg.extend_from_slice(&ct_bytes);

        // Derive shared secret
        let transcript = build_transcript(
            &peer_x25519_bytes,
            self.x25519_pub.as_bytes(),
            init_mlkem_pub_bytes,
            &ct_bytes,
        );
        let ss = derive_session_secret(x25519_ss.as_bytes(), mlkem_ss.as_slice(), &transcript)?;

        // Zeroize x25519 shared secret bytes
        x25519_ss.zeroize();

        Ok((msg, ss))
    }

    /// **Initiator side**: receive the responder's message, decapsulate, and derive the
    /// matching shared secret.
    pub fn finish(&mut self, responder_msg: &[u8]) -> Result<Vec<u8>, KexError> {
        if responder_msg.len() < RESPONDER_MSG_LEN {
            return Err(KexError::BadResponderMsg);
        }

        // --- Classical: X25519 DH ---
        let resp_x25519_bytes: [u8; 32] = responder_msg[..32].try_into()
            .map_err(|_| KexError::BadResponderMsg)?;
        let resp_x25519_pub = X25519PublicKey::from(resp_x25519_bytes);

        let x25519_priv = self.x25519_priv.take().ok_or(KexError::KeyGenFailed)?;
        let mut x25519_ss = x25519_priv.diffie_hellman(&resp_x25519_pub);

        // --- PQC: ML-KEM-768 decapsulation ---
        let ct_bytes = &responder_msg[32..32 + MLKEM768_CT_LEN];
        let ct_arr: hybrid_array::Array<u8, _> = hybrid_array::Array::try_from(ct_bytes)
            .map_err(|_| KexError::BadResponderMsg)?;
        let mlkem_ss = self.mlkem_dk.decapsulate(&ct_arr)
            .map_err(|_| KexError::DecapsulateFailed)?;

        let init_mlkem_pub_bytes = self.mlkem_ek.as_bytes();
        let transcript = build_transcript(
            self.x25519_pub.as_bytes(),
            &resp_x25519_bytes,
            init_mlkem_pub_bytes.as_slice(),
            ct_bytes,
        );
        let ss = derive_session_secret(x25519_ss.as_bytes(), mlkem_ss.as_slice(), &transcript)?;

        x25519_ss.zeroize();
        Ok(ss)
    }
}

/// Derives a deterministic HKDF salt from the handshake transcript so both peers
/// use identical salt without a separate round-trip.
fn build_transcript(
    x25519_init_pub: &[u8],
    x25519_resp_pub: &[u8],
    mlkem_init_pub:  &[u8],
    mlkem_ct:        &[u8],
) -> [u8; 32] {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(x25519_init_pub);
    h.update(x25519_resp_pub);
    h.update(mlkem_init_pub);
    h.update(mlkem_ct);
    h.finalize().into()
}

/// Two-stage HKDF combiner producing a 64-byte session secret.
fn derive_session_secret(
    x25519_ss: &[u8],
    mlkem_ss:  &[u8],
    transcript: &[u8],
) -> Result<Vec<u8>, KexError> {
    // prk_classical = HKDF-Extract(transcript, x25519_ss)
    let (prk_classical, _) = Hkdf::<Sha256>::extract(Some(transcript), x25519_ss);
    // prk_pqc = HKDF-Extract(transcript, mlkem_ss)
    let (prk_pqc, _) = Hkdf::<Sha256>::extract(Some(transcript), mlkem_ss);

    // combined_prk = HKDF-Extract(transcript, prk_classical || prk_pqc)
    let mut combined = Vec::with_capacity(prk_classical.len() + prk_pqc.len());
    combined.extend_from_slice(&prk_classical);
    combined.extend_from_slice(&prk_pqc);
    let (prk_combined, _) = Hkdf::<Sha256>::extract(Some(transcript), &combined);

    // session_secret = HKDF-Expand(combined_prk, "smip-mwp-kex-v1", 64)
    let hkdf = Hkdf::<Sha256>::from_prk(&prk_combined)
        .map_err(|_| KexError::HkdfExpandFailed)?;
    let mut session_secret = vec![0u8; SESSION_SECRET_LEN];
    hkdf.expand(b"smip-mwp-kex-v1", &mut session_secret)
        .map_err(|_| KexError::HkdfExpandFailed)?;

    Ok(session_secret)
}

// Keep the type alias for compatibility with existing callers.
pub type HybridKeyExchange = HybridKEX;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hybrid_kex_full_round_trip() {
        // Initiator creates keypair
        let mut init = HybridKEX::new().expect("initiator keygen");
        let init_pub = init.public_key();
        assert_eq!(init_pub.len(), INITIATOR_PUB_LEN);

        // Responder receives initiator pubkey, encapsulates
        let mut resp = HybridKEX::new().expect("responder keygen");
        let (resp_msg, ss_resp) = resp.respond(&init_pub).expect("respond");
        assert_eq!(resp_msg.len(), RESPONDER_MSG_LEN);
        assert_eq!(ss_resp.len(), SESSION_SECRET_LEN);

        // Initiator receives responder message, decapsulates
        let ss_init = init.finish(&resp_msg).expect("finish");
        assert_eq!(ss_init.len(), SESSION_SECRET_LEN);

        // Both peers must derive the same secret
        assert_eq!(ss_init, ss_resp, "session secrets must match");
    }

    #[test]
    fn different_sessions_produce_different_secrets() {
        let mut i1 = HybridKEX::new().unwrap();
        let mut i2 = HybridKEX::new().unwrap();
        let mut r1 = HybridKEX::new().unwrap();
        let mut r2 = HybridKEX::new().unwrap();

        let (resp_msg1, ss1) = r1.respond(&i1.public_key()).unwrap();
        let (resp_msg2, ss2) = r2.respond(&i2.public_key()).unwrap();

        // Complete both handshakes
        let ss1_init = i1.finish(&resp_msg1).unwrap();
        let ss2_init = i2.finish(&resp_msg2).unwrap();

        // Each pair must agree
        assert_eq!(ss1, ss1_init);
        assert_eq!(ss2, ss2_init);
        // Two independent sessions must differ (overwhelming probability)
        assert_ne!(ss1, ss2, "independent sessions must produce distinct secrets");
    }

    #[test]
    fn bad_inputs_are_rejected() {
        let mut kex = HybridKEX::new().unwrap();
        assert!(kex.respond(&[0u8; 10]).is_err(), "short initiator pub should fail");

        let mut kex2 = HybridKEX::new().unwrap();
        let mut r = HybridKEX::new().unwrap();
        let pub2 = kex2.public_key();
        let _ = r.respond(&pub2).unwrap();
        // kex2's private key is consumed; finish with bad msg
        assert!(kex2.finish(&[0u8; 10]).is_err(), "short responder msg should fail");
    }
}
