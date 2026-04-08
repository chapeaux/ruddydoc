//! Shared server state for the RuddyDoc server.
//!
//! [`ServerState`] holds the in-memory document store, converter, and a
//! registry of converted documents. All converted documents persist for
//! the lifetime of the server process.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

use ruddydoc_converter::DocumentConverter;
use ruddydoc_core::{DocumentMeta, DocumentSource, DocumentStore, OutputFormat};
use ruddydoc_export::{ChunkOptions, chunk_document, exporter_for};
use ruddydoc_graph::OxigraphStore;

// ---------------------------------------------------------------------------
// DocumentRecord
// ---------------------------------------------------------------------------

/// Metadata about a converted document stored in the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentRecord {
    /// Server-assigned document ID (UUID).
    pub id: String,
    /// The named graph IRI where this document's triples live.
    pub graph_iri: String,
    /// Document metadata (format, hash, page count, etc.).
    pub meta: DocumentMeta,
    /// When the document was converted (seconds since server start).
    #[serde(skip)]
    pub converted_at: Option<std::time::Instant>,
}

// ---------------------------------------------------------------------------
// ServerState
// ---------------------------------------------------------------------------

/// Shared server state.
///
/// Holds the in-memory Oxigraph store, the document converter, and a
/// map from document IDs to their records. All converted documents share
/// a single store, each in its own named graph.
pub struct ServerState {
    /// In-memory RDF store. All documents are stored here.
    pub store: Arc<OxigraphStore>,
    /// Document converter for processing uploaded files.
    pub converter: DocumentConverter,
    /// Map from document ID to its record.
    pub documents: Arc<RwLock<HashMap<String, DocumentRecord>>>,
}

