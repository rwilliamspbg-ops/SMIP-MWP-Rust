<!-- Please describe the change and why it is needed. -->

## Summary

Describe the change in one or two sentences.

## Changes
- What changed (files, modules, behavior)

## Testing
- How was this tested? List commands and reproducible steps.

```sh
# unit tests
cargo test --workspace --all-targets

# example: run the smoke harness
./tools/benchmark/run_smoke.sh
```

## Performance
- If this PR affects datapath performance, include a short summary and attach any bench logs/CSV artifacts (put them under `tools/bench_results/` when appropriate).
- If updating CI baselines (e.g. `ci_baseline_mcr.txt`), request `perf-approval` and include pinned run artifacts.

## Related issues / PRs
- Link any related issues or PRs.

## Checklist
- [ ] I ran `cargo test --workspace --all-targets` and formatting (`cargo fmt`).
- [ ] For performance changes, artifacts are attached and reproduction steps are provided.
- [ ] I assigned reviewers and labels as appropriate.
