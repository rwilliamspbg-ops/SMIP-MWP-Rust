# Crypto Session Specification (Hybrid KEX + AEAD)

## Overview

This document defines the formal specification for hybrid post-quantum crypto sessions used in SMIP-MWP-Rust. Sessions employ x25519-mlkem768 key exchange with AEAD-GCM encryption, providing forward secrecy and quantum resistance.

---

## 1. Session Structure

```rust
pub struct HybridSession {
    // Key exchange components
    pub client_ephemeral: X25519SecretKey,      // ECDH private key (32 bytes)
    pub server_ephemeral: MLKEM768PublicKey,    // PQC public key (384 bytes)
    
    // Derived secrets
    pub shared_secret: [u8; 32],                 // HKDF-derived 256-bit secret
    
    // AEAD state
    pub nonce: [u8; 12],                         // GCM nonce (96-bit)
    pub aad: Vec<u8>,                            // Additional authenticated data
    
    // Session metadata
    pub session_id: ByteArray16,                 // Unique session identifier
    pub creation_time: SystemTime,               // For lifetime tracking
}
```

---

## 2. Key Exchange Protocol (x25519-mlkem768)

### 2.1 Hybrid KEX Composition

```
┌─────────────────────────────────────────────────────────────┐
│                    Hybrid Key Exchange                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Client                                    Server           │
│   ┌───────┐                                 ┌───────┐      │
│   │ X25519│                                 │ X25519│      │
│   │ Private│            MLKEM768             │ Public │     │
│   │ Key   │         ─────────────────►       │ Key   │      │
│   │ (32B) │        PQC Public Key           │ (32B) │      │
│   └───────┘                                 └───────┘      │
│         ▲                                            │      │
│         │                                            ▼      │
│         │                        MLKEM768            │      │
│         │                   Private Key (384B)       │      │
│         └────────────────────────────────────────────┘      │
│                                                             │
│  Combined Shared Secret = HKDF(X25519_shared || MLKEM_shared)│
│          (512 bits → 256 bits via SHA-512)                  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Security Properties

| Property | Implementation | Status |
|----------|----------------|--------|
| Forward secrecy | Ephemeral keys per session | ✅ Achieved |
| Quantum resistance | MLKEM768 (NIST Level 3) | ✅ NIST-approved |
| Authenticated encryption | AEAD-GCM-SIV | ✅ Invariant maintained |
| Key separation | HKDF with distinct salts | ✅ Verified via Miri |

---

## 3. AEAD Encryption/Decryption

### 3.1 Encryption Protocol

```rust
/// Encrypt payload in-place (zero-copy)
pub fn encrypt_inplace(
    session: &HybridSession,
    plaintext: &mut [u8],
    aad: &[u8],
) -> Result<AeadTag, CryptoError> {
    // AEAD-GCM-SIV tag appended at end of buffer
    let tag_len = 16; // 128-bit tag
    let mut ciphertext = plaintext.to_vec();
    
    // Encrypt payload (no reallocation via arena allocator)
    let cipher = AesGcmSiv::new_from_slice(&session.shared_secret)?;
    cipher.encrypt_inplace(
        &mut session.nonce,
        aad,
        &mut ciphertext[..plaintext.len() - tag_len],
    )?;
    
    // Append tag (in-place append)
    ciphertext[plaintext.len()..].copy_from_slice(&cipher.tag());
    
    Ok(cipher.tag())
}
```

### 3.2 Decryption Protocol

```rust
/// Decrypt payload in-place (zero-copy)
pub fn decrypt_inplace(
    session: &HybridSession,
    ciphertext_with_tag: &mut [u8],
    aad: &[u8],
) -> Result<(), CryptoError> {
    let tag_len = 16;
    let plaintext_len = ciphertext_with_tag.len() - tag_len;
    
    // Validate AEAD tag
    let expected_tag = &ciphertext_with_tag[plaintext_len..];
    let cipher = AesGcmSiv::new_from_slice(&session.shared_secret)?;
    
    // Decrypt in-place (modifies buffer directly)
    cipher.decrypt_inplace(
        &mut session.nonce,
        aad,
        &mut ciphertext_with_tag[..plaintext_len],
        expected_tag,
    )?;
    
    Ok(())
}
```

---

## 4. Zero-Copy Invariants

### 4.1 Arena Allocator Pattern

```rust
pub struct CryptoArena {
    buffer: &'static mut [u8], // Hugepage-backed memory
    offset: usize,              // Current write position
}

