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
use ruddydoc_graph::SparqStore;
use ruddydoc_models::{
    ApiEmbeddingModel, ApiEmbeddingOptions, ApiLlmModel, ApiLlmOptions, EmbeddingProvider,
    LlmProvider,
};

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
    /// SHACL validation report from the initial conversion (`conforms` +
    /// `results`). Does not reflect any changes made after conversion --
    /// use the `validate_document` tool/method to re-check on demand.
    pub validation: Option<serde_json::Value>,
    /// When the document was converted (seconds since server start).
    #[serde(skip)]
    pub converted_at: Option<std::time::Instant>,
}

// ---------------------------------------------------------------------------
// ServerState
// ---------------------------------------------------------------------------

/// Shared server state.
///
/// Holds the in-memory Sparq store, the document converter, and a
/// map from document IDs to their records. All converted documents share
/// a single store, each in its own named graph.
pub struct ServerState {
    /// In-memory RDF store. All documents are stored here.
    pub store: Arc<dyn DocumentStore>,
    /// The same store, held concretely for Sparq-only capabilities
    /// (introspection, and later reasoning/SHACL/vectors/etc.) that have no
    /// generic `DocumentStore` equivalent and so can't go through `store`.
    /// Points at the exact same underlying store as `store` -- just a second,
    /// differently-typed handle to it, not a separate instance.
    pub sparq: Arc<SparqStore>,
    /// Document converter for processing uploaded files.
    pub converter: DocumentConverter,
    /// Map from document ID to its record.
    pub documents: Arc<RwLock<HashMap<String, DocumentRecord>>>,
    /// The configured embedding provider for `embed_document`/
    /// `semantic_search`, if one is configured (see
    /// `RUDDYDOC_EMBEDDING_URL`). `None` when unset -- embeddings are opt-in,
    /// since configuring one means the server will make outbound HTTP calls.
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// The configured LLM provider for `ask_document` (natural-language
    /// querying), if one is configured (see `RUDDYDOC_LLM_URL`). `None`
    /// when unset, same opt-in reasoning as `embedding_provider`.
    pub llm_provider: Option<Arc<dyn LlmProvider>>,
}

impl ServerState {
    /// Create a new server state with a fresh in-memory store.
    pub fn new() -> ruddydoc_core::Result<Self> {
        let sparq = Arc::new(SparqStore::new()?);
        let store: Arc<dyn DocumentStore> = sparq.clone();
        let converter = DocumentConverter::default_converter();
        let embedding_provider = Self::configured_embedding_provider();
        let llm_provider = Self::configured_llm_provider();
        Ok(Self {
            store,
            sparq,
            converter,
            embedding_provider,
            llm_provider,
            documents: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Builds an embedding provider from `RUDDYDOC_EMBEDDING_*` environment
    /// variables, or `None` if `RUDDYDOC_EMBEDDING_URL` isn't set. Opt-in by
    /// design: an embedding provider makes outbound HTTP calls, so it
    /// shouldn't silently default to `localhost:8000` the way `ApiVlmModel`'s
    /// options do -- an unset URL means "not configured," not "use the
    /// default local endpoint."
    fn configured_embedding_provider() -> Option<Arc<dyn EmbeddingProvider>> {
        let url = std::env::var("RUDDYDOC_EMBEDDING_URL").ok()?;
        let options = ApiEmbeddingOptions {
            url,
            api_key: std::env::var("RUDDYDOC_EMBEDDING_API_KEY").ok(),
            model_name: std::env::var("RUDDYDOC_EMBEDDING_MODEL")
                .unwrap_or_else(|_| ApiEmbeddingOptions::default().model_name),
            ..ApiEmbeddingOptions::default()
        };
        match ApiEmbeddingModel::new(options) {
            Ok(model) => Some(Arc::new(model)),
            Err(e) => {
                debug!(error = %e, "failed to build configured embedding provider");
                None
            }
        }
    }

    /// Builds an LLM provider from `RUDDYDOC_LLM_*` environment variables,
    /// or `None` if `RUDDYDOC_LLM_URL` isn't set. Same opt-in reasoning as
    /// `configured_embedding_provider`.
    fn configured_llm_provider() -> Option<Arc<dyn LlmProvider>> {
        let url = std::env::var("RUDDYDOC_LLM_URL").ok()?;
        let options = ApiLlmOptions {
            url,
            api_key: std::env::var("RUDDYDOC_LLM_API_KEY").ok(),
            model_name: std::env::var("RUDDYDOC_LLM_MODEL")
                .unwrap_or_else(|_| ApiLlmOptions::default().model_name),
            ..ApiLlmOptions::default()
        };
        match ApiLlmModel::new(options) {
            Ok(model) => Some(Arc::new(model)),
            Err(e) => {
                debug!(error = %e, "failed to build configured LLM provider");
                None
            }
        }
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
        // inside the blocking task. The conversion result contains its own
        // store (Arc<dyn DocumentStore>); we need to copy the triples into
        // our shared store.
        let store = Arc::clone(&self.store);
        let sparq = Arc::clone(&self.sparq);
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

            // Also load ontology + SHACL shapes into the shared store if not
            // already done.
            let ont_count = store.triple_count_in(ruddydoc_ontology::ONTOLOGY_GRAPH)?;
            if ont_count == 0 {
                ruddydoc_ontology::load_ontology(store.as_ref())?;
            }
            let shapes_count = store.triple_count_in(ruddydoc_ontology::SHAPES_GRAPH)?;
            if shapes_count == 0 {
                ruddydoc_ontology::load_shapes(&sparq)?;
            }

            Ok::<_, ruddydoc_core::Error>((
                conversion.input,
                conversion.doc_graph,
                conversion.validation,
            ))
        })
        .await
        .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })??;

