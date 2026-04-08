# Release Checklist

Use this checklist when creating a new RuddyDoc release.

## Pre-Release

- [ ] All tests passing on `main` branch
- [ ] CI passing on `main` branch (check GitHub Actions)
- [ ] CHANGELOG.md updated with release notes
- [ ] Version bumped in all necessary files:
  - [ ] `Cargo.toml` (workspace version)
  - [ ] `npm/package.json`
  - [ ] `packaging/homebrew/ruddydoc.rb` (template version)

## Release

- [ ] Create git tag: `git tag -a v{VERSION} -m "Release v{VERSION}"`
- [ ] Push tag: `git push origin v{VERSION}`
- [ ] Wait for release workflow to complete (check GitHub Actions)
- [ ] Verify GitHub Release was created with all artifacts:
  - [ ] 5 platform binaries (tar.gz/zip)
  - [ ] 5 SHA256 checksum files
  - [ ] Changelog in release notes

## Post-Release

### Publish to crates.io

Run these commands in order (crates must be published in dependency order):

```bash
# Core infrastructure
cargo publish -p ruddydoc-core
sleep 30  # Wait for crates.io to index
cargo publish -p ruddydoc-graph
sleep 30
cargo publish -p ruddydoc-ontology
sleep 30

# Converter and pipeline
cargo publish -p ruddydoc-converter
sleep 30
cargo publish -p ruddydoc-pipeline
sleep 30
cargo publish -p ruddydoc-models
sleep 30
cargo publish -p ruddydoc-export
sleep 30

# Backends
cargo publish -p ruddydoc-backend-md
sleep 30
cargo publish -p ruddydoc-backend-html
sleep 30
cargo publish -p ruddydoc-backend-csv
sleep 30
cargo publish -p ruddydoc-backend-docx
sleep 30
cargo publish -p ruddydoc-backend-pdf
sleep 30
cargo publish -p ruddydoc-backend-latex
sleep 30
cargo publish -p ruddydoc-backend-pptx
sleep 30
cargo publish -p ruddydoc-backend-xlsx
sleep 30
cargo publish -p ruddydoc-backend-image
sleep 30
cargo publish -p ruddydoc-backend-xml
sleep 30
cargo publish -p ruddydoc-backend-webvtt
sleep 30
cargo publish -p ruddydoc-backend-asciidoc
sleep 30

# Server and CLI (last)
cargo publish -p ruddydoc-server
sleep 30
cargo publish -p ruddydoc  # Note: CLI is renamed to "ruddydoc" for crates.io
```

- [ ] All crates published successfully
- [ ] Verify: `cargo search ruddydoc` shows correct version

### Publish to npm

```bash
cd npm
npm publish --access public
```

- [ ] npm package published
- [ ] Verify: `npx @chapeaux/ruddydoc@{VERSION} --version`

### Build and Push Docker Image

```bash
# Build
docker build -t ghcr.io/chapeaux/ruddydoc:{VERSION} .
docker tag ghcr.io/chapeaux/ruddydoc:{VERSION} ghcr.io/chapeaux/ruddydoc:latest

# Login to GitHub Container Registry
echo $GITHUB_TOKEN | docker login ghcr.io -u USERNAME --password-stdin

# Push
docker push ghcr.io/chapeaux/ruddydoc:{VERSION}
docker push ghcr.io/chapeaux/ruddydoc:latest
```

- [ ] Docker images built and pushed
- [ ] Verify: `docker run --rm ghcr.io/chapeaux/ruddydoc:{VERSION} --version`

### Update Homebrew Formula

1. Download release artifacts:
   ```bash
   VERSION={VERSION}
   curl -LO https://github.com/chapeaux/ruddydoc/releases/download/v${VERSION}/ruddydoc-v${VERSION}-x86_64-apple-darwin.sha256
   curl -LO https://github.com/chapeaux/ruddydoc/releases/download/v${VERSION}/ruddydoc-v${VERSION}-aarch64-apple-darwin.sha256
   curl -LO https://github.com/chapeaux/ruddydoc/releases/download/v${VERSION}/ruddydoc-v${VERSION}-x86_64-unknown-linux-gnu.sha256
   curl -LO https://github.com/chapeaux/ruddydoc/releases/download/v${VERSION}/ruddydoc-v${VERSION}-aarch64-unknown-linux-gnu.sha256
   ```

2. Extract checksums (first line is binary checksum, second is archive):
   ```bash
   cat ruddydoc-v${VERSION}-*.sha256
   ```

3. Update `packaging/homebrew/ruddydoc.rb`:
   - [ ] Version number
   - [ ] URLs for all platforms
   - [ ] SHA256 checksums (use archive checksums, not binary)

4. Test formula locally:
   ```bash
   brew install --build-from-source packaging/homebrew/ruddydoc.rb
   ruddydoc --version
   brew uninstall ruddydoc
   ```

5. Submit to Homebrew tap (if tap exists) or document for users

- [ ] Homebrew formula updated
- [ ] Formula tested locally

## Verification

Test all distribution channels:

```bash
# GitHub Release (already tested in post-release)
curl -LO https://github.com/chapeaux/ruddydoc/releases/download/v{VERSION}/ruddydoc-v{VERSION}-x86_64-unknown-linux-gnu.tar.gz
tar -xzf ruddydoc-v{VERSION}-x86_64-unknown-linux-gnu.tar.gz
./ruddydoc --version

# crates.io (already tested in post-release)
cargo install ruddydoc --version {VERSION}
ruddydoc --version

# npm (already tested in post-release)
npx @chapeaux/ruddydoc@{VERSION} --version

# Docker (already tested in post-release)
docker run --rm ghcr.io/chapeaux/ruddydoc:{VERSION} --version
```

- [ ] All channels verified
- [ ] No broken links in release notes
- [ ] Documentation updated on website (if applicable)

## Announce

- [ ] Tweet release (if applicable)
- [ ] Post to Reddit r/rust (if major release)
- [ ] Update project README with latest version
- [ ] Notify users/stakeholders

## Rollback (if needed)

If critical issues are found:

1. Yank bad versions:
   ```bash
   cargo yank --vers {VERSION} ruddydoc
   cargo yank --vers {VERSION} ruddydoc-*  # For all crates
   ```

2. Unpublish npm (within 72 hours):
   ```bash
   npm unpublish @chapeaux/ruddydoc@{VERSION}
   ```

3. Delete GitHub Release and tag:
   ```bash
   git push --delete origin v{VERSION}
   git tag -d v{VERSION}
   ```

4. Delete Docker images:
   ```bash
   # Use GitHub UI or API to delete from ghcr.io
   ```

5. Create hotfix release with incremented version

## Notes

- crates.io has a 10-minute wait between publish operations to prevent abuse
- npm allows unpublishing within 72 hours
- Docker tags cannot be deleted once pulled by users (use new tags)
- GitHub Releases can be edited or deleted at any time
- Homebrew formula updates may take time to propagate

## Version Template

Replace `{VERSION}` with actual version (e.g., `0.2.0`) throughout this checklist.
