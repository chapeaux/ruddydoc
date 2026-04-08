# RuddyDoc Compatibility Test Plan

**Version:** 1.0  
**Date:** 2026-04-07  
**QA Engineer:** Claude Code QA Engineer  
**Status:** Active  

## Overview

This document defines the compatibility testing strategy for RuddyDoc against Python docling (v2.85.0). The goal is to verify that RuddyDoc produces **structurally equivalent** output to Python docling across all supported formats, while documenting acceptable differences.

## Definitions

### Structural Equivalence

Two documents are **structurally equivalent** if they contain:

1. **Same element count by type**: Same number of paragraphs, headings, tables, pictures, etc.
2. **Same reading order**: Elements appear in the same sequence
3. **Same element hierarchy**: Parent-child relationships are preserved (e.g., list items under lists, cells under tables)
4. **Equivalent content**: Text content matches after normalization (whitespace differences OK)
5. **Equivalent metadata**: Document-level metadata (format, page count) matches

Structural equivalence **does NOT require**:

- Byte-for-byte identical JSON output
- Identical floating-point bounding boxes (within 1% tolerance is acceptable)
- Identical confidence scores (ML models may differ slightly)
- Identical element IDs or internal identifiers
- Identical timestamp/hash values

### Normalization Rules

When comparing text content:

- Normalize Unicode (NFC)
- Collapse consecutive whitespace to single space
- Trim leading/trailing whitespace
- Normalize line endings (LF)
- Case-insensitive for code language tags

When comparing numbers:

- Bounding boxes: within 1% of page dimensions
- Confidence scores: within 0.05 absolute difference
- Cell positions: exact match required

## Test Fixtures

### Fixtures to Port from Python Docling

From `/home/ldary/rh/chapeaux/ruddydoc/tests/data/`:

| Format | Fixture File | Test Coverage |
|--------|-------------|--------------|
| **Markdown** | `md/wiki_duck.md` | Tables, headings, lists, links |
| **HTML** | `html/wiki_duck.html` | Semantic elements, tables, nested lists |
| **CSV** | `csv/simple.csv` | Header detection, delimiter handling |
| **CSV** | `csv/semicolons.csv` | Semicolon delimiter |
| **CSV** | `csv/tabs.tsv` | Tab delimiter |
| **DOCX** | `docx/lorem_ipsum.docx` | Paragraphs, headings, bold/italic |
| **DOCX** | `docx/tables_and_lists.docx` | Tables with merged cells, numbered lists |
| **XLSX** | `xlsx/simple.xlsx` | Multiple sheets, formulas |
| **XLSX** | `xlsx/merged_cells.xlsx` | Cell merging, spans |
| **PPTX** | `pptx/sample_presentation.pptx` | Slide text, images, tables |
| **LaTeX** | `latex/sample_paper.tex` | Sections, equations, citations |
| **LaTeX** | `latex/tables_and_figures.tex` | Tables, figures, captions |
| **WebVTT** | `webvtt/sample.vtt` | Cues with timestamps |
| **AsciiDoc** | `asciidoc/sample.adoc` | Admonitions, code blocks, tables |
| **JATS** | `jats/sample_article.xml` | Scientific article structure |
| **USPTO** | `uspto/sample_patent.xml` | Patent document structure |

### RuddyDoc-Specific Fixtures

Located in `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/`:

- `sample.md` - Core Markdown test (already tested in roundtrip)
- `sample.html` - HTML with semantic elements
- `sample.csv` - CSV with headers
- `sample.tex` - LaTeX document
- `sample.vtt` - WebVTT subtitles
- `sample.adoc` - AsciiDoc
- `sample_jats.xml` - JATS scientific article
- `sample_uspto.xml` - USPTO patent document
- `semicolons.csv` - Semicolon delimiter test
- `tabs.tsv` - Tab delimiter test

## Test Categories

### 1. Format Round-Trip Tests

**Goal:** Verify that parsing and re-exporting preserves structure.

For each format that RuddyDoc both parses and exports:

```
Input (Format A) → Parse → RDF Graph → Export (Format A) → Parse → RDF Graph'
```

**Assertion:** Graph and Graph' have same element counts and reading order.

**Formats:** Markdown, HTML, JSON