        let (meta, doc_graph, validation) = result;
        let id = uuid::Uuid::new_v4().to_string();

        let record = DocumentRecord {
            id: id.clone(),
            graph_iri: doc_graph,
            meta,
            validation,
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

    /// Resolve a document ID to its named graph IRI, or an error if unknown.
    async fn doc_graph(&self, id: &str) -> ruddydoc_core::Result<String> {
        let docs = self.documents.read().await;
        docs.get(id)
            .map(|record| record.graph_iri.clone())
            .ok_or_else(|| format!("document '{id}' not found").into())
    }

    /// Full schema introspection for a document (triple/entity counts,
    /// classes, predicates, characteristic sets, join hints, vocabularies).
    pub async fn introspect_document(&self, id: &str) -> ruddydoc_core::Result<serde_json::Value> {
        let doc_graph = self.doc_graph(id).await?;
        let sparq = Arc::clone(&self.sparq);
        tokio::task::spawn_blocking(move || sparq.introspect_json(Some(&doc_graph)))
            .await
            .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Classes observed in a document, by descending instance count.
    pub async fn list_classes(&self, id: &str) -> ruddydoc_core::Result<serde_json::Value> {
        let doc_graph = self.doc_graph(id).await?;
        let sparq = Arc::clone(&self.sparq);
        tokio::task::spawn_blocking(move || sparq.classes_json(&doc_graph))
            .await
            .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Namespaces/prefixes in use in a document, by descending term count.
    pub async fn list_prefixes(&self, id: &str) -> ruddydoc_core::Result<serde_json::Value> {
        let doc_graph = self.doc_graph(id).await?;
        let sparq = Arc::clone(&self.sparq);
        tokio::task::spawn_blocking(move || sparq.prefixes_json(&doc_graph))
            .await
            .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Full-text (BM25) search over a document's string literals.
    pub async fn search_text(
        &self,
        id: &str,
        query: &str,
        limit: usize,
    ) -> ruddydoc_core::Result<serde_json::Value> {
        let doc_graph = self.doc_graph(id).await?;
        let sparq = Arc::clone(&self.sparq);
        let query = query.to_string();
        tokio::task::spawn_blocking(move || sparq.search_text(&doc_graph, &query, limit))
            .await
            .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Re-validates a document against the ontology's SHACL shapes on
    /// demand -- unlike `DocumentRecord.validation` (a snapshot from initial
    /// conversion), this reflects the document graph's current state.
    pub async fn validate_document(&self, id: &str) -> ruddydoc_core::Result<serde_json::Value> {
        let doc_graph = self.doc_graph(id).await?;
        let sparq = Arc::clone(&self.sparq);
        tokio::task::spawn_blocking(move || {
            sparq.validate_shacl(ruddydoc_ontology::SHAPES_GRAPH, &doc_graph)
        })
        .await
        .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Computes an RDFC-1.0 canonical-graph hash (hex SHA-256) for a
    /// document's current graph state. The server doesn't compute this at
    /// conversion time (it's an opt-in, CLI-only `ConvertOptions` flag), so
    /// this is the only way to obtain it via the MCP/server surface.
    pub async fn canonicalize_document(&self, id: &str) -> ruddydoc_core::Result<String> {
        let doc_graph = self.doc_graph(id).await?;
        let sparq = Arc::clone(&self.sparq);
        tokio::task::spawn_blocking(move || sparq.canonical_hash(&doc_graph))
            .await
            .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Chunks a document, embeds each chunk via the configured embedding
    /// provider, inserts each chunk as a real `rdoc:Chunk` node in the
    /// document graph (`rdoc:chunkText`/`rdoc:chunkIndex`), and indexes the
    /// resulting vectors for `semantic_search`. Returns the number of chunks
    /// embedded. Errors if no provider is configured
    /// (`RUDDYDOC_EMBEDDING_URL` unset).
    pub async fn embed_document(&self, id: &str, max_tokens: usize) -> ruddydoc_core::Result<usize> {
        let provider = self.embedding_provider.clone().ok_or_else(|| -> ruddydoc_core::Error {
            "no embedding provider configured; set RUDDYDOC_EMBEDDING_URL".into()
        })?;
        let doc_graph = self.doc_graph(id).await?;
        let store = Arc::clone(&self.store);
        let sparq = Arc::clone(&self.sparq);

        tokio::task::spawn_blocking(move || {
            let ont = ruddydoc_ontology::NAMESPACE;
            let rdf_type = ruddydoc_ontology::rdf_iri("type");
            let chunk_class = ruddydoc_ontology::iri(ruddydoc_ontology::CLASS_CHUNK);

            let options = ChunkOptions {
                max_tokens,
                ..Default::default()
            };
            let chunks = chunk_document(store.as_ref(), &doc_graph, &options)?;

            let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
            let vectors = provider.embed(&texts)?;
            if vectors.len() != chunks.len() {
                return Err(format!(
                    "embedding provider returned {} vectors for {} chunks",
                    vectors.len(),
                    chunks.len()
                )
                .into());
            }

            let mut entries = Vec::with_capacity(chunks.len());
            for (i, (chunk, vector)) in chunks.into_iter().zip(vectors).enumerate() {
                let chunk_iri = format!("{doc_graph}/chunk-{i}");
                store.insert_triple_into(&chunk_iri, &rdf_type, &chunk_class, &doc_graph)?;
                store.insert_literal(
                    &chunk_iri,
                    &format!("{ont}chunkText"),
                    &chunk.text,
                    "string",
                    &doc_graph,
                )?;
                store.insert_literal(
                    &chunk_iri,
                    &format!("{ont}chunkIndex"),
                    &i.to_string(),
                    "integer",
                    &doc_graph,
                )?;
                entries.push((chunk_iri, vector));
            }

            let count = entries.len();
            sparq.index_embeddings(&doc_graph, entries)?;
            Ok::<_, ruddydoc_core::Error>(count)
        })
        .await
        .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Embeds `query` via the configured embedding provider and returns the
    /// top-`k` most similar previously-embedded chunks (see
    /// [`embed_document`](Self::embed_document)), each with its text and
    /// similarity score. Errors if no provider is configured, or if the
    /// document hasn't been embedded yet (empty results, not an error, if
    /// `embed_document` ran but found no chunks).
    pub async fn semantic_search(
        &self,
        id: &str,
        query: &str,
        k: usize,
    ) -> ruddydoc_core::Result<serde_json::Value> {
        let provider = self.embedding_provider.clone().ok_or_else(|| -> ruddydoc_core::Error {
            "no embedding provider configured; set RUDDYDOC_EMBEDDING_URL".into()
        })?;
        let doc_graph = self.doc_graph(id).await?;
        let store = Arc::clone(&self.store);
        let sparq = Arc::clone(&self.sparq);
        let query = query.to_string();

        tokio::task::spawn_blocking(move || {
            let mut vectors = provider.embed(std::slice::from_ref(&query))?;
            let query_vector = vectors.pop().ok_or_else(|| -> ruddydoc_core::Error {
                "embedding provider returned no vector for the query".into()
            })?;

            let matches = sparq.search_similar(&doc_graph, &query_vector, k)?;
            let ont = ruddydoc_ontology::NAMESPACE;
            let results: Vec<serde_json::Value> = matches
                .into_iter()
                .map(|(chunk_iri, score)| {
                    let sparql = format!(
                        "SELECT ?text WHERE {{ GRAPH <{doc_graph}> {{ <{chunk_iri}> <{ont}chunkText> ?text }} }}"
                    );
                    let text = store
                        .query_to_json(&sparql)
                        .ok()
                        .and_then(|rows| rows.as_array().and_then(|a| a.first().cloned()))
                        .and_then(|row| row.get("text").cloned())
                        .unwrap_or(serde_json::Value::Null);
                    serde_json::json!({ "chunk": chunk_iri, "score": score, "text": text })
                })
                .collect();
            Ok::<_, ruddydoc_core::Error>(serde_json::Value::Array(results))
        })
        .await
        .map_err(|e| -> ruddydoc_core::Error { format!("task join error: {e}").into() })?
    }

    /// Answers a natural-language `question` about a document without the
    /// caller writing SPARQL -- grounds via the document's schema, generates
    /// a query with the configured LLM, validates/repairs it, and executes
    /// it. Errors if no LLM provider is configured
    /// (`RUDDYDOC_LLM_URL` unset).
    pub async fn ask_document(&self, id: &str, question: &str) -> ruddydoc_core::Result<serde_json::Value> {
        let provider = self.llm_provider.clone().ok_or_else(|| -> ruddydoc_core::Error {
            "no LLM provider configured; set RUDDYDOC_LLM_URL".into()
        })?;
        let doc_graph = self.doc_graph(id).await?;
        let sparq = Arc::clone(&self.sparq);
        let question = question.to_string();

        tokio::task::spawn_blocking(move || {
            sparq.ask_natural_language(&doc_graph, &question, move |prompt| {
                provider.complete(prompt).map_err(|e| e.to_string())
            })
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
    const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";

    if let Some(idx) = s.find("\"^^<") {
        let value = &s[1..idx];
        // Keep the full datatype IRI rather than just its local name (e.g.
        // "dateTime") -- `SparqStore::insert_literal` only special-cases a
        // handful of short keywords ("string", "integer", ...) and falls
        // back to parsing whatever it's given as an absolute IRI, so a bare
        // local name (not a valid IRI on its own) would fail for any
        // datatype outside that short list (found via `xsd:dateTime`
        // literals produced by PROV-O lineage triples).
        let dt_iri = &s[idx + 4..s.len() - 1]; // strip ^^< and >
        (value.to_string(), dt_iri.to_string())
    } else if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        (s[1..s.len() - 1].to_string(), XSD_STRING.to_string())
    } else {
        (s.to_string(), XSD_STRING.to_string())
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
        assert_eq!(dt, "http://www.w3.org/2001/XMLSchema#integer");
    }

    #[test]
    fn parse_literal_typed_datetime() {
        // Regression test: a datatype outside `insert_literal`'s short
        // keyword list (previously truncated to the bare local name
        // "dateTime", which isn't a valid absolute IRI on its own and broke
        // `convert_file`'s triple-copy loop for PROV-O lineage literals).
        let (val, dt) = parse_literal(
            "\"2026-01-01T00:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>",
        );
        assert_eq!(val, "2026-01-01T00:00:00Z");
        assert_eq!(dt, "http://www.w3.org/2001/XMLSchema#dateTime");
    }

    #[test]
    fn parse_literal_plain() {
        let (val, dt) = parse_literal("\"hello world\"");
        assert_eq!(val, "hello world");
        assert_eq!(dt, "http://www.w3.org/2001/XMLSchema#string");
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
