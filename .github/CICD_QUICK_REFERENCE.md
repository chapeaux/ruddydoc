# CI/CD Quick Reference

## CI Checks (Runs on every push/PR)

```bash
# Run all CI checks locally before pushing:
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo test --workspace --all-features
cargo deny check licenses
```

## Triggering a Release

```bash
# 1. Update version in Cargo.toml (workspace level)
# 2. Update CHANGELOG.md
# 3. Commit and tag:
git add Cargo.toml CHANGELOG.md
git commit -m "chore: bump version to X.Y.Z"
git tag vX.Y.Z
git push origin rust-rewrite --tags

# GitHub Actions will build and release automatically
```

## CI Workflow Files

- `.github/workflows/rust-ci.yml` - Main CI (check, test, lint, audit)
- `.github/workflows/rust-release.yml` - Release builds (cross-platform binaries)

## Configuration Files

- `rust-toolchain.toml` - Rust version pinning
- `.cargo/config.toml` - Build configuration
- `deny.toml` - License and dependency policies

## Build Targets

Linux x86_64 | Linux ARM64 | macOS Intel | macOS Apple Silicon | Windows
-------------|-------------|-------------|---------------------|--------
x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu | x86_64-apple-darwin | aarch64-apple-darwin | x86_64-pc-windows-msvc

## Common Issues

**"clippy warnings as errors"**: Fix with `cargo clippy --fix --workspace --allow-dirty`

**"format check failed"**: Fix with `cargo fmt --all`

**"license denied"**: Check `deny.toml` - may need to add license or change dependency

**"build cache stale"**: Delete `target/` and `~/.cargo/registry` dirs
