//! Shared test helpers for compatibility testing.

use ruddydoc_core::{DocumentBackend, DocumentExporter, DocumentSource, DocumentStore};
use ruddydoc_graph::OxigraphStore;
use ruddydoc_ontology as ont;
use sha2::{Digest, Sha256};
use serde_json::Value;
use std::path::Path;

/// Parse a file and return the store and document graph IRI.
/// Path should be relative to workspace root.
pub fn parse_file<B: DocumentBackend>(
    backend: &B,
    path: impl AsRef<Path>,
) -> (OxigraphStore, String) {
    // Resolve path relative to workspace root
    let workspace_root = std::env::var("CARGO_MANIFEST_DIR")
        .map(|p| std::path::PathBuf::from(p).parent().unwrap().parent().unwrap().to_path_buf())
        .unwrap_or_else(|_| std::env::current_dir().unwrap());

    let full_path = workspace_root.join(path.as_ref());
    let data = std::fs::read(&full_path).expect(&format!("failed to read file: {:?}", full_path));
    parse_bytes(backend, full_path.file_name().unwrap().to_str().unwrap(), &data)
}

/// Parse bytes and return the store and document graph IRI.
pub fn parse_bytes<B: DocumentBackend>(
    backend: &B,
    name: &str,
    data: &[u8],
) -> (OxigraphStore, String) {
    let store = OxigraphStore::new().expect("failed to create store");
    ont::load_ontology(&store).expect("failed to load ontology");

    let hash = compute_hash(data);
    let doc_graph = ruddydoc_core::doc_iri(&hash);

    let source = DocumentSource::Stream {
        name: name.to_string(),
        data: data.to_vec(),
    };

    backend
        .parse(&source, &store, &doc_graph)
        .expect("parse failed");

    (store, doc_graph)
}

/// Parse a string as a document.
pub fn parse_string<B: DocumentBackend>(
    backend: &B,
    name: &str,
    content: &str,
) -> (OxigraphStore, String) {
    parse_bytes(backend, name, content.as_bytes())
}

/// Compute SHA256 hash of data.
pub fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Count elements of a given class in the document graph.
pub fn count_elements(store: &OxigraphStore, graph: &str, class: &str) -> usize {
    let sparql = format!(
        "SELECT (COUNT(?e) AS ?count) WHERE {{ GRAPH <{graph}> {{ ?e a <{}> }} }}",
        ont::iri(class)
    );
    let result = store.query_to_json(&sparql).expect("query failed");
    parse_int_result(&result, "count")
}

/// Count all paragraphs.
pub fn count_paragraphs(store: &OxigraphStore, graph: &str) -> usize {
    count_elements(store, graph, ont::CLASS_PARAGRAPH)
}

/// Count all section headers.
pub fn count_headings(store: &OxigraphStore, graph: &str) -> usize {
    count_elements(store, graph, ont::CLASS_SECTION_HEADER)
}

/// Count all list items.
pub fn count_list_items(store: &OxigraphStore, graph: &str) -> usize {
    count_elements(store, graph, ont::CLASS_LIST_ITEM)
}

/// Count all code blocks.
pub fn count_code_blocks(store: &OxigraphStore, graph: &str) -> usize {
    count_elements(store, graph, ont::CLASS_CODE)
}

/// Count all tables.
pub fn count_tables(store: &OxigraphStore, graph: &str) -> usize {
    count_elements(store, graph, ont::CLASS_TABLE_ELEMENT)
}

/// Count all pictures.
pub fn count_pictures(store: &OxigraphStore, graph: &str) -> usize {
    count_elements(store, graph, ont::CLASS_PICTURE_ELEMENT)
}

/// Get reading orders of all elements.
pub fn get_reading_orders(store: &OxigraphStore, graph: &str) -> Vec<i64> {
    let sparql = format!(
        "SELECT ?order WHERE {{ GRAPH <{graph}> {{ ?e <{}> ?order }} }} ORDER BY ?order",
        ont::iri(ont::PROP_READING_ORDER)
    );
    let result = store.query_to_json(&sparql).expect("query failed");
    result
        .as_array()
        .unwrap()
        .iter()
        .map(|row| parse_int(row["order"].as_str().unwrap()))
        .collect()
}

