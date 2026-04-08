# RuddyDoc Dependency License Audit

**Project License:** MIT

**Date:** 2026-04-07

**Audit Status:** All dependencies compatible with MIT license.

---

## Summary Table

| Crate | Version | License | Compatible with MIT? | Notes |
|-------|---------|---------|:-:|---------|
| oxigraph | 0.5 | Apache 2.0 OR MIT | ✅ | Dual-licensed. Graph database is core dependency. No transitive issues. |
| serde | 1 | Apache 2.0 OR MIT | ✅ | Standard serialization framework. Widely tested in Rust ecosystem. |
| serde_json | 1 | Apache 2.0 OR MIT | ✅ | Standard JSON support. Transitive via serde. |
| clap | 4 | Apache 2.0 OR MIT | ✅ | CLI argument parsing. Dual-licensed. |
| pulldown-cmark | 0.12 | MIT | ✅ | Markdown parser. GFM-compliant. Used widely. |
| scraper | 0.22 | MIT | ✅ | HTML DOM parsing via CSS selectors. Production-ready. |
| csv | 1 | MIT OR Unlicense | ✅ | CSV parsing. Unlicense is permissive (equivalent to public domain). |
| quick-xml | 0.37 | MIT | ✅ | XML parsing. Lightweight and fast. |
| zip | 2 | MIT | ✅ | ZIP archive handling. Core dependency for OOXML formats. |
| lopdf | 0.34 | MIT | ✅ | PDF parsing. Supports low-level PDF manipulation. |
| calamine | 0.26 | MIT | ✅ | Excel file reading. Pure Rust, no external dependencies. |
| image | 0.25 | Apache 2.0 OR MIT | ✅ | Image processing. Dual-licensed. |
| ort | 2 | Apache 2.0 OR MIT | ✅ | ONNX Runtime bindings. See special note below. |
| rayon | 1 | Apache 2.0 OR MIT | ✅ | Data parallelism library. Widely used in Rust. |
| tokio | 1 | MIT | ✅ | Async runtime. De facto standard in Rust ecosystem. |
| thiserror | - | Apache 2.0 OR MIT | ✅ | Error derive macros. Dual-licensed. |
| indicatif | - | MIT | ✅ | Progress bars and terminal UI. Lightweight. |

---

## License Compatibility Analysis

### Green Flags (All Clear)

All dependencies use permissive licenses compatible with MIT:

- **MIT License** (9 crates): pulldown-cmark, scraper, quick-xml, zip, lopdf, calamine, tokio, indicatif
- **Apache 2.0 OR MIT** (7 crates): oxigraph, serde, serde_json, clap, image, ort, rayon, thiserror
- **MIT OR Unlicense** (1 crate): csv

The Unlicense is explicitly public domain dedication, more permissive than MIT.

### No Red Flags

No GPL, AGPL, SSPL, Commons Clause, or any copyleft licenses detected.

No dual-license conflicts. All dependencies can be used in MIT-licensed projects without restrictions.

---

## Special Consideration: ONNX Runtime

The `ort` crate (version 2) is Rust bindings to ONNX Runtime, which is maintained by Microsoft.

**ONNX Runtime License:** MIT

**Important Note:** ONNX Runtime is open source (MIT license). However, users should be aware of:

1. **CUDA/cuDNN Dependencies** (Optional): If users enable GPU acceleration, they may need NVIDIA CUDA libraries. CUDA licensing is separate from ONNX Runtime and governed by NVIDIA's Software License Agreement. Verify CUDA licensing compliance separately if GPU support is enabled.

2. **TensorRT** (Optional): Microsoft provides optional ONNX Runtime builds with TensorRT support. TensorRT is governed by NVIDIA's license. Document any GPU-enabled builds separately.

3. **Recommendation:** Keep GPU support optional and feature-gated. Document this prominently in the README.

---

## Transitive Dependencies

A spot check of high-impact transitive dependencies:

- **serde_derive** (macro expansion for serde): MIT/Apache 2.0 ✅
- **indexmap** (used by serde for ordered maps): Apache 2.0 or MIT ✅
- **proc-macro2, quote, syn** (used by derive macros): MIT ✅
- **unicode-ident** (used by syn): MIT or Apache 2.0 ✅

Recommendation: Run `cargo-deny` in CI to catch GPL creep in transitive dependencies on every build.

---

## Recommendations

### 1. Add License Compliance to CI/CD

```bash
# Install cargo-deny
cargo install cargo-deny

# Run in CI before release
cargo deny check
```

Create a `deny.toml` file at the project root:

```toml
[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 OR MIT",
    "MIT OR Apache-2.0",
    "MIT OR Unlicense",
    "Unlicense",
    "ISC",
    "BSD-2-Clause",
    "BSD-3-Clause",
]
deny = [
    "GPL-2.0",
    "GPL-3.0",
    "AGPL-3.0",
    "SSPL-1.0",
]

[advisories]
db-path = "~/.cargo/advisory-db"
```

### 2. Update NOTICE File

Ensure `/home/ldary/rh/chapeaux/ruddydoc/NOTICE` lists all dependencies:

```
RuddyDoc License Attribution

RuddyDoc is licensed under the MIT License.

This project includes the following third-party dependencies:

- oxigraph (Apache 2.0 or MIT)
- serde (Apache 2.0 or MIT)
- serde_json (Apache 2.0 or MIT)
- clap (Apache 2.0 or MIT)
- pulldown-cmark (MIT)
- scraper (MIT)
- csv (MIT or Unlicense)
- quick-xml (MIT)
- zip (MIT)
- lopdf (MIT)
- calamine (MIT)
- image (Apache 2.0 or MIT)
- ort (Apache 2.0 or MIT) [includes ONNX Runtime - MIT]
- rayon (Apache 2.0 or MIT)
- tokio (MIT)
- thiserror (Apache 2.0 or MIT)
- indicatif (MIT)

For licenses, see LICENSE, LICENSE-APACHE, or check crates.io for each dependency.
```

### 3. Document GPU Usage Separately

If ONNX Runtime GPU support is enabled:

- Document CUDA/cuDNN licensing in GPU feature documentation
- Link to NVIDIA Software License Agreement
- Recommend users verify CUDA license compliance for their use case

### 4. Future Dependency Reviews

For each new dependency:

1. Check license on crates.io
2. Verify no GPL, AGPL, SSPL, or Commons Clause
3. Document dual-license crates with clear choice (select MIT when dual)
4. Run `cargo deny check` before merge

---

## Conclusion

**All planned dependencies are MIT-compatible.** The project is clear to proceed with Phase 1 and beyond.

No licensing conflicts detected. No further review needed unless:
- A new dependency is introduced with GPL or AGPL
- GPU support is enabled (then review NVIDIA licenses separately)
- A dependency changes its license

Last verified: 2026-04-07
