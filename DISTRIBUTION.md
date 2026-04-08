# RuddyDoc Distribution Guide

This document describes the distribution pipeline for RuddyDoc.

## Overview

RuddyDoc is distributed through multiple channels:

1. **GitHub Releases** - Pre-built binaries for 5 platforms
2. **crates.io** - Rust package registry (`cargo install ruddydoc`)
3. **npm** - JavaScript package manager (`npx @chapeaux/ruddydoc`)
4. **Docker** - Container images (`ghcr.io/chapeaux/ruddydoc`)
5. **Homebrew** - Package manager for macOS and Linux (planned)

## Supported Platforms

| Platform | Architecture | Target Triple |
|----------|-------------|---------------|
| Linux | x86_64 | `x86_64-unknown-linux-gnu` |
| Linux | ARM64 | `aarch64-unknown-linux-gnu` |
| macOS | x86_64 | `x86_64-apple-darwin` |
| macOS | ARM64 | `aarch64-apple-darwin` |
| Windows | x86_64 | `x86_64-pc-windows-msvc` |

## Release Process

### 1. Prepare Release

```bash
# Update version in Cargo.toml
sed -i 's/version = "0.1.0"/version = "0.2.0"/' Cargo.toml

# Update version in npm/package.json
sed -i 's/"version": "0.1.0"/"version": "0.2.0"/' npm/package.json

# Update CHANGELOG.md
# Add release notes under new version heading

# Commit changes
git add Cargo.toml npm/package.json CHANGELOG.md
git commit -m "chore: bump version to 0.2.0"
```

### 2. Create and Push Tag

```bash
# Create annotated tag
git tag -a v0.2.0 -m "Release v0.2.0"

# Push tag to trigger release workflow
git push origin v0.2.0
```

### 3. Automated Release Pipeline

The `.github/workflows/rust-release.yml` workflow will:

1. **Build** binaries for all 5 target platforms
2. **Strip** binaries to reduce size (Linux/macOS only)
3. **Package** binaries with LICENSE and README
4. **Generate** SHA256 checksums for all artifacts
5. **Create** GitHub Release with changelog
6. **Upload** all binaries and checksums to GitHub Release

### 4. Manual Steps (Post-Release)

#### Publish to crates.io

```bash
# Publish workspace crates in dependency order
cargo publish -p ruddydoc-core
cargo publish -p ruddydoc-graph
cargo publish -p ruddydoc-ontology
cargo publish -p ruddydoc-converter
cargo publish -p ruddydoc-pipeline
cargo publish -p ruddydoc-models
cargo publish -p ruddydoc-export
cargo publish -p ruddydoc-backend-md
cargo publish -p ruddydoc-backend-html
cargo publish -p ruddydoc-backend-csv
cargo publish -p ruddydoc-backend-docx
cargo publish -p ruddydoc-backend-pdf
cargo publish -p ruddydoc-backend-latex
cargo publish -p ruddydoc-backend-pptx
cargo publish -p ruddydoc-backend-xlsx
cargo publish -p ruddydoc-backend-image
cargo publish -p ruddydoc-backend-xml
cargo publish -p ruddydoc-backend-webvtt
cargo publish -p ruddydoc-backend-asciidoc
cargo publish -p ruddydoc-server
cargo publish -p ruddydoc  # CLI (formerly ruddydoc-cli)
```

**Note**: The CLI crate is renamed to `ruddydoc` for crates.io so users can `cargo install ruddydoc`.

#### Publish to npm

```bash
cd npm
npm publish --access public
```

#### Build and Push Docker Image

```bash
# Build image
docker build -t ghcr.io/chapeaux/ruddydoc:0.2.0 .
docker tag ghcr.io/chapeaux/ruddydoc:0.2.0 ghcr.io/chapeaux/ruddydoc:latest

# Push to GitHub Container Registry
docker push ghcr.io/chapeaux/ruddydoc:0.2.0
docker push ghcr.io/chapeaux/ruddydoc:latest
```

#### Update Homebrew Formula

1. Download the release artifacts and get their SHA256 checksums
2. Update `packaging/homebrew/ruddydoc.rb` with new version and checksums
3. Submit a PR to the Homebrew tap (or create a tap at `chapeaux/homebrew-tap`)

## Verification

After release, verify all distribution channels:

```bash
# GitHub Releases
curl -LO https://github.com/chapeaux/ruddydoc/releases/download/v0.2.0/ruddydoc-v0.2.0-x86_64-unknown-linux-gnu.tar.gz
tar -xzf ruddydoc-v0.2.0-x86_64-unknown-linux-gnu.tar.gz
./ruddydoc --version

# crates.io
cargo install ruddydoc --version 0.2.0
ruddydoc --version

# npm
npx @chapeaux/ruddydoc@0.2.0 --version

# Docker
docker run --rm ghcr.io/chapeaux/ruddydoc:0.2.0 --version
```

## CI/CD Architecture

### CI Pipeline (`.github/workflows/rust-ci.yml`)

Runs on every push to `main` and `rust-rewrite`, and on all PRs:

- **check**: `cargo check`, `cargo clippy`, `cargo fmt`
- **test**: Test suite on Linux, macOS (x64 + ARM64), Windows
- **deny**: License and security audit with `cargo-deny`
- **benchmark**: Run benchmarks (non-blocking)

### Release Pipeline (`.github/workflows/rust-release.yml`)

Triggered on version tags (`v*`):

- **build**: Cross-compile for 5 platforms, strip, package, checksum
- **release**: Generate changelog, create GitHub Release

## Binary Size Optimization

The release builds use aggressive optimization:

```toml
[profile.release]
lto = true
codegen-units = 1
```

Additionally:
- Binaries are stripped on Linux and macOS
- Docker images use multi-stage builds with debian:bookworm-slim
- Only the final binary is included in the runtime image

Expected sizes:
- Stripped binary: ~15-25 MB (depending on features)
- Docker image: ~50-80 MB

## Troubleshooting

### npm installation fails behind firewall

Users can install manually:

```bash
cargo install ruddydoc
```

### Cross-compilation fails for ARM64

The Linux ARM64 build requires cross-compilation tools. The CI installs:

```bash
sudo apt-get install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
```

### cargo publish fails with dependency errors

Ensure crates are published in dependency order. The CLI crate (`ruddydoc`) must be published last.

## Security

- Release artifacts include SHA256 checksums for verification
- All CI workflows use pinned action versions (@v5, @v2)
- cargo-deny enforces license allowlist and checks security advisories
- Docker images run as non-root user

## Future Enhancements

- [ ] Automated crates.io publishing in release workflow
- [ ] Automated npm publishing in release workflow
- [ ] Automated Docker image builds in release workflow
- [ ] Homebrew tap repository
- [ ] Code signing for macOS binaries
- [ ] Windows installer (MSI)
- [ ] Debian/Ubuntu packages (.deb)
- [ ] RPM packages for Fedora/RHEL
- [ ] AUR package for Arch Linux
