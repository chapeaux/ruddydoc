# Phase 2 Test Fixtures - QA Report

**Project:** RuddyDoc  
**Phase:** Phase 2 - Text-Based Backends  
**Date:** 2024-04-07  
**QA Engineer:** Claude (QA Engineer Role)  
**Status:** ✅ Test Fixtures Ready for Implementation

## Executive Summary

I have created comprehensive test fixtures and a detailed test plan for Phase 2 backend implementation. All fixtures are realistic, well-structured, and exercise the key parsing requirements for each format. The test plan includes SPARQL validation queries, performance baselines, and acceptance criteria.

## Deliverables Created

### Test Fixtures (9 new files)

All fixtures are located in `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/`:

1. **`sample.html`** (90 lines)
   - Full HTML5 document with DOCTYPE, semantic elements (article, main, header, footer)
   - Headings (h1, h2, h3), paragraphs, lists (ul, ol), code blocks (pre/code)
   - Table with colspan and rowspan attributes
   - Images (inline and within figure), figcaption, blockquote
   - Tests metadata extraction (title, meta tags)

2. **`sample.csv`** (8 lines: 1 header + 7 data rows)
   - Comma-delimited with mixed data types (text, numbers)
   - Quoted field containing comma ("Sales, International")
   - Empty cell (Emma Wilson's salary)
   - Tests delimiter detection and cell handling

3. **`tabs.tsv`** (5 lines: 1 header + 4 data rows)
   - Tab-delimited numeric data
   - Tests tab delimiter detection

4. **`semicolons.csv`** (5 lines: 1 header + 4 data rows)
   - Semicolon-delimited
   - European number format (comma as decimal separator: 1299,99)
   - Tests alternate delimiter and number format handling

5. **`sample.tex`** (110 lines)
   - Full LaTeX article with documentclass, title/author/date, maketitle
   - Sections (section, subsection, subsubsection)
   - Lists (itemize, enumerate, nested)
   - Table (tabular) with caption and label
   - Mathematics (inline $...$ and display equation environment)
   - Figure with includegraphics, caption, label
   - Verbatim code block
   - Footnotes, cross-references (ref{...})
   - Comments (should be ignored)

6. **`sample.vtt`** (35 lines)
   - WEBVTT header
   - 8 cues with timestamps (00:00:00.000 --> 00:00:05.000 format)
   - Multi-line cues
   - Cue identifiers (intro, section-overview, conclusion)
   - NOTE blocks (should be ignored as comments)

7. **`sample.adoc`** (100 lines)
   - Document title (= syntax), author, version
   - Sections (==, ===, ====)
   - Lists (unordered *, ordered ., nested)
   - Table (|=== syntax) with caption
   - Source code blocks with language ([source,python] and [source,rust])
   - Image (image::path[alt] syntax)
   - Admonitions ([NOTE], [WARNING])
   - Quote block ([quote, author])

8. **`sample_jats.xml`** (130 lines)
   - Valid JATS (Journal Article Tag Suite) v1.3 XML
   - Front matter (journal metadata, article metadata)
   - Title, authors with affiliations, publication date, DOI
   - Abstract, keywords
   - Body with sections (sec), paragraphs (p), nested sections
   - Table (table-wrap) with caption, thead, tbody
   - Figure (fig) with caption and graphic reference
   - Back matter with references (ref-list)

9. **`sample_uspto.xml`** (160 lines)
   - Valid USPTO patent grant XML
   - Bibliographic data (publication reference, application reference, classifications)
   - Invention title, applicants, inventors, assignees
   - Abstract
   - Description sections (field of invention, background, summary, detailed description)
   - Claims (8 claims with nested claim-text)
   - Tests hierarchical structure and metadata extraction

### Test Plan Document

**`/home/ldary/rh/chapeaux/ruddydoc/tests/PHASE2_TEST_PLAN.md`** (550+ lines)

Comprehensive test plan including:

1. **Fixture Inventory Table**: All 10 fixtures (including Phase 1 sample.md) with stats
2. **Backend Test Matrix**: Structural elements, metadata, and special features for each backend
3. **Expected Triple Counts**: Approximate RDF triple counts to detect parsing failures
4. **SPARQL Validation Queries**:
   - Universal queries (all backends): document existence, element count, reading order, text content validation
   - Backend-specific queries (30+ queries total):
     - HTML: headings, tables with spans, images with alt text
     - CSV: header detection, quoted fields, empty cells
     - LaTeX: section hierarchy, equations, footnotes, comments ignored
     - WebVTT: cues with timestamps, multi-line cues, NOTE blocks ignored
     - AsciiDoc: heading levels, code with language, admonitions
     - JATS: metadata, abstract, tables, figures
     - USPTO: patent metadata, claims
5. **Round-Trip Test Procedures**: Parse → export → parse → compare
6. **Performance Baselines**: Target parse times and memory usage for each backend
7. **Test Coverage Requirements**: 80% line coverage, edge cases, error handling
8. **Error Handling Test Matrix**: Expected behavior for malformed inputs
9. **Implementation Checklist**: For Rust Engineers and QA Engineer
10. **Phase 2 Acceptance Criteria**: 9-point checklist for completion
11. **Known Limitations**: Scope exclusions and format-specific limitations

## Verification of Existing Tests

I attempted to verify that the Phase 1 tests (53 tests mentioned in the task) are still passing. However, when I ran `cargo test`, I found:

- Most crates show 0 tests (they are stubs for Phase 2 implementation)
- `ruddydoc-core` has 12 passing tests
- `ruddydoc-export` has 8 passing tests (in markdown_roundtrip.rs)
- Total: **20 passing tests** (not 53 as mentioned in the task)

This suggests either:
1. The "53 passing tests" refers to a future state, or
2. Additional test files exist that I haven't located

The existing integration test `/home/ldary/rh/chapeaux/ruddydoc/crates/ruddydoc-export/tests/markdown_roundtrip.rs` is comprehensive and follows the pattern I've used for the test plan. It tests:
- JSON export structure
- Turtle export
- N-Triples export
- Markdown round-trip
- SPARQL queries
- Separate named graphs for document and ontology

## Test Fixture Quality Assessment

### Coverage

Each fixture comprehensively exercises the key parsing requirements:

| Format | Headings | Paragraphs | Lists | Tables | Images | Code | Special Features |
|--------|----------|-----------|-------|--------|--------|------|------------------|
| Markdown | ✓ (3 levels) | ✓ | ✓ (ul, ol) | ✓ | ✓ | ✓ (2 langs) | Blockquote |
| HTML | ✓ (3 levels) | ✓ | ✓ (ul, ol) | ✓ (spans) | ✓ (2) | ✓ | Semantic HTML5, figcaption |
| CSV | - | - | - | ✓ | - | - | Quoted fields, empty cells |
| TSV | - | - | - | ✓ | - | - | Tab delimiter |
| CSV (semi) | - | - | - | ✓ | - | - | Semicolon delimiter, EU numbers |
| LaTeX | ✓ (3 levels) | ✓ | ✓ (nested) | ✓ | ✓ | ✓ | Equations, footnotes, refs |
| WebVTT | - | ✓ (cues) | - | - | - | - | Timestamps, multi-line |
| AsciiDoc | ✓ (4 levels) | ✓ | ✓ (nested) | ✓ | ✓ | ✓ (2 langs) | Admonitions, quotes |
| JATS | ✓ (sections) | ✓ | - | ✓ | ✓ | - | Metadata, abstract, refs |
| USPTO | ✓ (sections) | ✓ | - | - | - | - | Claims, metadata |

### Realism

- **HTML**: Uses modern HTML5 semantic elements, not just divs
- **CSV**: Includes real-world edge cases (quoted commas, empty cells)
- **LaTeX**: Uses authentic LaTeX packages (amsmath, graphicx) and structure
- **JATS**: Follows NLM DTD v1.3 specification
- **USPTO**: Follows USPTO XML v4.5 schema

### Size

All fixtures are appropriately sized for fast integration testing (50-160 lines), while still being complex enough to catch bugs.

### Validity

I verified syntax validity for:
- HTML: Valid HTML5 with proper DOCTYPE and structure
- CSV: Properly quoted and delimited
- LaTeX: Valid LaTeX that would compile with pdflatex
- WebVTT: Valid WebVTT format
- AsciiDoc: Valid AsciiDoc syntax
- XML (JATS, USPTO): Valid XML with proper DOCTYPE/namespace declarations

## Test Plan Quality Assessment

### Completeness

The test plan covers:
- ✓ All 8 Phase 2 backends (HTML, CSV×3, LaTeX, WebVTT, AsciiDoc, JATS, USPTO)
- ✓ Structural validation (SPARQL queries)
- ✓ Round-trip testing
- ✓ Performance baselines
- ✓ Error handling
- ✓ Edge cases
- ✓ Coverage requirements

### SPARQL Query Coverage

I wrote 30+ SPARQL queries covering:
- Universal validation (all backends)
- Format-specific structural checks
- Metadata extraction verification
- Edge case detection (empty cells, ignored comments, etc.)

Each query includes:
- The SPARQL code
- Expected results
- What is being tested

### Performance Baselines

The baselines are realistic and achievable:
- HTML: <10ms (DOM parsing is well-optimized in Rust)
- CSV: <5ms (simple format)
- LaTeX: <50ms (custom parser, complex format)
- WebVTT: <10ms (simple text format)
- AsciiDoc: <30ms (moderate complexity)
- JATS/USPTO: <20-30ms (XML parsing)

These assume well-optimized parsers using appropriate Rust crates (scraper, csv, quick-xml, etc.).

### Actionability

For each backend, the test plan provides:
1. Clear SPARQL queries to run
2. Expected results to compare against
3. Acceptance criteria (pass/fail)
4. Performance targets
5. Implementation checklist

A Rust Engineer can implement a backend and immediately know if it's correct by running the queries.

## Recommendations for Rust Engineers

### HTML Backend (`ruddydoc-backend-html`)

- Use `scraper` crate (based on html5ever and selectors)
- Pay attention to colspan/rowspan in tables (use attr() selectors)
- Extract metadata from `<head>` (title, meta tags)
- Map semantic HTML5 elements correctly (article, section, main, header, footer)
- Test with malformed HTML (browsers are forgiving, your parser should be too)

### CSV Backends (`ruddydoc-backend-csv`)

- Use the `csv` crate with flexible delimiter detection
- For delimiter auto-detection, try comma, then tab, then semicolon
- Handle quoted fields correctly (the crate does this, but verify)
- Detect header rows (usually first row, but can be auto-detected if all cells are text)
- For European numbers with comma decimals, preserve as strings (don't auto-convert)

### LaTeX Backend (`ruddydoc-backend-latex`)

- This is the most complex Phase 2 backend
- Custom parser required (no good Rust crate exists for full LaTeX)
- Start with structural commands: \section, \subsection, \begin{itemize}, \begin{table}
- Handle common inline commands: \textbf, \emph, \texttt
- For math: preserve LaTeX source in rdoc:Formula.textContent (don't try to parse)
- Ignore comments (% to end of line)
- Handle \ref and \label for cross-references
- Accept that some LaTeX documents won't parse perfectly (document limitations)

### WebVTT Backend (`ruddydoc-backend-webvtt`)

- Simple text format, easy to parse with regex or a simple state machine
- Each cue becomes an rdoc:TextElement with rdoc:startTime and rdoc:endTime properties
- NOTE blocks are comments, ignore them
- Preserve multi-line cue text with newlines
- Optional: preserve cue identifiers as metadata

### AsciiDoc Backend (`ruddydoc-backend-asciidoc`)

- Consider using `asciidoctor` via FFI or implement a simple parser
- Heading syntax: =, ==, ===, ==== map to levels 1, 2, 3, 4
- Lists: * for unordered, . for ordered
- Tables: |=== delimiter
- Code blocks: [source,lang] followed by ---- delimiters
- Admonitions: [NOTE], [WARNING], etc. map to rdoc:Paragraph with rdoc:admonitionType

### JATS Backend (`ruddydoc-backend-xml`)

- Use `quick-xml` for XML parsing
- Extract metadata from `<front>` (title, authors, affiliations, pub-date, abstract, keywords)
- Parse body sections recursively (sec can contain nested sec)
- Tables: `<table-wrap>` contains `<table>`, `<thead>`, `<tbody>`, `<tr>`, `<th>`, `<td>`
- Figures: `<fig>` with `<caption>` and `<graphic>` (xlink:href attribute)
- Store DOI and other identifiers as document metadata

### USPTO Backend (`ruddydoc-backend-xml`)

- Similar XML parsing approach as JATS
- Metadata: invention-title, inventors, assignees, classifications
- Description sections: hierarchical headings with id attributes
- Claims: `<claim>` with num attribute, `<claim-text>` (can be nested)
- Store patent-specific metadata (doc-number, country, date)

## Next Steps for QA Validation

Once Rust Engineers implement the backends, I will:

1. **Run Integration Tests**
   - Parse each fixture
   - Verify triple counts are in expected range
   - Run all SPARQL validation queries
   - Compare actual results to expected results in test plan

2. **Test Round-Trip**
   - Parse → Export to JSON → Validate structure
   - Parse → Export to original format → Parse again → Compare

3. **Benchmark Performance**
   - Measure parse time for each fixture
   - Measure memory usage
   - Compare to baselines
   - Report any regressions

4. **Test Error Handling**
   - Feed malformed inputs to each backend
   - Verify graceful degradation
   - Check error messages are helpful

5. **Test Edge Cases**
   - Empty files
   - Single-element documents
   - Unicode content
   - Very large files (if streaming parsers are implemented)

6. **Sign Off**
   - Update this report with test results
   - Sign off on each backend individually
   - Report any issues to Rust Engineers
   - Final sign-off when all backends pass

## Conclusion

All test fixtures and test plan documentation have been created and are ready for Phase 2 implementation. The fixtures are comprehensive, realistic, and properly sized. The test plan provides clear acceptance criteria, validation queries, and performance baselines.

**Status: ✅ Ready for Rust Engineers to begin Phase 2 backend implementation**

---

## Appendix: File Paths

All created files (absolute paths):

- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/sample.html`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/sample.csv`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/tabs.tsv`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/semicolons.csv`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/sample.tex`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/sample.vtt`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/sample.adoc`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/sample_jats.xml`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/sample_uspto.xml`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/PHASE2_TEST_PLAN.md`
- `/home/ldary/rh/chapeaux/ruddydoc/tests/PHASE2_QA_REPORT.md` (this file)

Existing Phase 1 fixture (not modified):
- `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/sample.md`

Existing Phase 1 integration test (not modified):
- `/home/ldary/rh/chapeaux/ruddydoc/crates/ruddydoc-export/tests/markdown_roundtrip.rs`
