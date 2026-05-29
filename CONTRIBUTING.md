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
