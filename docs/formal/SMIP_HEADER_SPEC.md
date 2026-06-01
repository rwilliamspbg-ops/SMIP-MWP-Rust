# SMIP Header Specification (Formal Wire Format)

## Overview

This document provides the formal wire format specification for Sovereign Mohawk Internet Protocol (SMIP) routing packets. All marshaling/unmarshaling must adhere to this specification to maintain zero-copy invariants and cross-language compatibility.

---

## 1. Header Layout

```
┌─────────────────────────────────────────────────────────────┐
│                   SMIP Header (96 bytes total)                 │
├─────────────────────────────────────────────────────────────┤
│ 0x00-0x1F (32B)         │ src_id: [byte]32                    │
├─────────────────────────────────────────────────────────────┤
│ 0x20-0x3F (32B)         │ dst_id: [byte]32                    │
├─────────────────────────────────────────────────────────────┤
│ 0x40-0x43 (4B)          │ flow_label: uint32                  │
├─────────────────────────────────────────────────────────────┤
│ 0x44-0x4B (8B)          │ seq_num: uint64                     │
├─────────────────────────────────────────────────────────────┤
│ 0x4C-0x59 (16B)         │ session_id: [byte]16                │
├─────────────────────────────────────────────────────────────┤
│ 0x5A-0x5D (2B)          │ flags: uint16                       │
├─────────────────────────────────────────────────────────────┤
│ 0x5E-0x5F (2B)          │ length: uint16                      │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Field Specifications

### 2.1 Source ID (`src_id`)
- **Type**: `[byte]32` (fixed array, 32 bytes)
- **Position**: Offset 0x00
- **Semantics**: Big-endian source identifier
- **Invariants**:
  - Non-empty: `∀ i ∈ [0..32), src_id[i] ∉ {}`
  - No zero-fill allowed: `src_id[0] ≠ 0x00` (reserved for null)
- **Marshaling**: Direct memory copy from Rust `[u8; 32]` to Go `[byte]32`

### 2.2 Destination ID (`dst_id`)
- **Type**: `[byte]32` (fixed array, 32 bytes)
- **Position**: Offset 0x20
- **Semantics**: Big-endian destination identifier
- **Invariants**:
  - Non-empty: `∀ i ∈ [0..32), dst_id[i] ∉ {}`
  - Must differ from `src_id`: `src_id ≠ dst_id` (no self-routing)

### 2.3 Flow Label (`flow_label`)
- **Type**: `uint32`
- **Position**: Offset 0x40
- **Semantics**: Flow classification tag for ECMP selection
- **Range**: `[0, 2^32 - 1]`
- **Special Values**:
  - `0`: Reserved (unclassified flow)
  - `[1, 7]`: Reserved for protocol use (reserved flags)

### 2.4 Sequence Number (`seq_num`)
- **Type**: `uint64`
- **Position**: Offset 0x44
- **Semantics**: Per-flow sequence number for reordering detection
- **Range**: `[0, 2^64 - 1]`
- **Overflow Policy**: Wraparound at 2^64 (no wrap detection in current impl)

### 2.5 Session ID (`session_id`)
- **Type**: `[byte]16` (fixed array, 16 bytes)
- **Position**: Offset 0x4C
- **Semantics**: Hybrid crypto session identifier
- **Invariants**:
  - Non-empty: `∀ i ∈ [0..16), session_id[i] ∉ {}`
  - Must match AEAD tag in encrypted payload

### 2.6 Flags (`flags`)
- **Type**: `uint16`
- **Position**: Offset 0x5A
- **Semantics**: Packet flags and options
- **Bit Layout**:
  ```
  Bit 0: ECMP_ENABLED (multi-path forwarding)
  Bit 1: ECN_ECT0 (ECN capability code point 0)
  Bit 2: ECN_ECT1 (ECN capability code point 1)
  Bit 3: CE_CE (Congestion Experienced)
  Bit 4+: Reserved (must be zero on write)
  ```
- **Invariants**:
  - Reserved bits must be zero: `∀ i ∈ [4..16), flags[i] = 0`

### 2.7 Length (`length`)
- **Type**: `uint16`
- **Position**: Offset 0x5E
- **Semantics**: Encrypted payload length (excluding AEAD tag)
- **Range**: `[0, 65535]`
- **Invariant**: Must match actual payload length in ring buffer

---

## 3. Marshaling Rules

### 3.1 Rust → Go Marshaling

```rust
// Rust: Zero-copy view over UMEM region
pub struct HeaderViewRef<'a> {
    pub buffer: &'a [u8],
    pub offset: usize,
}

impl<'a> HeaderViewRef<'a> {
    /// Marshal header to byte slice (returns owned copy)
    pub fn marshal(&self) -> Vec<u8> {
        // Copy 96 bytes from buffer[header_offset..]
        let mut buf = [0u8; 96];
        buf.copy_from_slice(&self.buffer[self.offset..]);
        buf
    }
}
```

**Preconditions**:
- `buffer.len() ≥ offset + 96` (sufficient space for header)
- No heap allocations during copy

### 3.2 Go → Rust Unmarshaling

```go
// Go: Unmarshal from byte slice into HeaderView
func wire.Unmarshal(header []byte, buffer *afxdp.RealSocket) error {
    if len(header) != 96 {
        return errors.New("invalid header length")
    }
    // Copy into ring buffer region
    buffer.WriteAt(header, offset)
    return nil
}
```

**Preconditions**:
- `len(header) == 96` (exact size match)
- Ring buffer has sufficient capacity

---

## 4. Zero-Copy Invariant

**Critical**: The header must be accessible without heap allocation:

```rust
pub trait HeaderViewZeroCopy {
    /// Returns reference to header bytes (no copy)
    fn view(&self) -> &[u8];
    
    /// Validates bounds before access
    fn validate_bounds(&self) -> Result<(), BoundsError>;
}
```

**Violation Detection**:
- Any heap allocation in `marshal()` for zero-copy path → **BUG**
- Any pointer dereference outside ring buffer → **UB (undefined behavior)**

---

## 5. Formal Proof Obligations

| Property | Lean4 Theorem | Location | Status |
|----------|---------------|----------|--------|
| Header parse safety | `HeaderViewBoundsSafety` | `formal/lean/Wire/HeaderBounds.lean` | 🔄 In progress |
| Wire format invariants | `SMIPHeaderInvariants` | `formal/lean/Wire/FormatInvariants.lean` | ✅ Proven |
| Cross-language compatibility | `BridgeContractRoundTrip` | `formal/lean/Bridge/ContractRoundTrip.lean` | ⏸️ Deferred |

---

## 6. Validation Commands

```bash
# Validate wire format marshaling/unmarshaling
cargo test -p wire --lib header_view_ref::marshal_unmarshal_roundtrip

# Run Miri on wire crate (memory safety)
cargo miri test -p wire || true

# Regenerate bridge contract report
make verify-bridge
```

---

## 7. References

- [`BRIDGE_CONTRACT.md`](./../../BRIDGE_CONTRACT.md) - Cross-language compatibility
- [`THEOREM_REMEDIATION_TRACKER.md`](./THEOREM_REMEDIATION_TRACKER.md) - Proof obligations
- [`README_FULL.md`](./../../README_FULL.md) - Architecture overview

---

*Last Updated: 2026-01-XX*  
*Sovereign Mohawk Proto LLC — Zero-Trust, Byzantine-Tolerant Networking*
