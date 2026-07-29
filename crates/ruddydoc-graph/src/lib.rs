//! Sparq-based document store for RuddyDoc.
//!
//! This crate wraps `sparq_core::Graph` / `sparq_engine` and implements the
//! `DocumentStore` trait from `ruddydoc-core`. No other crate in the
//! workspace should depend on Sparq directly.

use std::collections::HashMap;
use std::sync::Mutex;

use oxrdf::vocab::xsd;
use oxrdf::{Literal, NamedNode, NamedNodeRef, NamedOrBlankNode, Term, Triple};
use serde_json::{Map, Value};
use sparq_core::Graph;
use sparq_core::dict::Dict;

use ruddydoc_core::{DocumentStore, Error};

/// Cosine similarity between two equal-length vectors, in `[-1.0, 1.0]`
/// (`0.0` if either is the zero vector).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Percent-encode characters that are invalid in IRIs.
///
/// Adapted from beret's `iri_escape()` function. Preserves characters
/// that are legal in IRI references and percent-encodes everything else.
fn iri_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || "-._~:@!$&'()*+,;=/?#".contains(c) {
            out.push(c);
        } else {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// Sparq-backed document store.
///
/// Each parsed document is stored in its own named graph, enabling
/// per-document queries and multi-document SPARQL queries. Unlike Oxigraph's
/// `Store`, Sparq's `Graph` requires `&mut self` for every mutation and has
/// no interior mutability of its own, so it's wrapped in a `Mutex` here to
/// satisfy `DocumentStore`'s `&self`-based methods.
pub struct SparqStore {
    graph: Mutex<Graph>,
    /// Named graph -> (id, vector) entries for that graph's embedded chunks.
    /// A plain in-memory brute-force store, not `sparq-vectors` -- that
    /// crate's `VectorStore` is file-backed (writes/mmaps a `.spqv` file),
    /// which doesn't fit RuddyDoc's fully in-memory architecture. At
    /// RuddyDoc's actual scale (tens of chunks per document, not millions),
    /// exact brute-force cosine search is fast enough and far simpler than
    /// managing a temp-file-backed ANN index's lifecycle.
    embeddings: Mutex<HashMap<String, Vec<(String, Vec<f32>)>>>,
}

impl SparqStore {
    /// Create a new in-memory document store.
    pub fn new() -> std::result::Result<Self, Error> {
        Ok(Self {
            graph: Mutex::new(Graph::new()),
            embeddings: Mutex::new(HashMap::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Graph> {
        self.graph.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Collects every triple in `graph`'s own store (not its named
    /// sub-graphs) as owned `oxrdf` terms -- shared prep for N-Triples and
    /// RDF/XML export, which both need a flat triple list rather than
    /// Sparq's dictionary-encoded internal representation.
    fn collect_triples(graph: &Graph) -> Vec<Triple> {
        let scan = graph.store.scan(&[None, None, None]);
        scan.rows
            .iter()
            .map(|row| {
                let spo = scan.to_spo(row);
                let subject = match graph.dict.term(spo[0]) {
                    Term::NamedNode(n) => NamedOrBlankNode::NamedNode(n),
                    Term::BlankNode(b) => NamedOrBlankNode::BlankNode(b),
                    other => unreachable!("non-IRI/blank subject in store: {other}"),
                };
                let predicate = match graph.dict.term(spo[1]) {
                    Term::NamedNode(n) => n,
                    other => unreachable!("non-IRI predicate in store: {other}"),
                };
                let object = graph.dict.term(spo[2]);
                Triple {
                    subject,
                    predicate,
                    object,
                }
            })
            .collect()
    }

    /// Serialize a named graph using Sparq's writers (Turtle, N-Triples) or
    /// `oxrdfxml` directly (RDF/XML -- Sparq has no RDF/XML writer). A named
    /// graph that was never created serializes to an empty document, same as
    /// the previous Oxigraph-backed behavior.
    fn serialize_graph_internal(&self, graph: &str, format: &str) -> ruddydoc_core::Result<String> {
        let g = iri_escape(graph);
        let g_node = Term::NamedNode(NamedNode::new(&g)?);

        let guard = self.lock();
        let empty = Graph::new();
        let sub = guard.named_graph(&g_node).unwrap_or(&empty);

        match format {
            "turtle" | "ttl" => Ok(sparq_engine::serialize::graph_to_turtle(sub)),
            "ntriples" | "nt" => {
                let triples = Self::collect_triples(sub);
                Ok(sparq_engine::triples_to_ntriples(&triples))
            }
            "rdfxml" | "rdf" => {
                let triples = Self::collect_triples(sub);
                let mut serializer = oxrdfxml::RdfXmlSerializer::new().for_writer(Vec::new());
                for t in &triples {
                    serializer.serialize_triple(t.as_ref())?;
                }
                let bytes = serializer.finish()?;
                Ok(String::from_utf8(bytes)?)
            }
            _ => Err(format!("unsupported serialization format: {format}").into()),
        }
    }

    /// Runs `f` against the named graph `graph`, or an empty graph if it was
    /// never created -- the shared scoping helper behind the introspection
    /// methods below (same "missing named graph reads as empty" convention
    /// as `serialize_graph_internal`).
    fn with_named_graph<R>(
        &self,
        graph: &str,
        f: impl FnOnce(&Graph) -> R,
    ) -> ruddydoc_core::Result<R> {
        let g = iri_escape(graph);
        let g_node = Term::NamedNode(NamedNode::new(&g)?);

        let guard = self.lock();
        let empty = Graph::new();
        let sub = guard.named_graph(&g_node).unwrap_or(&empty);
        Ok(f(sub))
    }

    /// Full schema introspection (triple/entity counts, classes, predicates,
    /// characteristic sets, join hints, vocabularies) for a named graph, or
    /// the whole store's default graph if `graph` is `None`. Mined purely
    /// from Sparq's existing indexes -- no SPARQL, no extra state.
    pub fn introspect_json(&self, graph: Option<&str>) -> ruddydoc_core::Result<Value> {
        let introspection = match graph {
            Some(g) => self.with_named_graph(g, |sub| sparq_introspect::Introspection::build(sub))?,
            None => sparq_introspect::Introspection::build(&self.lock()),
        };
        Ok(serde_json::to_value(introspection)?)
    }

    /// Classes observed in a named graph, by descending instance count.
    pub fn classes_json(&self, graph: &str) -> ruddydoc_core::Result<Value> {
        let introspection =
            self.with_named_graph(graph, |sub| sparq_introspect::Introspection::build(sub))?;
        Ok(serde_json::to_value(introspection.classes)?)
    }

    /// Namespaces/prefixes in use in a named graph, by descending term count.
    pub fn prefixes_json(&self, graph: &str) -> ruddydoc_core::Result<Value> {
        let introspection =
            self.with_named_graph(graph, |sub| sparq_introspect::Introspection::build(sub))?;
        Ok(serde_json::to_value(introspection.vocabularies.namespaces)?)
    }

    /// Materializes the RDFS closure (`rdfs:subClassOf`/`subPropertyOf`/
    /// `domain`/`range` entailment) over the union of `ontology_graph`'s
    /// schema and `doc_graph`'s asserted triples, writing only the
    /// newly-derived triples back into `doc_graph` -- never into the
    /// ontology graph -- so each document graph stays self-contained and
    /// queryable without needing the ontology graph joined in. Returns the
    /// number of newly added triples. Idempotent: materializing twice adds
    /// nothing the second time.
    ///
    /// Each named `Graph` has its own separate dictionary/id-space, so the
    /// two graphs' triples can't be mixed by `Id` directly -- both are
    /// collected as portable `oxrdf` terms first, then re-interned together
    /// into one fresh `Dict` for `sparq_reason::materialize` to reason over.
    pub fn materialize_rdfs(
        &self,
        ontology_graph: &str,
        doc_graph: &str,
    ) -> ruddydoc_core::Result<usize> {
        let onto_triples = self.with_named_graph(ontology_graph, Self::collect_triples)?;
        let doc_triples = self.with_named_graph(doc_graph, Self::collect_triples)?;

        let mut dict = Dict::new();
        let mut ids: Vec<[sparq_core::dict::Id; 3]> =
            Vec::with_capacity(onto_triples.len() + doc_triples.len());
        for t in onto_triples.iter().chain(doc_triples.iter()) {
            let subject = match &t.subject {
                NamedOrBlankNode::NamedNode(n) => Term::NamedNode(n.clone()),
                NamedOrBlankNode::BlankNode(b) => Term::BlankNode(b.clone()),
            };
            ids.push([
                dict.intern(&subject),
                dict.intern(&Term::NamedNode(t.predicate.clone())),
                dict.intern(&t.object),
            ]);
        }

        let added = sparq_reason::materialize(sparq_reason::Profile::Rdfs, &mut dict, &mut ids);
        let new_triples = &ids[ids.len() - added..];

        let g = iri_escape(doc_graph);
        let doc_node = Term::NamedNode(NamedNode::new(&g)?);
        let mut guard = self.lock();
        let idx = guard.ensure_named(&doc_node)?;
        let sub = &mut guard.named[idx].1;
        let mut inserted = 0;
        for &[s, p, o] in new_triples {
            // rdfs3 (rdfs:range) can legitimately entail `"value" rdf:type
            // xsd:string`-shaped triples when a property's range is a
            // datatype -- a literal as the *subject* of a triple. Valid
            // RDFS, but useless noise for RuddyDoc's element-typing use case
            // (nobody queries "which literals have type xsd:string") and
            // breaks callers that assume every triple subject is an IRI or
            // blank node (a normally-safe assumption for asserted data).
            // Skip these rather than inserting them.
            let subject = match dict.term(s) {
                Term::Literal(_) => continue,
                other => other,
            };
            let predicate = match dict.term(p) {
                Term::NamedNode(n) => n,
                other => {
                    return Err(format!(
                        "materialized triple has a non-IRI predicate: {other}"
                    )
                    .into());
                }
            };
            sub.insert_triple(subject, predicate, dict.term(o))?;
            inserted += 1;
        }

        Ok(inserted)
    }

    /// Parses `turtle` (SHACL shapes, or any Turtle document) and merges its
    /// triples into a named graph -- RuddyDoc's first use of Sparq's Turtle
    /// *parsing* (only serialization was used before this). Re-running with
    /// the same `turtle` is idempotent: `Graph::insert_triple` is set-valued,
    /// so re-inserting an already-present triple is a no-op.
    pub fn load_shapes_turtle(&self, shapes_graph: &str, turtle: &str) -> ruddydoc_core::Result<()> {
        let parsed = Graph::load_str(turtle, "turtle")?;
        let triples = Self::collect_triples(&parsed);

        let g = iri_escape(shapes_graph);
        let node = Term::NamedNode(NamedNode::new(&g)?);
        let mut guard = self.lock();
        let idx = guard.ensure_named(&node)?;
        let sub = &mut guard.named[idx].1;
        for t in &triples {
            let subject = match &t.subject {
                NamedOrBlankNode::NamedNode(n) => Term::NamedNode(n.clone()),
                NamedOrBlankNode::BlankNode(b) => Term::BlankNode(b.clone()),
            };
            sub.insert_triple(subject, t.predicate.clone(), t.object.clone())?;
        }
        Ok(())
    }

    /// Validates `data_graph` against the SHACL shapes in `shapes_graph`,
    /// returning a deterministic JSON validation report (`conforms` +
    /// `results`). Never returns an `Err` for a non-conforming document --
    /// SHACL violations are a property of the document's content, reported
    /// in the result, not a store-level failure.
    pub fn validate_shacl(&self, shapes_graph: &str, data_graph: &str) -> ruddydoc_core::Result<Value> {
        let shapes_iri = iri_escape(shapes_graph);
        let shapes_node = Term::NamedNode(NamedNode::new(&shapes_iri)?);
        let data_iri = iri_escape(data_graph);
        let data_node = Term::NamedNode(NamedNode::new(&data_iri)?);

        let guard = self.lock();
        let empty = Graph::new();
        let shapes = guard.named_graph(&shapes_node).unwrap_or(&empty);
        let data = guard.named_graph(&data_node).unwrap_or(&empty);

        let report = sparq_shacl::validate(data, shapes);
        Ok(serde_json::from_str(&report.to_json())?)
    }

    /// BM25 full-text search over a named graph's string/language-tagged
    /// literals, returning the matching text, its relevance score, and every
    /// (subject, predicate) pair that literal is the object of. The index is
    /// built fresh per call -- consistent with `introspect_json`/`classes_json`
    /// above, and cheap enough at RuddyDoc's per-document graph sizes that a
    /// persistent cached index isn't worth the invalidation complexity yet.
    pub fn search_text(&self, graph: &str, query: &str, limit: usize) -> ruddydoc_core::Result<Value> {
        self.with_named_graph(graph, |sub| {
            let index = sparq_text::TextIndex::build(sub);
            let hits: Vec<Value> = index
                .search(query)
                .into_iter()
                .take(limit)
                .map(|hit| {
                    let text = sub.dict.term(hit.id);
                    let scan = sub.store.scan(&[None, None, Some(hit.id)]);
                    let references: Vec<Value> = scan
                        .rows
                        .iter()
                        .map(|row| {
                            let spo = scan.to_spo(row);
                            serde_json::json!({
                                "subject": sub.dict.term(spo[0]).to_string(),
                                "predicate": sub.dict.term(spo[1]).to_string(),
                            })
                        })
                        .collect();
                    serde_json::json!({
                        "text": text.to_string(),
                        "score": hit.score,
                        "references": references,
                    })
                })
                .collect();
            Value::Array(hits)
        })
    }

    /// Replaces the embedding entries for `graph` (e.g. a document's chunks)
    /// with `entries` (id, vector) pairs -- typically a chunk's node IRI and
    /// its embedding. Re-indexing a graph discards its previous entries.
    pub fn index_embeddings(
        &self,
        graph: &str,
        entries: Vec<(String, Vec<f32>)>,
    ) -> ruddydoc_core::Result<()> {
        let mut embeddings = self.embeddings.lock().unwrap_or_else(|e| e.into_inner());
        embeddings.insert(graph.to_string(), entries);
        Ok(())
    }

    /// Top-`k` nearest entries to `query_vector` in `graph` by cosine
    /// similarity, best-first. Empty (not an error) if `graph` was never
    /// indexed. Brute-force -- see the `embeddings` field's doc comment for
    /// why this is exact rather than approximate.
    pub fn search_similar(
        &self,
        graph: &str,
        query_vector: &[f32],
        k: usize,
    ) -> ruddydoc_core::Result<Vec<(String, f32)>> {
        let embeddings = self.embeddings.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entries) = embeddings.get(graph) else {
            return Ok(Vec::new());
        };

        let mut scored: Vec<(String, f32)> = entries
            .iter()
            .map(|(id, vector)| (id.clone(), cosine_similarity(query_vector, vector)))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(k);
        Ok(scored)
    }

    /// Answers a natural-language `question` against a named graph via
    /// `sparq-nlq`'s GROUND (schema summary) -> GENERATE -> VALIDATE ->
    /// REPAIR -> EXECUTE loop, so a caller can query without writing SPARQL.
    ///
    /// `complete` is a plain completion function (prompt in, response text
    /// out) rather than a trait object from another crate -- this keeps
    /// Sparq's `Llm` trait fully encapsulated here; callers (e.g.
    /// `ruddydoc-models`' `LlmProvider`) never need to depend on Sparq
    /// directly, they just pass a closure.
    pub fn ask_natural_language(
        &self,
        graph: &str,
        question: &str,
        complete: impl Fn(&str) -> Result<String, String> + 'static,
    ) -> ruddydoc_core::Result<Value> {
        struct ClosureLlm<F>(F);
        impl<F: Fn(&str) -> Result<String, String>> sparq_nlq::Llm for ClosureLlm<F> {
            fn complete(&self, prompt: &str) -> Result<String, String> {
                (self.0)(prompt)
            }
        }

        let outcome = self.with_named_graph(graph, |sub| {
            let nlq = sparq_nlq::Nlq::new(sub, Box::new(ClosureLlm(complete)));
            let answer = nlq.ask(question).map_err(|e| e.to_string())?;

            let variables: Vec<String> =
                answer.result.vars.iter().map(|v| v.as_str().to_owned()).collect();
            let mut rows = Vec::with_capacity(answer.result.rows.len());
            for row in &answer.result.rows {
                let mut obj = Map::new();
                for (var, cell) in variables.iter().zip(row.iter()) {
                    let value = cell
                        .as_ref()
                        .map_or(Value::Null, |term| Value::String(term.to_string()));
                    obj.insert(var.clone(), value);
                }
                rows.push(Value::Object(obj));
            }

            Ok::<_, String>(serde_json::json!({
                "sparql": answer.sparql,
                "result": Value::Array(rows),
                "repairs": answer.repairs,
            }))
        })?;
        outcome.map_err(Into::into)
    }

    /// Canonicalizes a named graph per W3C RDF Dataset Canonicalization
    /// (RDFC-1.0) and returns the SHA-256 hex digest of the canonical
    /// N-Quads output. Two graphs that are RDF-isomorphic (same triples,
    /// regardless of blank-node labels or insertion order) hash identically
    /// -- unlike `compute_hash` elsewhere in RuddyDoc, which hashes the raw
    /// *source file bytes*, this hashes the *derived RDF graph* itself, so
    /// e.g. two different source formats that parse to the same graph
    /// content would produce the same canonical hash.
    pub fn canonical_hash(&self, graph: &str) -> ruddydoc_core::Result<String> {
        use sha2::{Digest, Sha256};

        let nquads = self
            .with_named_graph(graph, |sub| {
                let canonical = sparq_canon::canonicalize_graph_content(sub)
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>(canonical.to_nquads())
            })?
            .map_err(Error::from)?;

        let mut hasher = Sha256::new();
        hasher.update(nquads.as_bytes());
        let digest = hasher.finalize();
        Ok(digest.iter().fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        }))
    }

    /// Records a W3C PROV-O lineage record for a conversion: one
    /// `prov:Activity` (this operation), one `prov:Entity` (the document
    /// graph itself, reusing its own IRI), linked via `wasGeneratedBy` to the
    /// activity and `wasDerivedFrom`/`used` to `source_iri` (the conversion's
    /// input). This is a *supplementary* standards-compliant layer alongside
    /// RuddyDoc's existing bespoke `rdoc:Provenance` model -- it doesn't
    /// replace it.
    ///
    /// Implemented via `sparq_prov::derive_construct`, the crate's real API
    /// for capturing lineage of data derived by a CONSTRUCT query: querying
    /// `CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }` against the document graph
    /// "re-derives" its own current triples, which is exactly the shape
    /// `sparq-prov` expects (there's no bare "just record an activity"
    /// entry point -- lineage is always attached to an actual derivation).
    /// The resulting PROV-O triples are inserted into the document graph
    /// alongside the document's own data.
    pub fn record_conversion_provenance(
        &self,
        doc_graph: &str,
        source_iri: &str,
    ) -> ruddydoc_core::Result<()> {
        let g = iri_escape(doc_graph);
        let doc_node_iri = NamedNode::new(&g)?;
        let source_node = NamedNode::new(iri_escape(source_iri))?;

        let mut config = sparq_prov::ProvConfig::with_inputs([source_node]);
        config.entity = Some(doc_node_iri.clone());

        let derivation = self
            .with_named_graph(doc_graph, |sub| {
                sparq_prov::derive_construct(
                    sub,
                    "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }",
                    config,
                )
            })?
            .map_err(|e| -> ruddydoc_core::Error { e.into() })?;

        let mut guard = self.lock();
        let idx = guard.ensure_named(&Term::NamedNode(doc_node_iri))?;
        let sub = &mut guard.named[idx].1;
        for t in derivation.prov_graph() {
            sub.insert_triple(t.subject, t.predicate, t.object)?;
        }
        Ok(())
    }
}

impl DocumentStore for SparqStore {
    fn insert_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
    ) -> ruddydoc_core::Result<()> {
        let s = NamedNode::new(iri_escape(subject))?;
        let p = NamedNode::new(iri_escape(predicate))?;
        let o = NamedNode::new(iri_escape(object))?;

        self.lock().insert_triple(s, p, o)?;
        Ok(())
    }

    fn insert_triple_into(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        graph: &str,
    ) -> ruddydoc_core::Result<()> {
        let s = NamedNode::new(iri_escape(subject))?;
        let p = NamedNode::new(iri_escape(predicate))?;
        let o = NamedNode::new(iri_escape(object))?;
        let g = Term::NamedNode(NamedNode::new(iri_escape(graph))?);

        let mut guard = self.lock();
        let idx = guard.ensure_named(&g)?;
        guard.named[idx].1.insert_triple(s, p, o)?;
        Ok(())
    }

    fn insert_literal(
        &self,
        subject: &str,
        predicate: &str,
        value: &str,
        datatype: &str,
        graph: &str,
    ) -> ruddydoc_core::Result<()> {
        let s = NamedNode::new(iri_escape(subject))?;
        let p = NamedNode::new(iri_escape(predicate))?;
        let g = Term::NamedNode(NamedNode::new(iri_escape(graph))?);

        let dt = match datatype {
            "string" => xsd::STRING,
            "integer" => xsd::INTEGER,
            "float" => xsd::FLOAT,
            "double" => xsd::DOUBLE,
            "boolean" => xsd::BOOLEAN,
            "base64Binary" => xsd::BASE_64_BINARY,
            _ => NamedNodeRef::new(datatype)?,
        };
        let literal = Literal::new_typed_literal(value, dt);

        let mut guard = self.lock();
        let idx = guard.ensure_named(&g)?;
        guard.named[idx].1.insert_triple(s, p, literal)?;
        Ok(())
    }

    fn query_to_json(&self, sparql: &str) -> ruddydoc_core::Result<Value> {
        let guard = self.lock();

        // Sparq's `query()` handles SELECT and ASK uniformly, folding ASK
        // into a zero-variable `QueryResult` (no rows = false, one empty row
        // = true) rather than a discriminated result type like Oxigraph's
        // `QueryResults`. `ask()` fails both for "not an ASK query" and for
        // a genuine parse error, so try it first and fall back to `query()`
        // -- which uses the identical parser, so a real syntax error surfaces
        // through it with the same message either way.
        if let Ok(is_true) = sparq_engine::ask(&guard, sparql) {
            return Ok(Value::Bool(is_true));
        }

        let result = sparq_engine::query(&guard, sparql)?;
        let variables: Vec<String> = result.vars.iter().map(|v| v.as_str().to_owned()).collect();

        let mut rows = Vec::with_capacity(result.rows.len());
        for row in &result.rows {
            let mut obj = Map::new();
            for (var, cell) in variables.iter().zip(row.iter()) {
                let value = cell
                    .as_ref()
                    .map_or(Value::Null, |term| Value::String(term.to_string()));
                obj.insert(var.clone(), value);
            }
            rows.push(Value::Object(obj));
        }
        Ok(Value::Array(rows))
    }

    fn clear(&self) -> ruddydoc_core::Result<()> {
        *self.lock() = Graph::new();
        Ok(())
    }

    fn clear_graph(&self, graph: &str) -> ruddydoc_core::Result<()> {
        let g = Term::NamedNode(NamedNode::new(iri_escape(graph))?);
        self.lock().drop_named_durable(&g)?;
        Ok(())
    }

    fn serialize_graph(&self, graph: &str, format: &str) -> ruddydoc_core::Result<String> {
        self.serialize_graph_internal(graph, format)
    }

    fn triple_count(&self) -> ruddydoc_core::Result<usize> {
        let guard = self.lock();
        Ok(guard.len() + guard.named.iter().map(|(_, g)| g.len()).sum::<usize>())
    }

    fn triple_count_in(&self, graph: &str) -> ruddydoc_core::Result<usize> {
        let g = Term::NamedNode(NamedNode::new(iri_escape(graph))?);
        let guard = self.lock();
        Ok(guard.named_graph(&g).map_or(0, Graph::len))
    }

    fn insert_language_tagged_literal(
        &self,
        subject: &str,
        predicate: &str,
        value: &str,
        language: &str,
        graph: &str,
    ) -> ruddydoc_core::Result<()> {
        let s = NamedNode::new(iri_escape(subject))?;
        let p = NamedNode::new(iri_escape(predicate))?;
        let g = Term::NamedNode(NamedNode::new(iri_escape(graph))?);

        let literal = Literal::new_language_tagged_literal(value, language)
            .map_err(|e| format!("invalid BCP 47 language tag '{language}': {e}"))?;

        let mut guard = self.lock();
        let idx = guard.ensure_named(&g)?;
        guard.named[idx].1.insert_triple(s, p, literal)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_core::DocumentStore;

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const RDOC_DOCUMENT: &str = "https://ruddydoc.chapeaux.io/ontology#Document";
    const RDOC_PARAGRAPH: &str = "https://ruddydoc.chapeaux.io/ontology#Paragraph";
    const RDOC_TEXT_CONTENT: &str = "https://ruddydoc.chapeaux.io/ontology#textContent";
    const RDOC_HEADING_LEVEL: &str = "https://ruddydoc.chapeaux.io/ontology#headingLevel";
    const RDOC_IS_HEADER: &str = "https://ruddydoc.chapeaux.io/ontology#isHeader";
    const RDOC_READING_ORDER: &str = "https://ruddydoc.chapeaux.io/ontology#readingOrder";

    #[test]
    fn new_store() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        assert_eq!(store.triple_count()?, 0);
        Ok(())
    }

    #[test]
    fn insert_and_query_default_graph() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        store.insert_triple("urn:ruddydoc:doc:test", RDF_TYPE, RDOC_DOCUMENT)?;

        let json = store.query_to_json("SELECT ?s ?p ?o WHERE { ?s ?p ?o }")?;
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let row = &rows[0];
        assert!(
            row["s"]
                .as_str()
                .expect("expected string")
                .contains("urn:ruddydoc:doc:test")
        );
        Ok(())
    }

    #[test]
    fn insert_and_query_named_graph() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:abc123";

        store.insert_triple_into("urn:ruddydoc:doc:abc123", RDF_TYPE, RDOC_DOCUMENT, graph)?;

        // Query within the named graph
        let sparql = format!("SELECT ?s ?p ?o WHERE {{ GRAPH <{graph}> {{ ?s ?p ?o }} }}");
        let json = store.query_to_json(&sparql)?;
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        Ok(())
    }

    #[test]
    fn named_graph_isolation() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph_a = "urn:ruddydoc:doc:aaa";
        let graph_b = "urn:ruddydoc:doc:bbb";

        store.insert_triple_into("urn:ruddydoc:doc:aaa", RDF_TYPE, RDOC_DOCUMENT, graph_a)?;
        store.insert_triple_into("urn:ruddydoc:doc:bbb", RDF_TYPE, RDOC_PARAGRAPH, graph_b)?;

        // Graph A should only have 1 triple
        let sparql_a = format!("SELECT ?s WHERE {{ GRAPH <{graph_a}> {{ ?s ?p ?o }} }}");
        let json_a = store.query_to_json(&sparql_a)?;
        assert_eq!(json_a.as_array().expect("expected array").len(), 1);

        // Graph B should only have 1 triple
        let sparql_b = format!("SELECT ?s WHERE {{ GRAPH <{graph_b}> {{ ?s ?p ?o }} }}");
        let json_b = store.query_to_json(&sparql_b)?;
        assert_eq!(json_b.as_array().expect("expected array").len(), 1);

        // Total should be 2
        assert_eq!(store.triple_count()?, 2);

        // Per-graph counts
        assert_eq!(store.triple_count_in(graph_a)?, 1);
        assert_eq!(store.triple_count_in(graph_b)?, 1);

        Ok(())
    }

    #[test]
    fn insert_string_literal() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:lit";

        store.insert_literal(
            "urn:ruddydoc:doc:lit/p0",
            RDOC_TEXT_CONTENT,
            "Hello, world!",
            "string",
            graph,
        )?;

        let sparql = format!(
            "SELECT ?text WHERE {{ GRAPH <{graph}> {{ ?s <{RDOC_TEXT_CONTENT}> ?text }} }}"
        );
        let json = store.query_to_json(&sparql)?;
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text_val = rows[0]["text"].as_str().expect("expected string");
        assert!(text_val.contains("Hello, world!"));
        Ok(())
    }

    #[test]
    fn insert_integer_literal() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:intlit";

        store.insert_literal(
            "urn:ruddydoc:doc:intlit/h1",
            RDOC_HEADING_LEVEL,
            "2",
            "integer",
            graph,
        )?;

        let sparql = format!(
            "SELECT ?level WHERE {{ GRAPH <{graph}> {{ ?s <{RDOC_HEADING_LEVEL}> ?level }} }}"
        );
        let json = store.query_to_json(&sparql)?;
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let level_str = rows[0]["level"].as_str().expect("expected string");
        assert!(level_str.contains('2'));
        Ok(())
    }

    #[test]
    fn insert_float_literal() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:floatlit";

        store.insert_literal(
            "urn:ruddydoc:doc:floatlit/el",
            "https://ruddydoc.chapeaux.io/ontology#confidence",
            "0.95",
            "float",
            graph,
        )?;

        let sparql = format!(
            "SELECT ?c WHERE {{ GRAPH <{graph}> {{ ?s <https://ruddydoc.chapeaux.io/ontology#confidence> ?c }} }}"
        );
        let json = store.query_to_json(&sparql)?;
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        Ok(())
    }

    #[test]
    fn insert_boolean_literal() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:boollit";

        store.insert_literal(
            "urn:ruddydoc:doc:boollit/cell",
            RDOC_IS_HEADER,
            "true",
            "boolean",
            graph,
        )?;

        let sparql =
            format!("SELECT ?h WHERE {{ GRAPH <{graph}> {{ ?s <{RDOC_IS_HEADER}> ?h }} }}");
        let json = store.query_to_json(&sparql)?;
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let h_str = rows[0]["h"].as_str().expect("expected string");
        assert!(h_str.contains("true"));
        Ok(())
    }

    #[test]
    fn clear_empties_store() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        store.insert_triple(
            "urn:ruddydoc:doc:a",
            "urn:ruddydoc:rel",
            "urn:ruddydoc:doc:b",
        )?;
        assert_eq!(store.triple_count()?, 1);

        store.clear()?;
        assert_eq!(store.triple_count()?, 0);

        let json = store.query_to_json("SELECT ?s ?p ?o WHERE { ?s ?p ?o }")?;
        let rows = json.as_array().expect("expected array");
        assert!(rows.is_empty());
        Ok(())
    }

    #[test]
    fn clear_graph_removes_only_target() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph_a = "urn:ruddydoc:doc:clear_a";
        let graph_b = "urn:ruddydoc:doc:clear_b";

        store.insert_triple_into("urn:ruddydoc:doc:a", RDF_TYPE, RDOC_DOCUMENT, graph_a)?;
        store.insert_triple_into("urn:ruddydoc:doc:b", RDF_TYPE, RDOC_PARAGRAPH, graph_b)?;
        assert_eq!(store.triple_count()?, 2);

        store.clear_graph(graph_a)?;
        assert_eq!(store.triple_count()?, 1);
        assert_eq!(store.triple_count_in(graph_a)?, 0);
        assert_eq!(store.triple_count_in(graph_b)?, 1);

        Ok(())
    }

    #[test]
    fn triple_count_in_named_graph() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:count_test";

        store.insert_triple_into("urn:s1", RDF_TYPE, RDOC_DOCUMENT, graph)?;
        store.insert_triple_into("urn:s2", RDF_TYPE, RDOC_PARAGRAPH, graph)?;
        store.insert_literal("urn:s2", RDOC_TEXT_CONTENT, "hello", "string", graph)?;

        assert_eq!(store.triple_count_in(graph)?, 3);
        assert_eq!(store.triple_count()?, 3);
        Ok(())
    }

    #[test]
    fn serialize_graph_turtle() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:ser_test";

        store.insert_triple_into("urn:ruddydoc:doc:ser_test", RDF_TYPE, RDOC_DOCUMENT, graph)?;
        store.insert_literal(
            "urn:ruddydoc:doc:ser_test",
            RDOC_TEXT_CONTENT,
            "test",
            "string",
            graph,
        )?;

        let turtle = store.serialize_graph(graph, "turtle")?;
        assert!(!turtle.is_empty());
        // Turtle should contain the subject IRI
        assert!(turtle.contains("urn:ruddydoc:doc:ser_test"));
        Ok(())
    }

    #[test]
    fn serialize_graph_ntriples() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:nt_test";

        store.insert_triple_into("urn:ruddydoc:doc:nt_test", RDF_TYPE, RDOC_DOCUMENT, graph)?;

        let nt = store.serialize_graph(graph, "ntriples")?;
        assert!(!nt.is_empty());
        assert!(nt.contains("urn:ruddydoc:doc:nt_test"));
        assert!(nt.contains("<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>"));
        Ok(())
    }

    #[test]
    fn serialize_unsupported_format_errors() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let result = store.serialize_graph("urn:g", "yaml");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn ask_query_returns_boolean() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        store.insert_triple("urn:a", RDF_TYPE, RDOC_DOCUMENT)?;

        let result = store.query_to_json("ASK { ?s ?p ?o }")?;
        assert_eq!(result, Value::Bool(true));

        let result_empty =
            store.query_to_json("ASK { <urn:nonexistent> <urn:nonexistent> <urn:nonexistent> }")?;
        assert_eq!(result_empty, Value::Bool(false));
        Ok(())
    }

    #[test]
    fn insert_language_tagged_literal() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:lang_test";

        store.insert_language_tagged_literal(
            "urn:ruddydoc:doc:lang_test/p0",
            RDOC_TEXT_CONTENT,
            "Bonjour",
            "fr",
            graph,
        )?;

        let sparql = format!(
            "SELECT ?text WHERE {{ GRAPH <{graph}> {{ ?s <{RDOC_TEXT_CONTENT}> ?text }} }}"
        );
        let json = store.query_to_json(&sparql)?;
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text_val = rows[0]["text"].as_str().expect("expected string");
        // Language-tagged literals render as "value"@lang
        assert!(text_val.contains("Bonjour"));
        Ok(())
    }

    #[test]
    fn language_tagged_literal_with_lang_filter() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:lang_filter";

        store.insert_language_tagged_literal(
            "urn:ruddydoc:doc:lang_filter/p0",
            RDOC_TEXT_CONTENT,
            "Hello",
            "en",
            graph,
        )?;
        store.insert_language_tagged_literal(
            "urn:ruddydoc:doc:lang_filter/p1",
            RDOC_TEXT_CONTENT,
            "Bonjour",
            "fr",
            graph,
        )?;

        // Filter to only French literals
        let sparql = format!(
            "SELECT ?text WHERE {{ GRAPH <{graph}> {{ ?s <{RDOC_TEXT_CONTENT}> ?text . FILTER(LANG(?text) = \"fr\") }} }}"
        );
        let json = store.query_to_json(&sparql)?;
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text_val = rows[0]["text"].as_str().expect("expected string");
        assert!(text_val.contains("Bonjour"));
        Ok(())
    }

    #[test]
    fn language_tagged_literal_invalid_tag() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:bad_lang";

        // An invalid BCP 47 tag should produce an error
        let result = store.insert_language_tagged_literal(
            "urn:ruddydoc:doc:bad_lang/p0",
            RDOC_TEXT_CONTENT,
            "test",
            "not a valid tag!!",
            graph,
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn reading_order_filter() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:order_test";

        // Insert elements with reading order
        for i in 0..5 {
            let iri = format!("urn:ruddydoc:doc:order_test/el{i}");
            store.insert_triple_into(&iri, RDF_TYPE, RDOC_PARAGRAPH, graph)?;
            store.insert_literal(&iri, RDOC_READING_ORDER, &i.to_string(), "integer", graph)?;
        }

        // Query elements in reading order
        let sparql = format!(
            "SELECT ?el ?order WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{RDOC_READING_ORDER}> ?order \
               }} \
             }} ORDER BY ?order"
        );
        let json = store.query_to_json(&sparql)?;
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 5);
        Ok(())
    }

    #[test]
    fn introspect_json_reports_classes_and_prefixes() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:doc:introspect_test";

        store.insert_triple_into("urn:s1", RDF_TYPE, RDOC_DOCUMENT, graph)?;
        store.insert_triple_into("urn:s2", RDF_TYPE, RDOC_PARAGRAPH, graph)?;
        store.insert_triple_into("urn:s3", RDF_TYPE, RDOC_PARAGRAPH, graph)?;

        let introspection = store.introspect_json(Some(graph))?;
        assert_eq!(introspection["triples"], 3);
        assert_eq!(introspection["entities"], 3);

        let classes = store.classes_json(graph)?;
        let classes = classes.as_array().expect("expected array");
        assert_eq!(classes.len(), 2);
        let paragraph = classes
            .iter()
            .find(|c| c["class"] == RDOC_PARAGRAPH)
            .expect("expected paragraph class entry");
        assert_eq!(paragraph["instances"], 2);

        let prefixes = store.prefixes_json(graph)?;
        let prefixes = prefixes.as_array().expect("expected array");
        assert!(!prefixes.is_empty());

        Ok(())
    }

    #[test]
    fn introspect_json_missing_graph_is_empty() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let introspection = store.introspect_json(Some("urn:ruddydoc:doc:never_created"))?;
        assert_eq!(introspection["triples"], 0);
        Ok(())
    }

    #[test]
    fn materialize_rdfs_derives_superclass_types() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let ontology = "urn:ruddydoc:test:ontology";
        let doc = "urn:ruddydoc:test:doc";

        // ex:B rdfs:subClassOf ex:A, in the ontology graph.
        store.insert_triple_into(
            "urn:ex:B",
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "urn:ex:A",
            ontology,
        )?;
        // ex:x rdf:type ex:B, in the document graph -- no ex:A assertion.
        store.insert_triple_into("urn:ex:x", RDF_TYPE, "urn:ex:B", doc)?;

        let before = store.query_to_json(&format!(
            "ASK {{ GRAPH <{doc}> {{ <urn:ex:x> <{RDF_TYPE}> <urn:ex:A> }} }}"
        ))?;
        assert_eq!(before, Value::Bool(false));

        let added = store.materialize_rdfs(ontology, doc)?;
        assert!(added > 0, "expected at least one derived triple");

        let after = store.query_to_json(&format!(
            "ASK {{ GRAPH <{doc}> {{ <urn:ex:x> <{RDF_TYPE}> <urn:ex:A> }} }}"
        ))?;
        assert_eq!(after, Value::Bool(true));

        // The derived triple must land in the document graph, not the
        // ontology graph.
        let leaked_into_ontology = store.query_to_json(&format!(
            "ASK {{ GRAPH <{ontology}> {{ <urn:ex:x> <{RDF_TYPE}> <urn:ex:A> }} }}"
        ))?;
        assert_eq!(leaked_into_ontology, Value::Bool(false));

        // Idempotent: materializing again adds nothing further.
        let added_again = store.materialize_rdfs(ontology, doc)?;
        assert_eq!(added_again, 0);

        Ok(())
    }

    #[test]
    fn materialize_rdfs_skips_literal_subject_entailments() -> std::result::Result<(), Error> {
        // rdfs3 (rdfs:range) can validly entail `"value" rdf:type xsd:string`
        // when a property's range is a datatype -- a literal as the triple's
        // *subject*. Regression test: this must not be inserted (it broke
        // ruddydoc-server's IRI-assuming triple copy-loop) and must not be
        // silently miscounted either.
        let store = SparqStore::new()?;
        let ontology = "urn:ruddydoc:test:ontology2";
        let doc = "urn:ruddydoc:test:doc2";

        store.insert_triple_into(
            "urn:ex:hasName",
            "http://www.w3.org/2000/01/rdf-schema#range",
            "http://www.w3.org/2001/XMLSchema#string",
            ontology,
        )?;
        store.insert_literal("urn:ex:thing", "urn:ex:hasName", "hello", "string", doc)?;

        store.materialize_rdfs(ontology, doc)?;

        // The literal-subject entailment must not exist in the document graph.
        let leaked = store.query_to_json(&format!(
            "ASK {{ GRAPH <{doc}> {{ ?s <{RDF_TYPE}> <http://www.w3.org/2001/XMLSchema#string> . FILTER(isLiteral(?s)) }} }}"
        ))?;
        assert_eq!(leaked, Value::Bool(false));

        Ok(())
    }

    const TEST_SHAPES_TTL: &str = r#"
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
        @prefix ex: <urn:ex:> .

        ex:WidgetShape a sh:NodeShape ;
          sh:targetClass ex:Widget ;
          sh:property [
            sh:path ex:name ;
            sh:datatype xsd:string ;
            sh:minCount 1 ;
            sh:message "ex:Widget must have ex:name" ;
          ] .
    "#;

    #[test]
    fn validate_shacl_reports_conforming_document() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        store.load_shapes_turtle("urn:ruddydoc:test:shapes", TEST_SHAPES_TTL)?;

        let doc = "urn:ruddydoc:test:valid_doc";
        store.insert_triple_into("urn:ex:w1", RDF_TYPE, "urn:ex:Widget", doc)?;
        store.insert_literal("urn:ex:w1", "urn:ex:name", "Sprocket", "string", doc)?;

        let report = store.validate_shacl("urn:ruddydoc:test:shapes", doc)?;
        assert_eq!(report["conforms"], true);
        assert_eq!(
            report["results"]
                .as_array()
                .map(std::vec::Vec::len)
                .unwrap_or(usize::MAX),
            0
        );
        Ok(())
    }

    #[test]
    fn validate_shacl_reports_violation_for_missing_property() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        store.load_shapes_turtle("urn:ruddydoc:test:shapes2", TEST_SHAPES_TTL)?;

        let doc = "urn:ruddydoc:test:invalid_doc";
        // A Widget with no ex:name -- violates the minCount 1 constraint.
        store.insert_triple_into("urn:ex:w2", RDF_TYPE, "urn:ex:Widget", doc)?;

        let report = store.validate_shacl("urn:ruddydoc:test:shapes2", doc)?;
        assert_eq!(report["conforms"], false);
        let results = report["results"].as_array().expect("expected array");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["focusNode"], "urn:ex:w2");
        assert_eq!(results[0]["resultMessage"], "ex:Widget must have ex:name");
        Ok(())
    }

    #[test]
    fn search_text_finds_matching_literal_and_its_subject() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:test:search_doc";

        store.insert_literal(
            "urn:ex:p0",
            RDOC_TEXT_CONTENT,
            "The quick brown fox jumps over the lazy dog",
            "string",
            graph,
        )?;
        store.insert_literal(
            "urn:ex:p1",
            RDOC_TEXT_CONTENT,
            "Completely unrelated sentence about weather",
            "string",
            graph,
        )?;

        let hits = store.search_text(graph, "fox", 10)?;
        let hits = hits.as_array().expect("expected array");
        assert_eq!(hits.len(), 1);
        assert!(hits[0]["text"].as_str().unwrap_or("").contains("fox"));

        let references = hits[0]["references"].as_array().expect("expected array");
        assert_eq!(references.len(), 1);
        // Term::to_string() renders IRIs bracket-wrapped, matching
        // query_to_json's existing convention elsewhere in this file.
        assert_eq!(references[0]["subject"], "<urn:ex:p0>");
        assert_eq!(references[0]["predicate"], format!("<{RDOC_TEXT_CONTENT}>"));

        // A query with no matches returns an empty array, not an error.
        let no_hits = store.search_text(graph, "nonexistentword", 10)?;
        assert!(no_hits.as_array().expect("expected array").is_empty());

        Ok(())
    }

    #[test]
    fn search_similar_ranks_by_cosine_similarity() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:test:vectors_doc";

        store.index_embeddings(
            graph,
            vec![
                ("urn:ex:chunk-parallel".to_string(), vec![1.0, 0.0]),
                ("urn:ex:chunk-orthogonal".to_string(), vec![0.0, 1.0]),
                ("urn:ex:chunk-opposite".to_string(), vec![-1.0, 0.0]),
            ],
        )?;

        let results = store.search_similar(graph, &[1.0, 0.0], 2)?;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "urn:ex:chunk-parallel");
        assert!((results[0].1 - 1.0).abs() < 1e-6);
        assert_eq!(results[1].0, "urn:ex:chunk-orthogonal");
        assert!((results[1].1 - 0.0).abs() < 1e-6);

        Ok(())
    }

    #[test]
    fn search_similar_unindexed_graph_is_empty_not_error() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let results = store.search_similar("urn:ruddydoc:test:never_indexed", &[1.0, 0.0], 5)?;
        assert!(results.is_empty());
        Ok(())
    }

    #[test]
    fn index_embeddings_replaces_previous_entries() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:test:reindex_doc";

        store.index_embeddings(graph, vec![("urn:ex:old".to_string(), vec![1.0, 0.0])])?;
        store.index_embeddings(graph, vec![("urn:ex:new".to_string(), vec![0.0, 1.0])])?;

        let results = store.search_similar(graph, &[0.0, 1.0], 10)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "urn:ex:new");
        Ok(())
    }

    #[test]
    fn ask_natural_language_happy_path() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:test:nlq_doc";
        store.insert_triple_into("urn:ex:doc", RDF_TYPE, RDOC_DOCUMENT, graph)?;
        store.insert_triple_into("urn:ex:p0", RDF_TYPE, RDOC_PARAGRAPH, graph)?;

        // A scripted mock LLM: always returns a fixed, valid SPARQL query.
        let answer = store.ask_natural_language(graph, "What paragraphs are there?", |_prompt| {
            Ok(format!("SELECT ?p WHERE {{ ?p a <{RDOC_PARAGRAPH}> }}"))
        })?;

        assert_eq!(answer["repairs"], 0);
        let rows = answer["result"].as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["p"].as_str().unwrap_or("").contains("urn:ex:p0"));

        Ok(())
    }

    #[test]
    fn ask_natural_language_repairs_an_invalid_first_completion() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let graph = "urn:ruddydoc:test:nlq_repair_doc";
        store.insert_triple_into("urn:ex:p0", RDF_TYPE, RDOC_PARAGRAPH, graph)?;

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = call_count.clone();
        let answer = store.ask_natural_language(graph, "What paragraphs are there?", move |_prompt| {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                // An intentionally-invalid first completion (malformed SPARQL).
                Ok("SELECT ?p WHERE { this is not valid sparql".to_string())
            } else {
                Ok(format!("SELECT ?p WHERE {{ ?p a <{RDOC_PARAGRAPH}> }}"))
            }
        })?;

        assert!(
            call_count.load(std::sync::atomic::Ordering::SeqCst) >= 2,
            "expected the repair loop to make a second completion call"
        );
        assert!(answer["repairs"].as_u64().unwrap_or(0) > 0);
        let rows = answer["result"].as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        Ok(())
    }

    #[test]
    fn canonical_hash_is_stable_across_insertion_order() -> std::result::Result<(), Error> {
        let store_a = SparqStore::new()?;
        store_a.insert_triple_into("urn:ex:a", RDF_TYPE, RDOC_DOCUMENT, "urn:ruddydoc:test:graph_a")?;
        store_a.insert_triple_into(
            "urn:ex:b",
            RDF_TYPE,
            RDOC_PARAGRAPH,
            "urn:ruddydoc:test:graph_a",
        )?;

        // Same two triples, inserted in the opposite order, into a
        // differently-named graph.
        let store_b = SparqStore::new()?;
        store_b.insert_triple_into(
            "urn:ex:b",
            RDF_TYPE,
            RDOC_PARAGRAPH,
            "urn:ruddydoc:test:graph_b",
        )?;
        store_b.insert_triple_into("urn:ex:a", RDF_TYPE, RDOC_DOCUMENT, "urn:ruddydoc:test:graph_b")?;

        let hash_a = store_a.canonical_hash("urn:ruddydoc:test:graph_a")?;
        let hash_b = store_b.canonical_hash("urn:ruddydoc:test:graph_b")?;
        assert_eq!(hash_a, hash_b);
        assert_eq!(hash_a.len(), 64, "expected a SHA-256 hex digest");

        Ok(())
    }

    #[test]
    fn canonical_hash_differs_for_different_graphs() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        store.insert_triple_into("urn:ex:a", RDF_TYPE, RDOC_DOCUMENT, "urn:ruddydoc:test:graph_c")?;
        store.insert_triple_into("urn:ex:a", RDF_TYPE, RDOC_PARAGRAPH, "urn:ruddydoc:test:graph_d")?;

        let hash_c = store.canonical_hash("urn:ruddydoc:test:graph_c")?;
        let hash_d = store.canonical_hash("urn:ruddydoc:test:graph_d")?;
        assert_ne!(hash_c, hash_d);

        Ok(())
    }

    #[test]
    fn record_conversion_provenance_adds_prov_o_lineage() -> std::result::Result<(), Error> {
        let store = SparqStore::new()?;
        let doc_graph = "urn:ruddydoc:doc:abc123";
        store.insert_triple_into("urn:ex:a", RDF_TYPE, RDOC_DOCUMENT, doc_graph)?;

        let before = store.triple_count_in(doc_graph)?;
        store.record_conversion_provenance(doc_graph, "urn:ruddydoc:source:abc123")?;
        let after = store.triple_count_in(doc_graph)?;
        assert!(after > before, "expected new PROV-O triples to be inserted");

        let results = store.query_to_json(&format!(
            "SELECT ?p ?o WHERE {{ GRAPH <{doc_graph}> {{ <{doc_graph}> \
             <http://www.w3.org/ns/prov#wasGeneratedBy> ?activity . \
             ?activity ?p ?o }} }}"
        ))?;
        let rows = results.as_array().expect("SELECT returns an array");
        assert!(
            !rows.is_empty(),
            "expected the document graph's own IRI to be a prov:Entity wasGeneratedBy an activity"
        );

        let used = store.query_to_json(&format!(
            "ASK {{ GRAPH <{doc_graph}> {{ ?activity <http://www.w3.org/ns/prov#used> \
             <urn:ruddydoc:source:abc123> }} }}"
        ))?;
        assert_eq!(used, serde_json::json!(true));

        Ok(())
    }
}
