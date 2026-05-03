# Contributing

## Git hooks

Run `./.githooks/install.sh` once after cloning to enable pre-commit / pre-push hooks.

The hooks run from `.githooks/` (set via `core.hooksPath`):
- `pre-commit` — runs `cargo fmt --check` and `cargo clippy -D warnings` on commits touching Rust files.
- `pre-push` — runs `cargo test --all-features` from `generator/`.