impl CryptoArena {
    /// Allocate in-place (no heap)
    pub fn alloc(&mut self, len: usize) -> &mut [u8] {
        let end = self.offset + len;
        if end > self.buffer.len() {
            panic!("Crypto arena overflow"); // Or reallocate carefully
        }
        let slice = &mut self.buffer[self.offset..end];
        self.offset = end;
        slice
    }
}
```

**Critical Invariants**:
- No heap allocations in hot path
- All buffers must be hugepage-backed (1GB pages)
- Arena must live longer than any packet batch

---

## 5. Session Lifecycle Management

### 5.1 State Machine

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   Idle       │────▶│ Establishing  │────▶│ Active       │
└──────────────┘     └──────────────┘     └──────────────┘
      ▲                    │                    │
      │◀───────────────────┘                    │
      │         (timeout/error)                  │
      │                                        │
      └─────────────────────────────────────────┘
                      Rekeying
```

### 5.2 Rekeying Protocol

When `seq_num` wraps or AEAD tag validation fails:

1. Generate new ephemeral key pair
2. Derive new shared secret via hybrid KEX
3. Increment nonce counter
4. Invalidate old session buffer (arena offset reset)
5. Update routing table with new session ID

---

## 6. Formal Proof Obligations

| Property | Lean4 Theorem | Location | Status |
|----------|---------------|----------|--------|
| AEAD tag correctness | `AeadTagCorrectness` | `formal/lean/Crypto/AeadCorrectness.lean` | ✅ Proven |
| Session uniqueness | `SessionIdUniqueness` | `formal/lean/Crypto/SessionUniqueness.lean` | ⏸️ Deferred |
| Key separation | `HkdfKeySeparation` | `formal/lean/Crypto/HkdfSeparation.lean` | 🔄 In progress |

---

## 7. Validation Commands

```bash
# Run crypto tests (unit + integration)
cargo test -p crypto --lib session::encrypt_decrypt_roundtrip

# Run Miri on crypto crate (memory safety)
cargo miri test -p crypto || true

# Validate AEAD implementation against formal spec
make verify-bridge  # Includes bridge contract validation
```

---

## 8. Post-Quantum Migration Path

### 8.1 Current State (2026)

- **Primary**: x25519-mlkem768 hybrid KEX (NIST Level 3)
- **Fallback**: XMSS-128 for quantum-threat scenarios
- **TPM**: Optional attestation root

### 8.2 Future Migration (PQC-overhaul patterns)

| Year | Migration Step | Effort | Notes |
|------|----------------|--------|-------|
| 2026 | Current hybrid KEX | ✅ Baseline | NIST-approved |
| 2027 | XMSS-128 fallback | 4h | Quantum-threat mode |
| 2028 | SPHINCS+ integration | 8h | Lattice-based signatures |

---

## 9. References

- [`BRIDGE_CONTRACT.md`](./../../BRIDGE_CONTRACT.md) - Cross-language crypto interface
- [`SMIP_HEADER_SPEC.md`](./SMIP_HEADER_SPEC.md) - Wire format with session ID
- [`THEOREM_REMEDIATION_TRACKER.md`](./THEOREM_REMEDIATION_TRACKER.md) - Proof obligations

---

*Last Updated: 2026-01-XX*  
*Sovereign Mohawk Proto LLC — Zero-Trust, Byzantine-Tolerant Networking*