**Example:**
```rust
#[test]
fn markdown_roundtrip_preserves_structure() {
    // Parse sample.md
    let (store1, graph1) = parse_markdown("sample.md");
    
    // Export to Markdown
    let md_out = export_markdown(&store1, &graph1);
    
    // Parse exported Markdown
    let (store2, graph2) = parse_markdown_string(&md_out);
    
    // Compare element counts
    assert_eq!(count_paragraphs(&store1, &graph1), count_paragraphs(&store2, &graph2));
    assert_eq!(count_headings(&store1, &graph1), count_headings(&store2, &graph2));
    assert_eq!(count_tables(&store1, &graph1), count_tables(&store2, &graph2));
}
```

### 2. Cross-Format Structure Tests

**Goal:** Verify that semantically identical content in different formats produces the same logical structure.

**Test pairs:**
- `wiki_duck.md` vs `wiki_duck.html` (same Wikipedia article)
- Hand-authored equivalents

**Assertion:** Both produce same number and type of elements in same reading order.

### 3. Export Format Validation Tests

**Goal:** Verify that exported output is well-formed and conforms to format specifications.

For each export format:

| Format | Validation |
|--------|-----------|
| JSON | Valid JSON, has required fields (`name`, `source_format`, `texts`, `tables`, `pictures`) |
| HTML | Well-formed HTML5, passes Nu HTML Checker (basic) |
| Markdown | Parseable by CommonMark parser |
| Turtle | Valid Turtle RDF syntax |
| N-Triples | Each line matches N-Triples grammar |
| JSON-LD | Valid JSON-LD with `@context` |
| RDF/XML | Valid RDF/XML |
| DocTags | Starts with `<doctags>`, ends with `</doctags>` |
| WebVTT | Starts with "WEBVTT" header |

**Example:**
```rust
#[test]
fn json_export_is_valid_and_complete() {
    let (store, graph) = parse_any_document();
    let json_str = export_json(&store, &graph);
    
    // Parse as JSON
    let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    
    // Validate schema
    assert!(json.get("name").is_some(), "missing 'name' field");
    assert!(json.get("source_format").is_some(), "missing 'source_format'");
    assert!(json["texts"].is_array(), "'texts' must be array");
    assert!(json["tables"].is_array(), "'tables' must be array");
    assert!(json["pictures"].is_array(), "'pictures' must be array");
    
    // Validate text elements
    for text in json["texts"].as_array().unwrap() {
        assert!(text.get("type").is_some(), "text missing 'type'");
        assert!(text.get("text").is_some(), "text missing 'text' content");
        assert!(text.get("reading_order").is_some(), "text missing 'reading_order'");
    }
}
```

### 4. SPARQL Correctness Tests

**Goal:** Verify that SPARQL queries return correct results from the RDF graph.

**Query categories:**

#### a. Element Counting Queries

```sparql
SELECT (COUNT(?e) AS ?count) WHERE { 
  GRAPH <doc_graph> { ?e a <rdoc:Paragraph> } 
}
```

**Assertion:** Count matches expected value from fixture analysis.

#### b. Reading Order Integrity

```sparql
SELECT ?order WHERE { 
  GRAPH <doc_graph> { ?e rdoc:readingOrder ?order } 
} ORDER BY ?order
```

**Assertion:** Orders are 0, 1, 2, 3, ... (contiguous, no gaps).

#### c. Content Completeness

```sparql
SELECT ?e WHERE { 
  GRAPH <doc_graph> { 
    ?e a rdoc:TextElement . 
    FILTER NOT EXISTS { ?e rdoc:textContent ?t } 
  } 
}
```

**Assertion:** Result is empty (all text elements have content).

#### d. Table Structure Validation

```sparql
SELECT ?table ?cell ?row ?col WHERE { 
  GRAPH <doc_graph> { 
    ?table a rdoc:TableElement . 
    ?table rdoc:hasCell ?cell . 
    ?cell rdoc:cellRow ?row . 
    ?cell rdoc:cellColumn ?col 
  } 
}
```

**Assertion:** All cells have valid row/column positions, no duplicates.

#### e. Hierarchy Integrity

```sparql
SELECT ?child WHERE { 
  GRAPH <doc_graph> { 
    ?child rdoc:parentElement ?parent . 
    FILTER NOT EXISTS { ?parent rdoc:childElement ?child } 
  } 
}
```

**Assertion:** Result is empty (parent-child relationships are bidirectional).

