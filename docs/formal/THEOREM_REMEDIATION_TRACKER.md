# THEOREM REMEDIATION TRACKER

Status: placeholder stub required before further optimization commits.

Purpose: explicit traceability between Rust high-speed path behavior and Lean-verified invariants.

## Traceability map

| Rust behavior area | Rust location | Lean proof artifact | Status | Notes |
|---|---|---|---|---|
| Header parse safety and bounds | `wire` crate (`HeaderViewRef`) | `formal/lean/Wire/HeaderBounds.lean` | planned | Must prove no out-of-bounds reads for all parse entrypoints. |
| Routing miss/predictive determinism | `routing` crate (`lookup_or_predict`) | `formal/lean/Routing/PredictiveDeterminism.lean` | planned | Must prove deterministic next-hop selection for identical inputs. |
| Session establishment and key derivation constraints | `crypto` crate (`session`, `kex`) | `formal/lean/Crypto/SessionDerivationSoundness.lean` | planned | Must link nonces/session IDs to uniqueness assumptions used by Rust code. |
| Datapath forward-path non-corruption invariant | `datapath` crate (`Forwarder::process_batch`) | `formal/lean/Datapath/ForwardPathNonCorruption.lean` | planned | Must prove payload/header integrity except where explicit encryption/mutation is intended. |
| Rust to Go bridge contract consistency | `bridge` artifacts + `cli::bridge` | `formal/lean/Bridge/ContractRoundTrip.lean` | planned | Must prove schema-level contract fields preserve meaning across language boundary. |

## Mandatory gate policy

- No performance-significant datapath change should be merged unless the corresponding row above has:
  - a concrete Lean file path,
  - an owner,
  - and a verification outcome entry.
- Bridge and FFI-related changes must update the bridge row in the same pull request.
