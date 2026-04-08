# CI/CD Troubleshooting Guide

Common issues and solutions for RuddyDoc's CI/CD pipeline.

## CI Pipeline Issues

### Test Failures

#### Flaky tests on specific platforms
**Symptom**: Tests pass on Linux but fail on macOS or Windows

**Causes**:
- Path separator differences (`/` vs `\`)
- Line ending differences (LF vs CRLF)
- Timing-sensitive tests
- Platform-specific file system behavior

**Solutions**:
```rust
// Use std::path for cross-platform paths
use std::path::PathBuf;
let path = PathBuf::from("tests").join("fixtures").join("test.pdf");

// Normalize line endings in tests
let content = content.replace("\r\n", "\n");

// Add longer timeouts for slow CI machines
#[cfg(test)]
const TIMEOUT: Duration = if cfg!(debug_assertions) {
    Duration::from_secs(30)
} else {
    Duration::from_secs(5)
};
```

#### Out of memory errors
**Symptom**: `cargo test` killed by OOM

**Causes**:
- Tests running in parallel consuming too much memory
- Large test fixtures loaded into memory

**Solutions**:
```bash
# Run tests sequentially
cargo test -- --test-threads=1

# Or in workflow:
- name: Run tests
  run: cargo test --workspace --all-features -- --test-threads=2
```

### Clippy Failures

#### New clippy warnings fail CI
**Symptom**: `cargo clippy` fails with warnings after Rust update

**Solutions**:
```bash
# Locally: Fix all warnings
cargo clippy --workspace --all-features --fix

# Or suppress specific lints (use sparingly):
#![allow(clippy::too_many_arguments)]
```

**Workflow change** (temporary, for migrations):
```yaml
- name: Run clippy
  run: cargo clippy --workspace --all-targets --all-features -- -D warnings -A clippy::new_lint_name
```

### Cache Issues

#### Build takes too long despite caching
**Symptom**: Cache not being used, full rebuild every time

**Causes**:
- `Cargo.lock` changed (expected)
- Cache key mismatch
- Cache corrupted

**Solutions**:
```yaml
# Clear caches via GitHub UI: Settings → Actions → Caches
# Or update cache key to force new cache:
key: ${{ runner.os }}-cargo-build-target-v2-${{ hashFiles('**/Cargo.lock') }}
```

#### Stale cache causing errors
**Symptom**: Build errors that don't reproduce locally

**Solutions**:
```bash
# In workflow, add cache clearing step:
- name: Clear cargo cache
  run: |
    rm -rf ~/.cargo/registry
    rm -rf ~/.cargo/git
    rm -rf target
```

### Deny Failures

#### New dependency fails license check
**Symptom**: `cargo deny check licenses` fails

**Solutions**:
1. Check the dependency's license in `Cargo.toml` or crates.io
2. If acceptable, add to `deny.toml`:
   ```toml
   [licenses]
   allow = [
       "MIT",
       "Apache-2.0",
       # ... existing licenses
       "NEW-LICENSE",  # Add with comment explaining why
   ]
   ```
3. If unacceptable, find an alternative dependency

#### Security advisory found
**Symptom**: `cargo deny check advisories` fails

**Solutions**:
1. Update the vulnerable dependency:
   ```bash
   cargo update -p vulnerable-crate
   ```
2. If no fix available, temporarily ignore (with justification):
   ```toml
   [advisories]
   ignore = [
       "RUSTSEC-2024-0001",  # Waiting for upstream fix, no exploit path in our usage
   ]
   ```

## Release Pipeline Issues

### Cross-Compilation Failures

#### ARM64 Linux build fails
**Symptom**: `cargo build --target aarch64-unknown-linux-gnu` fails

**Causes**:
- Missing cross-compilation toolchain
- Native dependencies don't support cross-compilation

**Solutions**:
```yaml
# Ensure cross-compilation tools are installed:
- name: Install cross-compilation tools
  run: |
    sudo apt-get update
    sudo apt-get install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu

# For native dependencies, use cross:
- name: Install cross
  run: cargo install cross

- name: Build
  run: cross build --release --target aarch64-unknown-linux-gnu
```

#### Windows build fails with linking errors
**Symptom**: Link errors on Windows

**Causes**:
- Missing Windows SDK
- Native dependency issues

**Solutions**:
```yaml
# Use windows-latest which includes build tools
runs-on: windows-latest

# For native dependencies, check for windows-specific features:
[dependencies]
some-crate = { version = "1.0", features = ["windows"] }
```

#### macOS ARM64 build on x86_64 runner fails
**Symptom**: Cannot build aarch64-apple-darwin on x86_64 macOS

**Solutions**:
```yaml
# Use separate runners for each architecture:
matrix:
  include:
    - target: x86_64-apple-darwin
      os: macos-13  # Intel runner
    - target: aarch64-apple-darwin
      os: macos-14  # ARM runner
```

### Artifact Upload Issues

#### Artifact not found
**Symptom**: `upload-artifact` fails with "file not found"

**Causes**:
- File path is wrong
- File was not created
- File created in wrong directory

**Solutions**:
```bash
# Debug: List files before upload
- name: List artifacts
  run: |
    ls -la
    ls -la staging

# Check working directory
- name: Debug working directory
  run: pwd && ls -la
```

#### Artifact too large
**Symptom**: Upload fails with size error

**Solutions**:
- Strip binaries (already done for Unix)
- Exclude unnecessary files from staging
- Split into multiple artifacts

### Release Creation Issues

#### Release already exists
**Symptom**: `softprops/action-gh-release` fails with "release already exists"

**Causes**:
- Re-running workflow after release created
- Manual release created with same tag

**Solutions**:
```yaml
# Allow overwrite:
- name: Create release
  uses: softprops/action-gh-release@v2
  with:
    files: ruddydoc-*
    body: ${{ steps.changelog.outputs.changelog }}
    draft: false
    prerelease: false
    fail_on_unmatched_files: true
    overwrite: true  # Add this
```

#### Missing checksums in release
**Symptom**: SHA256 files not uploaded

**Causes**:
- Checksum generation failed silently
- File pattern doesn't match

**Solutions**:
```yaml
# More verbose checksum generation:
- name: Create checksums
  run: |
    sha256sum ruddydoc > ruddydoc.sha256 || { echo "Checksum failed"; exit 1; }
    cat ruddydoc.sha256

# More specific file pattern:
with:
  files: |
    ruddydoc-*.tar.gz
    ruddydoc-*.zip
    ruddydoc-*.sha256
```

## Debugging Workflows

### Enable debug logging

Add to workflow:
```yaml
env:
  ACTIONS_STEP_DEBUG: true
  ACTIONS_RUNNER_DEBUG: true
```

### SSH into runner (for debugging)

Use `mxschmitt/action-tmate`:
```yaml
- name: Setup tmate session
  uses: mxschmitt/action-tmate@v3
  if: failure()  # Only on failure
```

### Local workflow testing

Use `act` to run workflows locally:
```bash
# Install act
brew install act  # macOS
# or
curl https://raw.githubusercontent.com/nektos/act/master/install.sh | sudo bash

# Run workflow
act push  # For push trigger
act -j build  # For specific job
```

## Performance Optimization

### Speed up CI

1. **Use caching effectively**:
   - Cache `~/.cargo/registry`, `~/.cargo/git`, `target/`
   - Use specific cache keys with `Cargo.lock` hash

2. **Parallelize jobs**:
   - Independent jobs run concurrently
   - Use matrix builds for multi-platform tests

3. **Skip unnecessary steps**:
   ```yaml
   - name: Run tests
     if: "!contains(github.event.head_commit.message, '[skip ci]')"
   ```

4. **Use binary caching for tools**:
   ```yaml
   - name: Cache cargo-deny
     uses: actions/cache@v5
     with:
       path: ~/.cargo/bin/cargo-deny
       key: ${{ runner.os }}-cargo-deny-0.16.0
   ```

### Reduce release time

1. **Build in parallel**: Already using matrix builds
2. **Skip unnecessary targets**: Remove Windows ARM64 if not needed
3. **Incremental builds**: Cache between steps in same job
4. **Pre-built base images**: For Docker, use cached layers

## Common Error Messages

### "error: linker `cc` not found"
**Solution**: Install build tools on the runner
```yaml
- run: sudo apt-get install -y build-essential
```

### "error: could not find `Cargo.toml`"
**Solution**: Checkout code before build
```yaml
- uses: actions/checkout@v5
```

### "Permission denied" on script execution
**Solution**: Make script executable
```bash
chmod +x .github/validate-workflows.sh
```

### "API rate limit exceeded"
**Solution**: Use GITHUB_TOKEN for authenticated requests
```yaml
env:
  GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

## Monitoring

### Check CI status
- GitHub Actions tab: https://github.com/chapeaux/ruddydoc/actions
- Badge in README (add if not present):
  ```markdown
  ![CI](https://github.com/chapeaux/ruddydoc/workflows/Rust%20CI/badge.svg)
  ```

### Set up notifications
- GitHub: Settings → Notifications → Actions
- Slack/Discord: Use GitHub webhooks
- Email: Automatically sent on workflow failure

## Getting Help

1. Check workflow logs in GitHub Actions UI
2. Search GitHub Actions documentation
3. Check action-specific docs (e.g., `actions/cache`, `softprops/action-gh-release`)
4. Open issue in this repository with workflow run link
5. Community forum: https://github.com/orgs/community/discussions

## Useful Commands

```bash
# Validate workflows locally
.github/validate-workflows.sh

# Test release packaging locally
cargo build --release --target x86_64-unknown-linux-gnu
strip target/x86_64-unknown-linux-gnu/release/ruddydoc
tar czf ruddydoc.tar.gz -C target/x86_64-unknown-linux-gnu/release ruddydoc
sha256sum ruddydoc.tar.gz

# Test Docker build locally
docker build -t ruddydoc:test .
docker run --rm ruddydoc:test --version

# Clean all caches locally
cargo clean
rm -rf ~/.cargo/registry/cache
rm -rf ~/.cargo/git/db
```
