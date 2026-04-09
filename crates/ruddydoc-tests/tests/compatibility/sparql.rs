//! SPARQL correctness tests.

use super::helpers::*;
use ruddydoc_core::DocumentStore;
use ruddydoc_ontology as ont;

#[test]
fn sparql_count_paragraphs_matches_fixture() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let count = count_paragraphs(&store, &graph);

    // sample.md has 6 paragraphs (intro, features description,
    // code-block lead-ins, conclusion, and block quote content)
    assert_eq!(count, 6, "expected 6 paragraphs in sample.md");
}

#[test]
fn sparql_reading_order_is_contiguous() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let orders = get_reading_orders(&store, &graph);

    // Verify contiguous sequence starting at 0
    for (i, &order) in orders.iter().enumerate() {
        assert_eq!(
            order, i as i64,
            "reading order has gap: expected {i}, got {order}"
        );
    }
}

#[test]
fn sparql_all_text_elements_have_content() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    // Query for text elements without content
    let sparql = format!(
        "SELECT ?e WHERE {{ \
           GRAPH <{graph}> {{ \
             ?e a <{}>. \
             FILTER NOT EXISTS {{ ?e <{}> ?t }} \
           }} \
         }}",
        ont::iri(ont::CLASS_TEXT_ELEMENT),
        ont::iri(ont::PROP_TEXT_CONTENT)
    );

    let result = store.query_to_json(&sparql).expect("query failed");
    let rows = result.as_array().unwrap();

    assert_eq!(
        rows.len(),
        0,
        "found {} text elements without content",
        rows.len()
    );
}

#[test]
fn sparql_all_headings_have_levels() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    // Query for headings without levels
    let sparql = format!(
        "SELECT ?e WHERE {{ \
           GRAPH <{graph}> {{ \
             ?e a <{}>. \
             FILTER NOT EXISTS {{ ?e <{}> ?level }} \
           }} \
         }}",
        ont::iri(ont::CLASS_SECTION_HEADER),
        ont::iri(ont::PROP_HEADING_LEVEL)
    );

    let result = store.query_to_json(&sparql).expect("query failed");
    let rows = result.as_array().unwrap();

    assert_eq!(
        rows.len(),
        0,
        "found {} headings without heading_level",
        rows.len()
    );
}

#[test]
fn sparql_table_cells_have_positions() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    // Query all table cells
    let sparql = format!(
        "SELECT ?cell ?row ?col WHERE {{ \
           GRAPH <{graph}> {{ \
             ?table a <{}>. \
             ?table <{}> ?cell. \
             ?cell <{}> ?row. \
             ?cell <{}> ?col \
           }} \
         }}",
        ont::iri(ont::CLASS_TABLE_ELEMENT),
        ont::iri(ont::PROP_HAS_CELL),
        ont::iri(ont::PROP_CELL_ROW),
        ont::iri(ont::PROP_CELL_COLUMN)
    );

    let result = store.query_to_json(&sparql).expect("query failed");
    let cells = result.as_array().unwrap();

    // sample.md has 1 table with 3x3 = 9 cells
    assert!(cells.len() >= 9, "expected at least 9 table cells");

    // Verify all cells have valid row/col positions (non-negative)
    for (i, cell) in cells.iter().enumerate() {
        let row = parse_int(cell["row"].as_str().unwrap());
        let col = parse_int(cell["col"].as_str().unwrap());

        assert!(row >= 0, "cell {i} has negative row: {row}");
        assert!(col >= 0, "cell {i} has negative col: {col}");
    }
}

