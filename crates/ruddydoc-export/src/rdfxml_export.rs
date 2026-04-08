//! RDF/XML exporter: serialize a document graph as RDF/XML.
//!
//! Delegates to Oxigraph's built-in RDF/XML serialization via the
//! `DocumentStore::serialize_graph` method.

use ruddydoc_core::{DocumentExporter, DocumentStore, OutputFormat};

/// RDF/XML exporter using the store's built-in serialization.
pub struct RdfXmlExporter;

impl DocumentExporter for RdfXmlExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::RdfXml
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        store.serialize_graph(doc_graph, "rdfxml")
    }
}
