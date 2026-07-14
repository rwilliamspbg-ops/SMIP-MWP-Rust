# Bridge Contract Specification: Go ↔ Rust Compatibility Matrix

## Overview

This document defines the compatibility contract between the Go control plane and Rust datapath in the Sovereign Mohawk Internet Protocol stack. The bridge enables safe cross-language communication while maintaining zero-copy performance guarantees.

---

## 1. Wire Format Compatibility

### SMIP Header (Shared Structure)

| Field | Rust Type | Go Struct | Size | Alignment | Notes |
|-------|-----------|-----------|------|-----------|-------|
| `src_id` | `[u8; 32]` | `[byte]32` | 32B | 4B | Source ID, big-endian |
| `dst_id` | `[u8; 32]` | `[byte]32` | 32B | 4B | Destination ID, big-endian |
| `flow_label` | `u32` | `uint32` | 4B | 4B | Flow classification |
| `seq_num` | `u64` | `uint64` | 8B | 8B | Sequence number |
| `session_id` | `[u8; 16]` | `[byte]16` | 16B | 2B | Session identifier |
| `flags` | `u16` | `uint16` | 2B | 2B | Packet flags (ECMP, ECN) |
| `length` | `u16` | `uint16` | 2B | 2B | Payload length |
| **Total** | - | - | **96B** | - | Zero-copy view compatible |

**Marshaling Logic:**
- Rust: `HeaderViewRef::marshal()` → Go: `wire.Unmarshal()`
- Both use little-endian on x86_64 (native)
- AF_XDP ring buffers hold raw bytes; no intermediate copies

---

## 2. Protocol Message Contract

### Bridge Request/Response Messages

| Direction | Rust Type | Go Type | Semantic |
|-----------|-----------|---------|----------|
| Route Update → Rust | `RouteUpdateMessage` | `wire.RouteUpdateRequest` | Control plane pushes routing table updates |
| Route Query ← Rust | `RouteQueryResponse` | `wire.RouteQueryResponse` | Datapath responds with next-hop info |
| Session Create → Rust | `SessionCreateRequest` | `wire.SessionCreateRequest` | Crypto session establishment |
| Session Status ← Rust | `SessionStatusResponse` | `wire.SessionStatusResponse` | AEAD tag + encrypted payload |

### Schema Validation Rules

1. **Field Presence**: All struct fields must be present (no optional/nullable in hot path)
2. **Type Size Match**: Go and Rust types must have identical binary layouts
3. **Endianness**: Both use native byte order (x86_64 little-endian)
4. **Alignment**: 4B alignment minimum for all fields

---

## 3. Compatibility Matrix

| Feature | Rust Implementation | Go Control Plane | Bridge Status | Notes |
|---------|--------------------|------------------|---------------|-------|
| SMIP Header marshaling | ✅ `HeaderViewRef` | ✅ `wire.HeaderView` | **Verified** | Zero-copy views match |
| Route table lookup | ✅ `RouteEntry` | ✅ `route.RouteInfo` | **Verified** | Next-hop prediction sync'd |
| MCR spray logic | ✅ `datapath.Fowarder` | ✅ `control.SprayPolicy` | **Verified** | Channel selection hash seed shared |
| HybridSession crypto | ✅ `crypto.HybridSession` | ✅ `session.HybridSession` | **Pending** | Go impl in progress |
| AEAD encryption | ✅ `aead.AEADDemo` | ⏸️ TBD | **Deferred** | Requires Go AEAD implementation |
| AF_XDP ring/umem | ✅ `afxdp.RealSocket` | ❌ N/A | **Rust-only** | Hardware path is Rust-native |

---

## 4. Bridge Request Protocol (JSON Control Interface)

### Request Format

```json
{
  "bridge_request": {
    "route_updates": [
      {
        "dest_id": ["2;32", "10;32"],
        "next_hop_id": ["3;32"]
      }
    ],
    "runtime_config": {
      "num_workers": 2,
      "iface": "ens1f0"
    },
    "mcr_config": {
      "channels": 3,
      "spray_mode": "primary",
      "hash_seed_hex": "deadbeef"
    }
  }
}
```

### Response Format

```json
{
  "bridge_response": {
    "status": "accepted",
    "routes_updated": 1,
    "workers_spawned": 2,
    "crypto_sessions": [
      {
        "session_id": "a1b2c3d4...",
        "key_exchange_status": "completed"
      }
    ]
  }
}
```

---

## 5. Validation Harness

### Verification Targets

| Target | Command | Expected Output | Status |
|--------|---------|-----------------|--------|
| Schema diff | `make verify-bridge` | "Bridge contract validated" | ✅ Passes |
| Wire format check | `cargo test -p wire --lib` | "HeaderViewRef marshal/unmarshal OK" | ✅ Passes |
| Cross-language compile | N/A (manual) | No linker errors | ⏸️ Manual verify |

### Running Validation

```bash
# Validate bridge contract
make verify-bridge

# Full workspace validation
make verify

# Regenerate reports after changes
make performance-envelope
```

---

## 6. Invariants to Preserve

1. **Zero-Copy Invariant**: No heap allocations in AF_XDP hot path
2. **Type Safety**: All bridge messages are structurally compatible
3. **Memory Ownership**: Rust owns ring buffers; Go only references
4. **AEAD Integrity**: Session tags validated before plaintext access
5. **MCR Consistency**: Channel selection deterministic across languages

---

## 7. Breaking Change Policy

**Never modify** the following without coordination:

- SMIP Header field layout or sizes
- Bridge request/response JSON schema
- AF_XDP ring buffer structures
- AEAD tag length or position

Any breaking change must go through `THEOREM_REMEDIATION_TRACKER.md` with formal proof updates.

---

## 8. Migration Notes

### From Legacy Go ↔ Rust Bridge (pre-SMIP)

1. Replace legacy C structs with SMIP Header
2. Use `HeaderViewRef::marshal()` for new marshaling
3. Drop legacy lock-free queue; use AF_XDP UMEM
4. Migrate crypto to hybridSession pattern

**Migration checklist:**
- [ ] Update all Go structs to match SMIP header layout
- [ ] Replace `legacy_packet` with `HeaderViewRef`
- [ ] Remove legacy heap allocations in hot path
- [ ] Add AEAD tag validation before decrypt

---

## 9. Future Work

| Item | Priority | Effort | Notes |
|------|----------|--------|-------|
| Go AEAD implementation | High | 4h | Match Rust AEADDemo interface |
| Cross-language Miri testing | Medium | 2h | Validate memory safety jointly |
| Formal bridge spec in Lean4 | Low | 8h | Prove wire format correctness |
| TPM attestation bridge | Future | 16h | Hardware root of trust sync |

---

## 10. References

- [`README_FULL.md`](./README_FULL.md) - Architecture overview
- [`DEPLOYMENT.manifest.md`](./DEPLOYMENT.manifest.md) - K8s/Helm manifests
- [`docs/formal/SMIP_HEADER_SPEC.md`](./docs/formal/SMIP_HEADER_SPEC.md) - Wire format spec
- [`docs/formal/CRYPTO_SESSION_SPEC.md`](./docs/formal/CRYPTO_SESSION_SPEC.md) - AEAD invariants
- [`THEOREM_REMEDIATION_TRACKER.md`](./docs/formal/THEOREM_REMEDIATION_TRACKER.md) - Proof obligations

---

*Last Updated: 2026-01-XX*  
*Sovereign Mohawk Proto LLC — Zero-Trust, Byzantine-Tolerant Networking*
