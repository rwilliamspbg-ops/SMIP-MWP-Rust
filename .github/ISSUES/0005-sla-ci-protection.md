Title: Protect SLA baseline changes via CI pre-merge checks

Description:
Ensure `tools/benchmark/sla_baselines.json` cannot be changed silently without explicit performance owner approval. Enforce via CI gating or required PR labels.

Work items:
- Add a GitHub Action that checks PRs touching `tools/benchmark/sla_baselines.json` and fails unless the PR contains a `perf-approval` label or is from a trusted maintainer.
- Update `CONTRIBUTING.md` to document the required process: attach measurement artifacts, rationale, and approval before merging.

Labels: ci, perf