/// Parse an integer from a SPARQL result value.
pub fn parse_int(s: &str) -> i64 {
    let cleaned = clean_literal(s);
    cleaned.parse().unwrap_or(0)
}

/// Parse an integer result from a COUNT query.
fn parse_int_result(result: &Value, field: &str) -> usize {
    result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get(field))
        .and_then(|v| v.as_str())
        .map(parse_int)
        .unwrap_or(0) as usize
}

/// Clean a SPARQL literal (remove quotes and datatype suffix).
pub fn clean_literal(s: &str) -> String {
    // Handle format: "value"^^<http://...>
    if let Some(idx) = s.find("\"^^<") {
        return s[1..idx].to_string();
    }
    // Handle format: "value"
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// Export and parse result as JSON.
pub fn export_json(store: &OxigraphStore, graph: &str) -> Value {
    let exporter = ruddydoc_export::JsonExporter;
    let json_str = exporter.export(store, graph).expect("export failed");
    serde_json::from_str(&json_str).expect("invalid JSON")
}

/// Validate docling JSON schema (non-panicking version).
pub fn validate_docling_json(json: &Value) -> Result<(), String> {
    // Top-level fields
    if json.get("name").is_none() {
        return Err("missing 'name' field".to_string());
    }
    if json.get("source_format").is_none() {
        return Err("missing 'source_format' field".to_string());
    }

    // Texts array
    if !json["texts"].is_array() {
        return Err("'texts' must be an array".to_string());
    }

    for (i, text) in json["texts"].as_array().unwrap().iter().enumerate() {
        if text.get("type").is_none() {
            return Err(format!("text[{i}] missing 'type'"));
        }
        if text.get("text").is_none() {
            return Err(format!("text[{i}] missing 'text'"));
        }
        if text.get("reading_order").is_none() {
            return Err(format!("text[{i}] missing 'reading_order'"));
        }

        // Type-specific validation
        if text["type"] == "section_header" && text.get("heading_level").is_none() {
            return Err(format!("text[{i}] is section_header but missing 'heading_level'"));
        }
    }

    // Tables array
    if !json["tables"].is_array() {
        return Err("'tables' must be an array".to_string());
    }

    for (i, table) in json["tables"].as_array().unwrap().iter().enumerate() {
        if table.get("cells").is_none() && table.get("row_count").is_none() {
            return Err(format!("table[{i}] must have 'cells' or 'row_count'"));
        }

        if let Some(cells) = table.get("cells") {
            if !cells.is_array() {
                return Err(format!("table[{i}] cells must be array"));
            }
            for (j, cell) in cells.as_array().unwrap().iter().enumerate() {
                if cell.get("text").is_none() {
                    return Err(format!("table[{i}] cell[{j}] missing 'text'"));
                }
                if cell.get("row").is_none() {
                    return Err(format!("table[{i}] cell[{j}] missing 'row'"));
                }
                if cell.get("col").is_none() {
                    return Err(format!("table[{i}] cell[{j}] missing 'col'"));
                }
                if cell.get("is_header").is_none() {
                    return Err(format!("table[{i}] cell[{j}] missing 'is_header'"));
                }
            }
        }
    }

    // Pictures array
    if !json["pictures"].is_array() {
        return Err("'pictures' must be an array".to_string());
    }

    Ok(())
}

/// Normalize text for comparison (whitespace, Unicode).
pub fn normalize_text(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;

    // Normalize to NFC
    let nfc: String = s.nfc().collect();

    // Collapse whitespace
    nfc.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Check if two texts are equivalent after normalization.
pub fn texts_equivalent(a: &str, b: &str) -> bool {
    normalize_text(a) == normalize_text(b)
}