#[test]
fn sparql_no_duplicate_cell_positions() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    // Query all table cells with positions
    let sparql = format!(
        "SELECT ?table ?row ?col WHERE {{ \
           GRAPH <{graph}> {{ \
             ?table a <{}>. \
             ?table <{}> ?cell. \
             ?cell <{}> ?row. \
             ?cell <{}> ?col \
           }} \
         }}",
        ont::iri(ont::CLASS_TABLE_ELEMENT),
        ont::iri(ont::PROP_HAS_CELL),
        ont::iri(ont::PROP_CELL_ROW),
        ont::iri(ont::PROP_CELL_COLUMN)
    );

    let result = store.query_to_json(&sparql).expect("query failed");
    let cells = result.as_array().unwrap();

    // Group by table
    use std::collections::{HashMap, HashSet};
    let mut table_cells: HashMap<String, HashSet<(i64, i64)>> = HashMap::new();

    for cell in cells {
        let table = cell["table"].as_str().unwrap().to_string();
        let row = parse_int(cell["row"].as_str().unwrap());
        let col = parse_int(cell["col"].as_str().unwrap());

        let positions = table_cells.entry(table.clone()).or_default();
        let is_new = positions.insert((row, col));

        assert!(
            is_new,
            "duplicate cell position in table {table}: ({row}, {col})"
        );
    }
}

#[test]
fn sparql_hierarchy_is_bidirectional() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    // Query: children that aren't in parent's child list
    let sparql = format!(
        "SELECT ?child WHERE {{ \
           GRAPH <{graph}> {{ \
             ?child <{}> ?parent. \
             FILTER NOT EXISTS {{ ?parent <{}> ?child }} \
           }} \
         }}",
        ont::iri(ont::PROP_PARENT_ELEMENT),
        ont::iri(ont::PROP_CHILD_ELEMENT)
    );

    let result = store.query_to_json(&sparql).expect("query failed");
    let rows = result.as_array().unwrap();

    assert_eq!(
        rows.len(),
        0,
        "found {} elements with broken parent-child relationship",
        rows.len()
    );
}

#[test]
fn sparql_all_elements_have_reading_order() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    // Query for document elements without reading order
    let sparql = format!(
        "SELECT ?e WHERE {{ \
           GRAPH <{graph}> {{ \
             ?e a <{}>. \
             FILTER NOT EXISTS {{ ?e <{}> ?order }} \
           }} \
         }}",
        ont::iri(ont::CLASS_DOCUMENT_ELEMENT),
        ont::iri(ont::PROP_READING_ORDER)
    );

    let result = store.query_to_json(&sparql).expect("query failed");
    let rows = result.as_array().unwrap();

    assert_eq!(
        rows.len(),
        0,
        "found {} elements without reading_order",
        rows.len()
    );
}

#[test]
fn sparql_code_blocks_have_language() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    // Query code blocks
    let sparql = format!(
        "SELECT ?code ?lang WHERE {{ \
           GRAPH <{graph}> {{ \
             ?code a <{}>. \
             OPTIONAL {{ ?code <{}> ?lang }} \
           }} \
         }}",
        ont::iri(ont::CLASS_CODE),
        ont::iri(ont::PROP_CODE_LANGUAGE)
    );

    let result = store.query_to_json(&sparql).expect("query failed");
    let rows = result.as_array().unwrap();

    // sample.md has 2 code blocks (Rust and Python)
    assert_eq!(rows.len(), 2, "expected 2 code blocks");

    // Both should have language specified
    let with_lang = rows
        .iter()
        .filter(|r: &&serde_json::Value| {
            r.get("lang")
                .and_then(|v: &serde_json::Value| v.as_str())
                .filter(|s: &&str| *s != "null")
                .is_some()
        })
        .count();

    assert_eq!(with_lang, 2, "expected both code blocks to have language");
}

#[test]
fn sparql_triple_count_is_reasonable() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let count = store.triple_count_in(&graph).expect("count failed");

    // sample.md should produce >50 triples
    // (each element creates multiple triples: type, content, order, etc.)
    assert!(
        count > 50,
        "expected >50 triples for sample.md, got {count}"
    );

    // But not an unreasonable number (sanity check)
    assert!(count < 1000, "triple count seems excessive: {count}");
}
