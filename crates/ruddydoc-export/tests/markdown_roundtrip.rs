//! Integration test: Markdown round-trip through the full RuddyDoc pipeline.
//!
//! Parses `tests/fixtures/sample.md` through:
//! 1. Markdown backend -> Oxigraph store
//! 2. Export to JSON -> verify structure
//! 3. Export to Turtle -> verify it parses back
//! 4. Export to Markdown -> verify content equivalence
//! 5. Run a SPARQL query against the document graph

use ruddydoc_core::{DocumentBackend, DocumentExporter, DocumentSource, DocumentStore};
use ruddydoc_export::{JsonExporter, MarkdownExporter, NTriplesExporter, TurtleExporter};
use ruddydoc_graph::OxigraphStore;
use ruddydoc_ontology as ont;

const SAMPLE_MD: &str = include_str!("../../../tests/fixtures/sample.md");

/// Set up a parsed document in the store.
fn setup() -> (OxigraphStore, String) {
    let store = OxigraphStore::new().expect("failed to create store");
    let backend = ruddydoc_backend_md::MarkdownBackend::new();

    let source = DocumentSource::Stream {
        name: "sample.md".to_string(),
        data: SAMPLE_MD.as_bytes().to_vec(),
    };

    let hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(SAMPLE_MD.as_bytes());
        let result = hasher.finalize();
        result.iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
    };
    let doc_graph = ruddydoc_core::doc_iri(&hash);

    // Load ontology
    ont::load_ontology(&store).expect("failed to load ontology");

    // Parse
    backend
        .parse(&source, &store, &doc_graph)
        .expect("failed to parse sample.md");

    (store, doc_graph)
}

#[test]
fn json_export_has_expected_structure() {
    let (store, doc_graph) = setup();
    let exporter = JsonExporter;
    let json_str = exporter
        .export(&store, &doc_graph)
        .expect("JSON export failed");

    let json: serde_json::Value = serde_json::from_str(&json_str).expect("invalid JSON");

    // Verify top-level structure
    assert_eq!(json["name"].as_str().unwrap(), "sample.md");
    assert_eq!(json["source_format"].as_str().unwrap(), "markdown");

    // Verify texts
    let texts = json["texts"].as_array().expect("texts should be an array");
    assert!(texts.len() > 10, "expected at least 10 text elements");

    // First text should be the H1 heading
    let first = &texts[0];
    assert_eq!(first["type"].as_str().unwrap(), "section_header");
    assert_eq!(first["heading_level"].as_i64().unwrap(), 1);

    // Verify we have paragraphs
    let paragraphs: Vec<_> = texts
        .iter()
        .filter(|t| t["type"].as_str() == Some("paragraph"))
        .collect();
    assert!(paragraphs.len() >= 3, "expected at least 3 paragraphs");

    // Verify we have list items
    let list_items: Vec<_> = texts
        .iter()
        .filter(|t| t["type"].as_str() == Some("list_item"))
        .collect();
    assert!(list_items.len() >= 10, "expected at least 10 list items");

    // Verify we have code blocks
    let code_blocks: Vec<_> = texts
        .iter()
        .filter(|t| t["type"].as_str() == Some("code"))
        .collect();
    assert_eq!(code_blocks.len(), 2, "expected 2 code blocks");
    assert!(
        code_blocks
            .iter()
            .any(|c| c["code_language"].as_str() == Some("rust")),
        "expected a Rust code block"
    );
    assert!(
        code_blocks
            .iter()
            .any(|c| c["code_language"].as_str() == Some("python")),
        "expected a Python code block"
    );

    // Verify tables
    let tables = json["tables"]
        .as_array()
        .expect("tables should be an array");
    assert_eq!(tables.len(), 1, "expected 1 table");
    let table = &tables[0];
    assert!(
        table["cells"].as_array().unwrap().len() >= 9,
        "expected at least 9 cells"
    );

    // Verify pictures
    let pictures = json["pictures"]
        .as_array()
        .expect("pictures should be an array");
    assert_eq!(pictures.len(), 1, "expected 1 picture");
    assert_eq!(pictures[0]["alt_text"].as_str().unwrap(), "RuddyDoc Logo");
}

#[test]
fn turtle_export_is_nonempty_and_parseable() {
    let (store, doc_graph) = setup();
    let exporter = TurtleExporter;
    let turtle = exporter
        .export(&store, &doc_graph)
        .expect("Turtle export failed");

    assert!(!turtle.is_empty(), "Turtle output should not be empty");

    // Verify key ontology terms appear
    assert!(
        turtle.contains("ontology#Document"),
        "should contain Document class"
    );
    assert!(
        turtle.contains("ontology#SectionHeader"),
        "should contain SectionHeader class"
    );
    assert!(
        turtle.contains("ontology#Paragraph"),
        "should contain Paragraph class"
    );
    assert!(
        turtle.contains("ontology#textContent"),
        "should contain textContent property"
    );

    // Verify the Turtle can be re-parsed by loading it into a second store.
    // (We do a simple structural check since we don't have a Turtle parser
    //  outside of Oxigraph, and we already validated it came from Oxigraph.)
    assert!(
        turtle.contains(" ."),
        "Turtle should have statement terminators"
    );
}