**Example:**
```rust
#[test]
fn sparql_reading_order_is_contiguous() {
    let (store, graph) = parse_any_document();
    
    let sparql = format!(
        "SELECT ?order WHERE {{ GRAPH <{graph}> {{ ?e <{}> ?order }} }} ORDER BY ?order",
        ont::iri(ont::PROP_READING_ORDER)
    );
    
    let result = store.query_to_json(&sparql).unwrap();
    let orders: Vec<i64> = result.as_array().unwrap()
        .iter()
        .map(|row| parse_int(row["order"].as_str().unwrap()))
        .collect();
    
    // Verify contiguous sequence starting at 0
    for (i, &order) in orders.iter().enumerate() {
        assert_eq!(order, i as i64, "reading order has gap at position {i}");
    }
}
```

### 5. Docling JSON Schema Compatibility Tests

**Goal:** Verify RuddyDoc's JSON export matches Python docling's schema.

**Helper function:**
```rust
fn validate_docling_json(json: &serde_json::Value) {
    // Top-level fields
    assert!(json.get("name").is_some(), "missing 'name'");
    assert!(json.get("source_format").is_some(), "missing 'source_format'");
    
    // Texts array
    assert!(json["texts"].is_array(), "'texts' must be array");
    for text in json["texts"].as_array().unwrap() {
        assert!(text.get("type").is_some(), "text missing 'type'");
        assert!(text.get("text").is_some(), "text missing 'text'");
        assert!(text.get("reading_order").is_some(), "text missing 'reading_order'");
        
        // Type-specific fields
        if text["type"] == "section_header" {
            assert!(text.get("heading_level").is_some(), "heading missing 'heading_level'");
        }
        if text["type"] == "code" {
            // code_language is optional
        }
    }
    
    // Tables array
    assert!(json["tables"].is_array(), "'tables' must be array");
    for table in json["tables"].as_array().unwrap() {
        assert!(table.get("cells").is_some() || table.get("row_count").is_some(),
                "table must have 'cells' or 'row_count'");
        
        if let Some(cells) = table.get("cells") {
            for cell in cells.as_array().unwrap() {
                assert!(cell.get("text").is_some(), "cell missing 'text'");
                assert!(cell.get("row").is_some(), "cell missing 'row'");
                assert!(cell.get("col").is_some(), "cell missing 'col'");
                assert!(cell.get("is_header").is_some(), "cell missing 'is_header'");
            }
        }
    }
    
    // Pictures array
    assert!(json["pictures"].is_array(), "'pictures' must be array");
    // Picture fields are mostly optional
}
```

### 6. Performance Baseline Tests

**Goal:** Establish performance baselines for regression detection.

**Metrics:**
- Parse time per format
- Export time per format
- SPARQL query time
- Memory usage

**Not a pass/fail test**, but tracked in CI for regression detection.

## Test Execution

### Running Tests

```bash
# Run all compatibility tests
cargo test --test compatibility

# Run specific category
cargo test --test compatibility -- roundtrip
cargo test --test compatibility -- sparql
cargo test --test compatibility -- export_validation

# Run with output
cargo test --test compatibility -- --nocapture
```

### CI Integration

Compatibility tests run on every PR in GitHub Actions:

```yaml
- name: Compatibility Tests
  run: cargo test --test compatibility --verbose
```

## Known Differences and Acceptable Variances

### 1. Element IDs

- **Python docling:** Uses UUID or sequential integers
- **RuddyDoc:** Uses IRI format `urn:ruddydoc:doc:{hash}/{id}`
- **Status:** Acceptable (implementation detail)

### 2. Floating-Point Precision

- **Python docling:** Bounding boxes as `float`
- **RuddyDoc:** Bounding boxes as `f64`
- **Tolerance:** ±1% of page dimension or ±0.5 points
- **Status:** Acceptable

### 3. Confidence Scores

- **Python docling:** PyTorch inference
- **RuddyDoc:** ONNX Runtime inference
- **Tolerance:** ±0.05 absolute difference
- **Status:** Acceptable (different inference engines)

### 4. Whitespace Normalization

- **Python docling:** May preserve extra whitespace from source
- **RuddyDoc:** Normalizes consecutive whitespace to single space
- **Status:** Acceptable (semantic equivalence)

### 5. Reading Order for Ties

When multiple elements have identical spatial positions (e.g., side-by-side columns):

- **Python docling:** Left-to-right, top-to-bottom
- **RuddyDoc:** Left-to-right, top-to-bottom (same)
- **Tolerance:** Elements with overlapping bounding boxes may differ in order
- **Status:** Acceptable (both are valid reading orders)

### 6. Table Cell Merging