impl ServerState {
    /// Create a new server state with a fresh in-memory store.
    pub fn new() -> ruddydoc_core::Result<Self> {
        let store = Arc::new(OxigraphStore::new()?);
        let converter = DocumentConverter::default_converter();
        Ok(Self {
            store,
            converter,
            documents: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Convert a file at the given path and store the result.
    ///
    /// Returns the [`DocumentRecord`] for the newly converted document.
    /// The conversion is dispatched to a blocking thread to avoid stalling
    /// the tokio runtime.
    pub async fn convert_file(&self, path: &str) -> ruddydoc_core::Result<DocumentRecord> {
        let path_buf = PathBuf::from(path);
        let source = DocumentSource::File(path_buf);

        // DocumentConverter is not Send, so we need to create a new one
        // inside the blocking task. The conversion result contains an
        // Arc<OxigraphStore> with its own store; we need to copy the
        // triples into our shared store.
        let store = Arc::clone(&self.store);
        let result = tokio::task::spawn_blocking(move || {
            let converter = DocumentConverter::default_converter();
            let conversion = converter.convert(source)?;

            if conversion.status != ruddydoc_core::ConversionStatus::Success {
                return Err("document conversion failed".into());
            }

            // Copy triples from the conversion's store into the shared store
            // by serializing as N-Triples and re-inserting via SPARQL-like
            // triple insertion. Instead, we query all triples from the
            // conversion graph and insert them into the shared store.
            let doc_graph = &conversion.doc_graph;
            let sparql = format!("SELECT ?s ?p ?o WHERE {{ GRAPH <{doc_graph}> {{ ?s ?p ?o }} }}");
            let rows = conversion.store.query_to_json(&sparql)?;

            if let Some(arr) = rows.as_array() {
                for row in arr {
                    let s = row.get("s").and_then(|v| v.as_str()).unwrap_or_default();
                    let p = row.get("p").and_then(|v| v.as_str()).unwrap_or_default();
                    let o = row.get("o").and_then(|v| v.as_str()).unwrap_or_default();

                    // Clean IRI wrappers
                    let s = s.trim_start_matches('<').trim_end_matches('>');
                    let p = p.trim_start_matches('<').trim_end_matches('>');

                    // Determine if the object is a literal or IRI
                    if o.starts_with('"') {
                        // Literal value: parse datatype
                        let (value, datatype) = parse_literal(o);
                        store.insert_literal(s, p, &value, &datatype, doc_graph)?;
                    } else {
                        let o = o.trim_start_matches('<').trim_end_matches('>');
                        store.insert_triple_into(s, p, o, doc_graph)?;
                    }
                }
            }

            // Also load ontology into the shared store if not already done
            let ont_count = store.triple_count_in(ruddydoc_ontology::ONTOLOGY_GRAPH)?;
            if ont_count == 0 {
                ruddydoc_ontology::load_ontology(store.as_ref())?;
            }

            Ok::<_, ruddydoc_core::Error>((conversion.input, conversion.doc_graph))
        })
        .await
        .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })??;

        let (meta, doc_graph) = result;
        let id = uuid::Uuid::new_v4().to_string();

        let record = DocumentRecord {
            id: id.clone(),
            graph_iri: doc_graph,
            meta,
            converted_at: Some(std::time::Instant::now()),
        };

        debug!(id = %id, format = %record.meta.format, "document converted");

        let mut docs = self.documents.write().await;
        docs.insert(id, record.clone());

        Ok(record)
    }

    /// Export a document in the given format.
    pub async fn export_document(&self, id: &str, format: &str) -> ruddydoc_core::Result<String> {
        let docs = self.documents.read().await;
        let record = docs.get(id).ok_or_else(|| -> ruddydoc_core::Error {
            format!("document '{id}' not found").into()
        })?;
        let doc_graph = record.graph_iri.clone();
        drop(docs);

        let output_format = parse_output_format(format)?;
        let store = Arc::clone(&self.store);

        tokio::task::spawn_blocking(move || {
            let exporter = exporter_for(output_format)?;
            exporter.export(store.as_ref(), &doc_graph)
        })
        .await
        .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Run a SPARQL query against a document's named graph.
    pub async fn query_document(
        &self,
        id: &str,
        sparql: &str,
    ) -> ruddydoc_core::Result<serde_json::Value> {
        let docs = self.documents.read().await;
        let record = docs.get(id).ok_or_else(|| -> ruddydoc_core::Error {
            format!("document '{id}' not found").into()
        })?;
        let doc_graph = record.graph_iri.clone();
        drop(docs);

        // Wrap the user's SPARQL in a GRAPH clause if it doesn't already
        // reference the graph. If the user query already contains GRAPH, pass
        // it through unchanged.
        let effective_sparql = if sparql.contains("GRAPH") {
            sparql.to_string()
        } else {
            // Replace the outermost WHERE { ... } with WHERE { GRAPH <g> { ... } }
            wrap_in_graph(sparql, &doc_graph)
        };

        let store = Arc::clone(&self.store);
        let query = effective_sparql.clone();

        tokio::task::spawn_blocking(move || store.query_to_json(&query))
            .await
            .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// List elements in a document, optionally filtered by type.
    pub async fn list_elements(
        &self,
        id: &str,
        element_type: Option<&str>,
    ) -> ruddydoc_core::Result<serde_json::Value> {
        let docs = self.documents.read().await;
        let record = docs.get(id).ok_or_else(|| -> ruddydoc_core::Error {
            format!("document '{id}' not found").into()
        })?;
        let doc_graph = record.graph_iri.clone();
        drop(docs);

        let ont = ruddydoc_ontology::NAMESPACE;
        let type_filter = match element_type {
            Some(t) => format!("FILTER(?type = <{ont}{t}>)"),
            None => String::new(),
        };

        let sparql = format!(
            "SELECT ?el ?type ?text ?order WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 ?el a ?type. \
                 ?el <{ont}readingOrder> ?order. \
                 OPTIONAL {{ ?el <{ont}textContent> ?text }} \
                 {type_filter} \
               }} \
             }} ORDER BY ?order"
        );

        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || store.query_to_json(&sparql))
            .await
            .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Chunk a document for RAG workflows.
    pub async fn chunk_document(
        &self,
        id: &str,
        max_tokens: usize,
    ) -> ruddydoc_core::Result<Vec<serde_json::Value>> {
        let docs = self.documents.read().await;
        let record = docs.get(id).ok_or_else(|| -> ruddydoc_core::Error {
            format!("document '{id}' not found").into()
        })?;
        let doc_graph = record.graph_iri.clone();
        drop(docs);

        let store = Arc::clone(&self.store);
        let options = ChunkOptions {
            max_tokens,
            ..Default::default()
        };

        tokio::task::spawn_blocking(move || {
            let chunks = chunk_document(store.as_ref(), &doc_graph, &options)?;
            let json_chunks: Vec<serde_json::Value> = chunks
                .into_iter()
                .map(|c| serde_json::to_value(c).unwrap_or(serde_json::Value::Null))
                .collect();
            Ok::<_, ruddydoc_core::Error>(json_chunks)
        })
        .await
        .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a literal string from SPARQL results into (value, datatype).
///
/// Input formats:
/// - `"hello"` -> ("hello", "string")
/// - `"42"^^<http://www.w3.org/2001/XMLSchema#integer>` -> ("42", "integer")
fn parse_literal(s: &str) -> (String, String) {
    if let Some(idx) = s.find("\"^^<") {
        let value = &s[1..idx];
        let dt_iri = &s[idx + 4..s.len() - 1]; // strip ^^< and >
        let datatype = if let Some(frag) = dt_iri.rfind('#') {
            dt_iri[frag + 1..].to_string()
        } else {
            "string".to_string()
        };
        (value.to_string(), datatype)
    } else if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        (s[1..s.len() - 1].to_string(), "string".to_string())
    } else {
        (s.to_string(), "string".to_string())
    }
}

/// Parse an output format string into an [`OutputFormat`].
fn parse_output_format(s: &str) -> ruddydoc_core::Result<OutputFormat> {
    match s.to_lowercase().as_str() {
        "json" => Ok(OutputFormat::Json),
        "markdown" | "md" => Ok(OutputFormat::Markdown),
        "html" => Ok(OutputFormat::Html),
        "text" | "txt" => Ok(OutputFormat::Text),
        "turtle" | "ttl" => Ok(OutputFormat::Turtle),
        "ntriples" | "nt" => Ok(OutputFormat::NTriples),
        _ => Err(format!("unsupported output format: '{s}'").into()),
    }
}

/// Wrap a SPARQL query body in a GRAPH clause.
///
/// This is a best-effort transform: it replaces the first `WHERE {` with
/// `WHERE { GRAPH <graph> {` and appends a closing `}`.
fn wrap_in_graph(sparql: &str, graph: &str) -> String {
    // Look for WHERE (case-insensitive)
    let upper = sparql.to_uppercase();
    if let Some(where_pos) = upper.find("WHERE") {
        // Find the opening brace after WHERE
        if let Some(brace_pos) = sparql[where_pos..].find('{') {
            let abs_brace = where_pos + brace_pos;
            let before = &sparql[..abs_brace + 1];
            let after = &sparql[abs_brace + 1..];
            // Find the last closing brace
            if let Some(last_brace) = after.rfind('}') {
                let inner = &after[..last_brace];
                let trailing = &after[last_brace + 1..];
                return format!("{before} GRAPH <{graph}> {{{inner}}} }}{trailing}");
            }
        }
    }
    // Fallback: return as-is
    sparql.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_literal_typed() {
        let (val, dt) = parse_literal("\"42\"^^<http://www.w3.org/2001/XMLSchema#integer>");
        assert_eq!(val, "42");
        assert_eq!(dt, "integer");
    }

    #[test]
    fn parse_literal_plain() {
        let (val, dt) = parse_literal("\"hello world\"");
        assert_eq!(val, "hello world");
        assert_eq!(dt, "string");
    }

    #[test]
    fn parse_output_format_json() {
        let f = parse_output_format("json").unwrap();
        assert_eq!(f, OutputFormat::Json);
    }

    #[test]
    fn parse_output_format_turtle() {
        let f = parse_output_format("turtle").unwrap();
        assert_eq!(f, OutputFormat::Turtle);
    }

    #[test]
    fn parse_output_format_invalid() {
        let result = parse_output_format("xyz");
        assert!(result.is_err());
    }

    #[test]
    fn wrap_in_graph_basic() {
        let sparql = "SELECT ?s ?p ?o WHERE { ?s ?p ?o }";
        let wrapped = wrap_in_graph(sparql, "urn:test:graph");
        assert!(wrapped.contains("GRAPH <urn:test:graph>"));
        assert!(wrapped.contains("?s ?p ?o"));
    }

    #[test]
    fn wrap_in_graph_preserves_existing_graph() {
        let sparql = "SELECT ?s WHERE { GRAPH <urn:other> { ?s ?p ?o } }";
        // wrap_in_graph is only called when the query does NOT contain GRAPH,
        // but let's verify the wrapping still produces valid-ish SPARQL
        let wrapped = wrap_in_graph(sparql, "urn:test:graph");
        assert!(wrapped.contains("GRAPH"));
    }
}
