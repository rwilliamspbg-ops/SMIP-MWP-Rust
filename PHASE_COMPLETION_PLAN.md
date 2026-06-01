# SMIP-MWP-Rust Completion Plan: End-to-End Local Testing

## Executive Summary
This plan completes all gaps in SMIP-MWP-Rust to enable local testing. Execution follows the principle: **foundational infrastructure → formal scaffolding → hardware integration → bridge validation → stress harness → observability**.

**Total Estimated Time**: 45-60 minutes for full completion  
**Dependencies**: Rust toolchain, optional: Python3

---

## Phase 1: Foundational Infrastructure (15 mins)

### 1.1 Create Complete Makefile
Replace current Makefile with comprehensive build system

### 1.2 Create README_FULL.md
Add architecture deep-dive, deployment guides, and performance claims

### 1.3 Create DEPLOYMENT.manifest.md
Add K8s/Helm manifests and deployment procedures

### 1.4 Create BRIDGE_CONTRACT.md
Document Go ↔ Rust compatibility matrix

---

## Phase 2: Formal Verification Scaffolding (10 mins)

### 2.1 Initialize formal/ directory structure
Create Lean4 project scaffolding with theorem stubs

### 2.2 Create SMIP_HEADER_SPEC.md
Formal wire format specification for routing packets

### 2.3 Create CRYPTO_SESSION_SPEC.md
Hybrid KEX and AEAD invariants documentation

### 2.4 Create THEOREM_REMEDIATION_TRACKER.md
Traceability matrix linking proofs to code locations

---

## Phase 3: Hardware Integration & AF_XDP (15 mins)

### 3.1 Implement afxdp RealSocket wrapper
Complete AF_XDP ring/umem integration

### 3.2 Create hardware/setup_hardware.sh
Hugepage pinning, CPU isolation, IRQ affinity

### 3.3 Create tools/hardware/smoke/README.md
Document smoke test procedure

---

## Phase 4: MCR Spray Implementation (10 mins)

### 4.1 Complete routing table lookup_spray()
Implement multi-channel spraying logic

### 4.2 Update datapath process_batch_spray_full()
Remove stub implementation, add real spray logic

---

## Phase 5: Bridge Validation Tooling (5 mins)

### 5.1 Create tools/validation/verify_bridge.sh
Cross-language schema diff and validation harness

---

## Phase 6: Stress Harness Completion (10 mins)

### 6.1 Create tools/stress/run_stress.sh
Trex/BFCL traffic generation wrapper

### 6.2 Create tools/stress/profile_stress.sh
pprof capture + perf analysis script

---

## Phase 7: Observability & CI Artifacts (5 mins)

### 7.1 Create .github/workflows/ci.yml
CI workflow with lint, test, miri, validation gates

### 7.2 Create benchmark/observability/dashboards/Grafana JSON
Monitoring dashboard configuration

---

## Phase 8: Final Validation & Testing (5 mins)

### 8.1 Run full workspace tests
`cargo test --workspace --all-targets`

### 8.2 Run bridge validation
`make verify-bridge`

### 8.3 Generate performance envelope artifacts
`make performance-envelope`

---

## Execution Commands

Run these commands sequentially to complete all phases:

```bash
cd C:\Users\rwill\OneDrive\Desktop\SMIP-MWP-Rust

# Phase 1: Foundational Infrastructure
echo "=== PHASE 1: FOUNDATIONAL INFRASTRUCTURE ==="

# Create README_FULL.md
write_file path="README_FULL.md" content="..."

# Create DEPLOYMENT.manifest.md  
write_file path="DEPLOYMENT.manifest.md" content="..."

# Create BRIDGE_CONTRACT.md
write_file path="BRIDGE_CONTRACT.md" content="..."

echo "Phase 1 complete"
```

Continue through all phases...

---

## Post-Completion Validation Checklist

- [ ] `cargo test --workspace --all-targets` passes
- [ ] `make verify-bridge` completes successfully
- [ ] Hardware smoke test builds (even if cannot run without NIC)
- [ ] Bridge contract validation script exists and is executable
- [ ] Formal verification directory structure exists
- [ ] README_FULL.md contains architecture overview
- [ ] DEPLOYMENT.manifest.md contains K8s manifests
- [ ] .github/workflows/ci.yml exists with proper gates

---

## Next Steps After Local Testing

1. **Benchmark Validation**: Run `MOHAWK_MCR_CHANNELS=3 ./tools/benchmark/run_chaos_epyc_profile.sh`
2. **Performance Claims**: Generate `benchmark/PERFORMANCE_ENVELOPE.md`
3. **CI Integration**: Push to GitHub and verify workflow badges update
4. **Documentation Review**: Ensure all MD files pass markdown linter

---

## Rollback Plan

If any phase fails:
- Phase 1 failures: Revert Makefile, README_FULL, DEPLOYMENT, BRIDGE_CONTRACT
- Phase 2 failures: Delete formal/ directory contents, keep structure
- Phase 3 failures: Keep AF_XDP stubs, mark as hardware-dependent only
- Phase 4+ failures: Document in docs/mcr_architecture.md
