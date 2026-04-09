//! Oxigraph-based document store for RuddyDoc.
//!
//! This crate wraps `oxigraph::store::Store` and implements the
//! `DocumentStore` trait from `ruddydoc-core`. No other crate in the
//! workspace should depend on Oxigraph directly.

use oxigraph::io::RdfFormat;
use oxigraph::model::{
    GraphName, GraphNameRef, Literal, NamedNode, NamedNodeRef, Quad, QuadRef, TripleRef, vocab::xsd,
};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use serde_json::{Map, Value};

use ruddydoc_core::{DocumentStore, Error};

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

/// Oxigraph-backed document store.
///
/// Each parsed document is stored in its own named graph, enabling
/// per-document queries and multi-document SPARQL queries.
pub struct OxigraphStore {
    store: Store,
}

impl OxigraphStore {
    /// Create a new in-memory document store.
    pub fn new() -> std::result::Result<Self, Error> {
        Ok(Self {
            store: Store::new()?,
        })
    }

    /// Serialize a named graph using Oxigraph's built-in serialization.
    fn serialize_graph_internal(
        &self,
        graph: &str,
        rdf_format: RdfFormat,
    ) -> ruddydoc_core::Result<String> {
        let g = iri_escape(graph);
        let g_node = NamedNode::new(&g)?;

        let mut buf = Vec::new();
        let quads: Vec<Quad> = self
            .store
            .quads_for_pattern(None, None, None, Some(g_node.as_ref().into()))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Write quads as triples (strip the graph component for serialization)
        let serializer = oxigraph::io::RdfSerializer::from_format(rdf_format);
        let mut writer = serializer.for_writer(&mut buf);
        for quad in &quads {
            let quad_ref = quad.as_ref();
            let triple = TripleRef::new(quad_ref.subject, quad_ref.predicate, quad_ref.object);
            writer.serialize_triple(triple)?;
        }
        writer.finish()?;

        Ok(String::from_utf8(buf)?)
    }
}

impl DocumentStore for OxigraphStore {
    fn insert_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
    ) -> ruddydoc_core::Result<()> {
        let s = iri_escape(subject);
        let p = iri_escape(predicate);
        let o = iri_escape(object);

        let s_node = NamedNodeRef::new(&s)?;
        let p_node = NamedNodeRef::new(&p)?;
        let o_node = NamedNodeRef::new(&o)?;

        self.store.insert(QuadRef::new(
            s_node,
            p_node,
            o_node,
            GraphNameRef::DefaultGraph,
        ))?;
        Ok(())
    }

    fn insert_triple_into(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        graph: &str,
    ) -> ruddydoc_core::Result<()> {
        let s = iri_escape(subject);
        let p = iri_escape(predicate);
        let o = iri_escape(object);
        let g = iri_escape(graph);

        let s_node = NamedNodeRef::new(&s)?;
        let p_node = NamedNodeRef::new(&p)?;
        let o_node = NamedNodeRef::new(&o)?;
        let g_node = NamedNodeRef::new(&g)?;

        self.store
            .insert(QuadRef::new(s_node, p_node, o_node, g_node))?;
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
        let s = iri_escape(subject);
        let p = iri_escape(predicate);
        let g = iri_escape(graph);

        let s_node = NamedNode::new(&s)?;
        let p_node = NamedNode::new(&p)?;
        let g_node = GraphName::NamedNode(NamedNode::new(&g)?);

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

        self.store.insert(QuadRef::new(
            s_node.as_ref(),
            p_node.as_ref(),
            &literal,
            g_node.as_ref(),
        ))?;
        Ok(())
    }

    fn query_to_json(&self, sparql: &str) -> ruddydoc_core::Result<Value> {
        let results = SparqlEvaluator::new()
            .parse_query(sparql)?
            .on_store(&self.store)
            .execute()?;

        match results {
            QueryResults::Solutions(solutions) => {
                let variables: Vec<String> = solutions
                    .variables()
                    .iter()
                    .map(|v| v.as_str().to_owned())
                    .collect();

                let mut rows = Vec::new();
                for solution in solutions {
                    let solution = solution?;
                    let mut row = Map::new();
                    for var in &variables {
                        let value = solution
                            .get(var.as_str())
                            .map_or(Value::Null, |term| Value::String(term.to_string()));
                        row.insert(var.clone(), value);
                    }
                    rows.push(Value::Object(row));
                }
                Ok(Value::Array(rows))
            }
            QueryResults::Boolean(b) => Ok(Value::Bool(b)),
            QueryResults::Graph(_) => Err("CONSTRUCT/DESCRIBE queries not supported".into()),
        }
    }

    fn clear(&self) -> ruddydoc_core::Result<()> {
        self.store.clear()?;
        Ok(())
    }

    fn clear_graph(&self, graph: &str) -> ruddydoc_core::Result<()> {
        let g = iri_escape(graph);
        let g_node = NamedNode::new(&g)?;
        self.store.clear_graph(g_node.as_ref())?;
        Ok(())
    }

    fn serialize_graph(&self, graph: &str, format: &str) -> ruddydoc_core::Result<String> {
        match format {
            "turtle" | "ttl" => self.serialize_graph_internal(graph, RdfFormat::Turtle),
            "ntriples" | "nt" => self.serialize_graph_internal(graph, RdfFormat::NTriples),
            "rdfxml" | "rdf" => self.serialize_graph_internal(graph, RdfFormat::RdfXml),
            _ => Err(format!("unsupported serialization format: {format}").into()),
        }
    }

    fn triple_count(&self) -> ruddydoc_core::Result<usize> {
        Ok(self.store.len()?)
    }

    fn triple_count_in(&self, graph: &str) -> ruddydoc_core::Result<usize> {
        let g = iri_escape(graph);
        let g_node = NamedNode::new(&g)?;
        let count = self
            .store
            .quads_for_pattern(None, None, None, Some(g_node.as_ref().into()))
            .count();
        Ok(count)
    }

    fn insert_language_tagged_literal(
        &self,
        subject: &str,
        predicate: &str,
        value: &str,
        language: &str,
        graph: &str,
    ) -> ruddydoc_core::Result<()> {
        let s = iri_escape(subject);
        let p = iri_escape(predicate);
        let g = iri_escape(graph);

        let s_node = NamedNode::new(&s)?;
        let p_node = NamedNode::new(&p)?;
        let g_node = GraphName::NamedNode(NamedNode::new(&g)?);

        let literal = Literal::new_language_tagged_literal(value, language)
            .map_err(|e| format!("invalid BCP 47 language tag '{language}': {e}"))?;

        self.store.insert(QuadRef::new(
            s_node.as_ref(),
            p_node.as_ref(),
            &literal,
            g_node.as_ref(),
        ))?;
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
        let store = OxigraphStore::new()?;
        assert_eq!(store.triple_count()?, 0);
        Ok(())
    }

    #[test]
    fn insert_and_query_default_graph() -> std::result::Result<(), Error> {
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
        let result = store.serialize_graph("urn:g", "yaml");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn ask_query_returns_boolean() -> std::result::Result<(), Error> {
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        // Oxigraph renders language-tagged literals as "value"@lang
        assert!(text_val.contains("Bonjour"));
        Ok(())
    }

    #[test]
    fn language_tagged_literal_with_lang_filter() -> std::result::Result<(), Error> {
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
        let store = OxigraphStore::new()?;
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
}
