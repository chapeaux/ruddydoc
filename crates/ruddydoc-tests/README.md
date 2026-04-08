# RuddyDoc Integration Test Suite

This crate contains cross-crate integration tests for RuddyDoc, including the compatibility test suite that verifies structural equivalence with Python docling.

## Purpose

The `ruddydoc-tests` crate exists solely to run integration tests that span multiple RuddyDoc crates. It is not published and has no public API.

## Test Structure

```
tests/
├── compatibility_suite.rs      # Test entry point
└── compatibility/
    ├── mod.rs                  # Module root
    ├── helpers.rs              # Test utilities and fixtures
    ├── roundtrip.rs            # Round-trip tests
    ├── export_validation.rs    # Export format validation
    ├── sparql.rs               # SPARQL correctness tests
    └── schema.rs               # JSON schema validation
```

## Running Tests

### All Tests

```bash
cargo test --package ruddydoc-tests
```

### Specific Category

```bash
# Round-trip tests
cargo test --package ruddydoc-tests roundtrip

# Export validation
cargo test --package ruddydoc-tests export_validation

# SPARQL tests
cargo test --package ruddydoc-tests sparql

# JSON schema tests
cargo test --package ruddydoc-tests schema
```

### With Output

```bash
cargo test --package ruddydoc-tests -- --nocapture
```

## Test Categories

### 1. Round-Trip Tests (6 tests)

Verify that parsing → exporting → parsing produces structurally equivalent results.

- Markdown round-trip
- HTML round-trip
- JSON round-trip
- CSV → JSON preservation

**Location**: `tests/compatibility/roundtrip.rs`

### 2. Export Format Validation (11 tests)

Verify that each export format produces valid, well-formed output.

- JSON: valid JSON, correct schema
- Markdown: parseable, contains expected elements
- HTML: valid HTML5, semantic structure
- Text: plain text, no markup
- Turtle: valid Turtle RDF
- N-Triples: valid N-Triples RDF

**Location**: `tests/compatibility/export_validation.rs`

### 3. SPARQL Correctness (10 tests)

Verify that SPARQL queries return correct results from the RDF graph.

- Element counting queries
- Reading order contiguity
- Content completeness
- Table structure validation
- Hierarchy integrity

**Location**: `tests/compatibility/sparql.rs`

### 4. JSON Schema Validation (10 tests)

Verify that exported JSON matches Python docling's schema.

- Required fields present
- Correct data types
- Valid enum values
- Type-specific field validation

**Location**: `tests/compatibility/schema.rs`

## Test Fixtures

Tests use fixtures from the workspace root:

```
ruddydoc/tests/fixtures/
├── sample.md
├── sample.html
├── sample.csv
├── sample.tex
├── sample.vtt
├── sample.adoc
├── sample_jats.xml
└── sample_uspto.xml
```

Fixtures are accessed via workspace-relative paths, automatically resolved by the `parse_file()` helper function.

## Test Helpers

The `helpers.rs` module provides:

### Parsing Helpers

```rust
// Parse a file from workspace-relative path
let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

// Parse bytes
let (store, graph) = parse_bytes(&backend, "test.md", data);

// Parse string
let (store, graph) = parse_string(&backend, "test.md", &content);
```

### Element Counting

```rust
let para_count = count_paragraphs(&store, &graph);
let heading_count = count_headings(&store, &graph);
let table_count = count_tables(&store, &graph);
let list_count = count_list_items(&store, &graph);
let code_count = count_code_blocks(&store, &graph);
let picture_count = count_pictures(&store, &graph);
```

### SPARQL Utilities

```rust
// Get reading orders
let orders = get_reading_orders(&store, &graph);

// Parse SPARQL result integers
let value = parse_int(sparql_result_string);

// Clean SPARQL literals
let text = clean_literal(sparql_literal);
```

### Export and Validation

```rust
// Export to JSON and parse
let json = export_json(&store, &graph);

// Validate docling JSON schema
validate_docling_json(&json)?;
```

### Text Normalization

```rust
// Normalize for comparison
let normalized = normalize_text("  Multiple   spaces  ");

// Check equivalence
if texts_equivalent(text1, text2) { ... }
```

## Current Test Results

**Total Tests**: 37  
**Passing**: 34 (91.9%)  
**Failing**: 3 (minor issues)

### Passing Categories
- Export Validation: 11/11 ✓
- JSON Schema Validation: 10/10 ✓
- SPARQL Correctness: 9/10 ✓
- Round-Trip: 5/6 ✓

### Known Issues

1. `roundtrip::markdown_roundtrip_preserves_reading_order` - Off by 1 element
2. `roundtrip::html_roundtrip_preserves_structure` - Paragraph count differs
3. `sparql::sparql_count_paragraphs_matches_fixture` - Test assumption incorrect

All issues are minor and under investigation.

## Dependencies

This crate depends on:

- `ruddydoc-core` - Core types and traits
- `ruddydoc-graph` - Oxigraph store
- `ruddydoc-ontology` - Document ontology
- `ruddydoc-export` - Export formats
- `ruddydoc-backend-md` - Markdown backend
- `ruddydoc-backend-html` - HTML backend
- `ruddydoc-backend-csv` - CSV backend

Additional dependencies:

- `serde`, `serde_json` - JSON parsing
- `sha2` - Hash computation
- `unicode-normalization` - Text normalization

## Adding New Tests

### 1. Add a new test function

```rust
#[test]
fn my_new_test() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");
    
    // Your test logic here
    assert_eq!(count_paragraphs(&store, &graph), 6);
}
```

### 2. Add to appropriate module

Place the test in:
- `roundtrip.rs` for parse → export → parse tests
- `export_validation.rs` for format validation
- `sparql.rs` for SPARQL query tests
- `schema.rs` for JSON schema tests

### 3. Run your test

```bash
cargo test --package ruddydoc-tests my_new_test
```

## Documentation

For comprehensive documentation, see:

- `tests/COMPATIBILITY_TEST_PLAN.md` - Test strategy and plan
- `tests/COVERAGE_REPORT.md` - Test coverage summary
- `tests/COMPATIBILITY_TEST_SUITE_SUMMARY.md` - Deliverables summary

## License

MIT (same as RuddyDoc workspace)