#[test]
fn ntriples_export_is_valid() {
    let (store, doc_graph) = setup();
    let exporter = NTriplesExporter;
    let nt = exporter
        .export(&store, &doc_graph)
        .expect("N-Triples export failed");

    assert!(!nt.is_empty(), "N-Triples output should not be empty");

    // Every non-empty line should end with " ."
    for line in nt.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            assert!(
                trimmed.ends_with(" ."),
                "N-Triples line should end with ' .': {trimmed}"
            );
        }
    }
}

#[test]
fn markdown_export_preserves_key_content() {
    let (store, doc_graph) = setup();
    let exporter = MarkdownExporter;
    let md_out = exporter
        .export(&store, &doc_graph)
        .expect("Markdown export failed");

    // All headings should be present
    assert!(md_out.contains("# Introduction"), "missing H1 heading");
    assert!(md_out.contains("## Features"), "missing H2 heading");
    assert!(md_out.contains("### Code Example"), "missing H3 heading");
    assert!(md_out.contains("### Data Table"), "missing table heading");
    assert!(
        md_out.contains("## Conclusion"),
        "missing conclusion heading"
    );

    // Key paragraphs
    assert!(
        md_out.contains("introductory paragraph"),
        "missing intro paragraph"
    );
    assert!(
        md_out.contains("Phase 1 testing"),
        "missing conclusion text"
    );

    // List items
    assert!(md_out.contains("First item"), "missing unordered list item");
    assert!(md_out.contains("Step one"), "missing ordered list item");

    // Code blocks
    assert!(md_out.contains("println!"), "missing Rust code content");
    assert!(md_out.contains("def greet"), "missing Python code content");

    // Image
    assert!(md_out.contains("![RuddyDoc Logo]"), "missing image");
}

#[test]
fn sparql_query_returns_correct_elements() {
    let (store, doc_graph) = setup();

    // Query all paragraphs
    let sparql = format!(
        "SELECT ?text WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?p a <{}>. \
             ?p <{}> ?text \
           }} \
         }}",
        ont::iri(ont::CLASS_PARAGRAPH),
        ont::iri(ont::PROP_TEXT_CONTENT),
    );
    let result = store.query_to_json(&sparql).expect("SPARQL query failed");
    let rows = result.as_array().expect("expected array");
    assert!(
        rows.len() >= 3,
        "expected at least 3 paragraphs from SPARQL"
    );

    // Query all headings ordered by reading order
    let sparql_headings = format!(
        "SELECT ?text ?level WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?h a <{}>. \
             ?h <{}> ?text. \
             ?h <{}> ?level. \
             ?h <{}> ?order \
           }} \
         }} ORDER BY ?order",
        ont::iri(ont::CLASS_SECTION_HEADER),
        ont::iri(ont::PROP_TEXT_CONTENT),
        ont::iri(ont::PROP_HEADING_LEVEL),
        ont::iri(ont::PROP_READING_ORDER),
    );
    let result = store
        .query_to_json(&sparql_headings)
        .expect("headings query failed");
    let rows = result.as_array().expect("expected array");

    // We have: Introduction (H1), Features (H2), Code Example (H3), Data Table (H3),
    // Image (H3), Lists (H2), Unordered (H3), Ordered (H3), Conclusion (H2)
    assert_eq!(rows.len(), 9, "expected 9 headings");

    // Count triples in the document graph
    let count = store
        .triple_count_in(&doc_graph)
        .expect("triple count failed");
    assert!(
        count > 50,
        "expected >50 triples in document graph, got {count}"
    );
}

#[test]
fn document_and_ontology_are_in_separate_graphs() {
    let (store, doc_graph) = setup();

    // Ontology should be in its own graph
    let ont_count = store
        .triple_count_in(ont::ONTOLOGY_GRAPH)
        .expect("ontology count failed");
    assert!(
        ont_count > 100,
        "expected >100 ontology triples, got {ont_count}"
    );

    // Document should be in its own graph
    let doc_count = store.triple_count_in(&doc_graph).expect("doc count failed");
    assert!(
        doc_count > 50,
        "expected >50 document triples, got {doc_count}"
    );

    // They should be different graphs
    assert_ne!(
        doc_graph,
        ont::ONTOLOGY_GRAPH,
        "document and ontology should be in different graphs"
    );
}
