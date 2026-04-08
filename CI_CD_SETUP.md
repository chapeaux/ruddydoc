# CI/CD Pipeline Setup for RuddyDoc

This document describes the CI/CD infrastructure for the RuddyDoc Rust rewrite.

## Files Created

### 1. `/rust-toolchain.toml`
Pins the Rust toolchain to stable channel with rustfmt and clippy components.

### 2. `/.cargo/config.toml`
Workspace-level cargo configuration:
- Enables incremental compilation for dev builds
- Configures LTO and codegen-units for release builds
- Linux linker optimizations (commented out - enable if lld is available)

### 3. `/deny.toml`
cargo-deny configuration for license and dependency auditing:
- **License allowlist**: MIT, Apache-2.0, BSD variants, ISC, Unlicense, Zlib, CC0, MPL-2.0
- **Note**: MPL-2.0 is included because it's used by cssparser (dependency of scraper/HTML backend)
- Legal team should review whether MPL-2.0 is acceptable or if alternative dependencies are needed
- Bans unknown registries and unknown git sources

### 4. `/.github/workflows/rust-ci.yml`
Main CI pipeline triggered on push/PR to main and rust-rewrite branches.

**Jobs:**
- **check**: `cargo check --workspace --all-targets --all-features`
- **clippy**: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- **fmt**: `cargo fmt --all -- --check`
- **test**: `cargo test --workspace --all-features`
- **license-audit**: `cargo deny check licenses`

**Performance optimizations:**
- Caches cargo registry, index, and build artifacts
- All jobs run in parallel
- Expected completion time: <10 minutes for clean build, <3 minutes with cache

### 5. `/.github/workflows/rust-release.yml`
Release pipeline triggered on version tags (v*).

**Jobs:**
- **build**: Cross-compile for 5 targets in parallel
  - `x86_64-unknown-linux-gnu` (Linux x86_64)
  - `aarch64-unknown-linux-gnu` (Linux ARM64)
  - `x86_64-apple-darwin` (macOS Intel)
  - `aarch64-apple-darwin` (macOS Apple Silicon)
  - `x86_64-pc-windows-msvc` (Windows)
- **release**: Creates GitHub Release with all binaries and checksums
  - Binaries are stripped (Linux/macOS)
  - Packaged as .tar.gz (Unix) or .zip (Windows)
  - SHA256 checksums generated for all artifacts

**Cross-compilation:**
- Linux ARM64 builds use gcc-aarch64-linux-gnu cross-compiler
- Proper linker configuration for all targets

## Current Status

### Working
- All YAML syntax is valid
- deny.toml configuration is valid
- Toolchain configuration is correct
- Workflows will run when code compiles

### Blocked
The workspace currently has compilation errors in `ruddydoc-graph`:
```
error[E0425]: cannot find value `BASE64_BINARY` in module `xsd`
error[E0283]: type annotations needed for QuadRef conversion
```

These are implementation issues in the graph crate, not CI/CD issues. The CI/CD pipeline will work once these are resolved.

## Next Steps

### For Rust Engineer
1. Fix compilation errors in `ruddydoc-graph`
2. Ensure `cargo check --workspace` passes
3. Ensure `cargo clippy --workspace -- -D warnings` passes
4. Ensure `cargo fmt --all -- --check` passes
5. Add tests so `cargo test --workspace` has something to run

### For Legal
Review the license allowlist in `deny.toml`:
- Is MPL-2.0 acceptable? (Currently used by cssparser)
- If not, we need to find an alternative HTML parsing library
- Consider whether additional licenses should be allowed

### For DevOps (Phase 6)
Future enhancements for Phase 6:
1. Add crates.io publishing job (requires workspace publishing order)
2. Add npm publishing job (requires npm wrapper package)
3. Add container image publishing
4. Add code coverage reporting (codecov.io integration)
5. Add security advisory checks (`cargo deny check advisories`)
6. Add performance benchmarks to CI

## Testing the CI Locally

Before pushing, you can test CI jobs locally:

```bash
# Check
cargo check --workspace --all-targets --all-features

# Clippy
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format check
cargo fmt --all -- --check

# Tests
cargo test --workspace --all-features

# License audit
cargo deny check licenses
```

## Release Process (Future)

Once the workspace is ready for release:

1. Update version in `Cargo.toml`
2. Update CHANGELOG.md
3. Commit changes
4. Create and push tag: `git tag v0.1.0 && git push origin v0.1.0`
5. GitHub Actions will:
   - Build binaries for all platforms
   - Create GitHub Release with binaries and checksums
   - (Phase 6) Publish to crates.io, npm, and container registry

## Comparison to Python Docling CI

The existing Python workflows (`.github/workflows/ci.yml`, etc.) are for the Python codebase. The new Rust workflows are:
- Named `rust-ci.yml` and `rust-release.yml` to avoid conflicts
- Faster (no conda/uv dependency installation)
- Simpler (no ML model downloads in CI)
- Cross-platform native (Windows, macOS, Linux all supported)

Once the Rust rewrite is complete and replaces Python docling, the old Python workflows can be removed.

## Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| CI total time (cached) | <3 min | Parallel jobs with artifact caching |
| CI total time (clean) | <10 min | Full rebuild from scratch |
| Release build time | <15 min | Cross-compilation for 5 targets in parallel |
| Artifact size (compressed) | <10 MB | Stripped binary + LICENSE + README |

## Troubleshooting

### "error: rustc X.Y.Z is not supported"
Update `rust-toolchain.toml` to a newer stable version or use `channel = "stable"` for latest.

### cargo-deny fails with license errors
Check `deny.toml` allowlist. May need to add new licenses or find alternative dependencies.

### Cross-compilation fails on macOS
Ensure Xcode command-line tools are installed. GitHub Actions runners have this by default.

### Windows build fails
Windows runner uses MSVC toolchain. Ensure all dependencies support MSVC (not MinGW/GNU).
