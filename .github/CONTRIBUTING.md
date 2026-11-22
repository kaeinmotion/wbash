# Contributing to wxec

Thank you for your interest in contributing to `wxec`!

## Development Setup

`wxec` is written in Rust. You will need Rust stable installed.

```bash
cargo build --workspace
cargo test --workspace
```

## Quality Invariants

Before submitting a pull request, ensure all checks pass:

1. **Formatting**:
   ```bash
   cargo fmt --all --check
   ```
2. **Clippy (Strict)**:
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   ```
3. **Tests**:
   ```bash
   cargo test --workspace
   ```
4. **MCP Protocol & Schema Budget**:
   ```bash
   cargo test -p wxec-adapters --test mcp -- --nocapture
   ```

## Commit Guidelines

Follow Conventional Commits:
- `feat(tools): add ...`
- `fix(ci): fix ...`
- `docs: update ...`
- `perf: optimize ...`
