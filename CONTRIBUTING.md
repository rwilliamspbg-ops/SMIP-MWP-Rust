# Contributing

Thanks for your interest in contributing to SMIP-MWP-Rust. This document summarizes the preferred workflow, testing expectations, and guidance for performance-related changes.

Getting started
- Fork the repository and create a topic branch for your work.
- Keep changes focused and small; prefer one logical change per PR.

Development workflow
- Run the unit test suite before opening a PR:

  ```sh
  cargo test --workspace --all-targets
  ```

- Format code with `cargo fmt` and fix clippy warnings where practical:

  ```sh
  cargo fmt
  cargo clippy --all-targets -- -D warnings
  ```

Performance changes and benchmarks
- If your change affects the datapath hot path or performance baselines, include:
  - A short description of the expected performance impact.
  - Reproducible benchmark artifacts (logs, CSVs) produced with the harness in `tools/` or `benchmark/`.
  - For CI baseline updates (e.g. `tools/bench_results/ci_baseline_mcr.txt`) attach run logs and request a `perf-approval` label — merging baseline updates requires perf review.

- Use the smoke harness for quick checks and the Criterion benches for microbench validation. See `benchmark/README.md` and the `tools/bench_results/` scripts for reproducible runs.

Pull request guidelines
- Use a descriptive title and include a short summary of the change.
- Include testing steps and any manual validation performed.
- If the change touches benchmarks or CI baselines, include the benchmark output and a short explanation of how the results were produced.

Code review
- Assign reviewers familiar with the area (maintainers are listed in `CONTRIBUTORS.md`).
- Address review comments promptly and squash/fixup commits as requested.

Maintainers and governance
- The project is maintained by the core team listed in `CONTRIBUTORS.md`. For release-level changes and baseline modifications, a maintainer or perf-owner approval is required.

Contact
- Open an issue for large design discussions or to request guidance before implementing substantial changes.
# Contributing

Thanks for your interest in contributing to SMIP-MWP-Rust.

Guidelines

- Open an issue to discuss large design changes before implementing them.
- Keep pull requests focused and small; one logical change per PR.
- Include tests for bug fixes and new features. Benchmarks and microbench changes should include updated Criterion harnesses where applicable.
- Follow existing Rust style conventions. Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before opening a PR.

Commit messages

- Use conventional commit style when practical: `feat:`, `fix:`, `docs:`, `chore:`, `perf:`.

Review process

- PRs will be reviewed by maintainers and may request changes. Please address review comments promptly.

Performance baseline changes

- Changes to `tools/benchmark/sla_baselines.json` are gated and require explicit performance review.
- To change SLA baselines: attach measurement artifacts, explain the rationale in the PR description, and request a `perf-approval` label from maintainers. A dedicated CI check will block PRs that modify the baselines without this label.

License and DCO

- By contributing you agree that your contributions will be licensed under the project's AGPL-3.0 license.

Testing locally

```sh
source $HOME/.cargo/env
cargo test --workspace --all-targets
cargo bench -p bench --bench datapath_bench  # optional smoke/bench runs
```

Thank you for helping improve the project!