- **Python docling:** `rowspan`, `colspan` inferred from layout
- **RuddyDoc:** Same inference algorithm
- **Tolerance:** Complex merged cells may differ by ±1 cell
- **Status:** Investigate if difference >5% of cells

### 7. Code Language Detection

- **Python docling:** Uses heuristics
- **RuddyDoc:** Uses same heuristics + CommonMark info string
- **Status:** Acceptable (RuddyDoc may be more accurate)

### 8. Image Alt Text

- **Python docling:** From `alt` attribute or OCR
- **RuddyDoc:** From `alt` attribute or OCR
- **Status:** Should match exactly (investigate if different)

## Test Deliverables

### 1. Test Suite Code

- `/home/ldary/rh/chapeaux/ruddydoc/tests/compatibility/mod.rs` - Module root
- `/home/ldary/rh/chapeaux/ruddydoc/tests/compatibility/roundtrip.rs` - Round-trip tests
- `/home/ldary/rh/chapeaux/ruddydoc/tests/compatibility/cross_format.rs` - Cross-format tests
- `/home/ldary/rh/chapeaux/ruddydoc/tests/compatibility/export_validation.rs` - Export validation
- `/home/ldary/rh/chapeaux/ruddydoc/tests/compatibility/sparql.rs` - SPARQL correctness
- `/home/ldary/rh/chapeaux/ruddydoc/tests/compatibility/schema.rs` - JSON schema validation
- `/home/ldary/rh/chapeaux/ruddydoc/tests/compatibility/helpers.rs` - Shared test helpers

### 2. Documentation

- This file: `COMPATIBILITY_TEST_PLAN.md`
- `COVERAGE_REPORT.md` - Test coverage summary (generated)

### 3. CI Integration

- GitHub Actions workflow includes `cargo test --test compatibility`

## Success Criteria

The compatibility test suite is successful when:

1. All round-trip tests pass for Markdown, HTML, JSON
2. All export validation tests pass for all 10 output formats
3. All SPARQL correctness tests pass
4. All JSON schema validation tests pass
5. Known differences are documented and within tolerance
6. Tests are runnable with `cargo test --test compatibility`
7. Tests complete in <30 seconds on CI

## Future Enhancements

### Phase 6.2+ (Post-Initial Implementation)

1. **Python Docling Comparison Tests**
   - Run Python docling on same fixtures
   - Compare RuddyDoc output to Python output programmatically
   - Generate diff reports for structural differences

2. **Property-Based Testing**
   - Use `proptest` or `quickcheck` to generate random documents
   - Verify invariants hold (e.g., reading order always contiguous)

3. **Fuzzing**
   - Fuzz parsers with malformed inputs
   - Verify graceful error handling, no panics

4. **Visual Regression Testing**
   - Render HTML exports to PNG
   - Compare pixel diffs (for CSS/layout changes)

5. **Accessibility Testing**
   - Run axe-core on HTML exports
   - Verify WCAG 2.2 Level AA compliance

## Appendix: Test Fixture Inventory

### Current Fixtures in `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/`

- `sample.md` (1076 bytes) - Markdown with headings, lists, code, table, image
- `sample.html` (3016 bytes) - HTML with semantic elements
- `sample.csv` (329 bytes) - CSV with headers
- `sample.tex` (2547 bytes) - LaTeX document
- `sample.vtt` (773 bytes) - WebVTT subtitles
- `sample.adoc` (1879 bytes) - AsciiDoc
- `sample_jats.xml` (5146 bytes) - JATS article
- `sample_uspto.xml` (9633 bytes) - USPTO patent
- `semicolons.csv` (136 bytes) - Semicolon-delimited CSV
- `tabs.tsv` (127 bytes) - Tab-delimited TSV

### Python Docling Fixtures in `/home/ldary/rh/chapeaux/ruddydoc/tests/data/`

**Markdown:** `md/wiki_duck.md`, others  
**HTML:** `html/` directory (multiple files)  
**CSV:** `csv/simple.csv`, others  
**DOCX:** `docx/` directory  
**XLSX:** `xlsx/` directory  
**PPTX:** `pptx/` directory  
**LaTeX:** `latex/` directory  
**WebVTT:** `webvtt/` directory  
**JATS:** `jats/` directory  
**USPTO:** `uspto/` directory  

These can be used for compatibility testing once the corresponding backends are implemented.

---

**Document Status:** Active  
**Next Review:** After Phase 2 completion  
**Owner:** QA Engineer
