//! JSON-LD exporter producing schema.org-compatible linked data.
//!
//! Queries the document graph via SPARQL and produces a JSON-LD structure
//! using the schema.org vocabulary with RuddyDoc extensions. The output
//! uses the schema.org bridge mappings defined in the ontology.

use serde::Serialize;

use ruddydoc_core::{DocumentExporter, DocumentStore, OutputFormat};
use ruddydoc_ontology as ont;

/// JSON-LD exporter producing schema.org-compatible output.
pub struct JsonLdExporter;

#[derive(Serialize)]
struct JsonLdContext {
    schema: &'static str,
    rdoc: &'static str,
    dcterms: &'static str,
}

#[derive(Serialize)]
struct JsonLdElement {
    #[serde(rename = "@type")]
    at_type: String,
    #[serde(rename = "rdoc:textContent")]
    text_content: String,
    #[serde(rename = "rdoc:headingLevel", skip_serializing_if = "Option::is_none")]
    heading_level: Option<i64>,
    #[serde(rename = "rdoc:readingOrder")]
    reading_order: i64,
    #[serde(rename = "rdoc:codeLanguage", skip_serializing_if = "Option::is_none")]
    code_language: Option<String>,
}

#[derive(Serialize)]
struct JsonLdDocument {
    #[serde(rename = "@context")]
    context: JsonLdContext,
    #[serde(rename = "@type")]
    at_type: &'static str,
    #[serde(rename = "schema:name")]
    name: String,
    #[serde(rename = "schema:author", skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(
        rename = "schema:datePublished",
        skip_serializing_if = "Option::is_none"
    )]
    date_published: Option<String>,
    #[serde(
        rename = "schema:numberOfPages",
        skip_serializing_if = "Option::is_none"
    )]
    number_of_pages: Option<i64>,
    #[serde(rename = "schema:inLanguage", skip_serializing_if = "Option::is_none")]
    in_language: Option<String>,
    #[serde(rename = "rdoc:sourceFormat")]
    source_format: String,
    #[serde(rename = "rdoc:hasElement")]
    has_element: Vec<JsonLdElement>,
}

impl DocumentExporter for JsonLdExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::JsonLd
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        let name = query_document_name(store, doc_graph)?;
        let source_format = query_source_format(store, doc_graph)?;
        let author = query_optional_literal(store, doc_graph, "http://purl.org/dc/terms/creator")?;
        let date_published =
            query_optional_literal(store, doc_graph, "http://purl.org/dc/terms/date")?;
        let number_of_pages = query_page_count(store, doc_graph)?;
        let in_language = query_optional_literal(store, doc_graph, &ont::iri(ont::PROP_LANGUAGE))?;
        let elements = query_elements(store, doc_graph)?;

        let doc = JsonLdDocument {
            context: JsonLdContext {
                schema: "https://schema.org/",
                rdoc: "https://ruddydoc.chapeaux.io/ontology#",
                dcterms: "http://purl.org/dc/terms/",
            },
            at_type: "schema:CreativeWork",
            name,
            author,
            date_published,
            number_of_pages,
            in_language,
            source_format,
            has_element: elements,
        };

        let json_str = serde_json::to_string_pretty(&doc)?;
        Ok(json_str)
    }
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Extract a clean string from a SPARQL literal result.
fn clean_literal(s: &str) -> String {
    if let Some(idx) = s.find("\"^^<") {
        return s[1..idx].to_string();
    }
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// Parse an integer from a SPARQL literal result.
fn parse_int(s: &str) -> i64 {
    let cleaned = clean_literal(s);
    cleaned.parse().unwrap_or(0)
}

fn query_document_name(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<String> {
    let sparql = format!(
        "SELECT ?name WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?doc a <{doc_class}>. \
             ?doc <{file_name}> ?name \
           }} \
         }} LIMIT 1",
        doc_class = ont::iri(ont::CLASS_DOCUMENT),
        file_name = ont::iri(ont::PROP_FILE_NAME),
    );
    let result = store.query_to_json(&sparql)?;
    let name = result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("name"))
        .and_then(|v| v.as_str())
        .map(clean_literal)
        .unwrap_or_else(|| "unknown".to_string());
    Ok(name)
}

fn query_source_format(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<String> {
    let sparql = format!(
        "SELECT ?fmt WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?doc a <{doc_class}>. \
             ?doc <{source_format}> ?fmt \
           }} \
         }} LIMIT 1",
        doc_class = ont::iri(ont::CLASS_DOCUMENT),
        source_format = ont::iri(ont::PROP_SOURCE_FORMAT),
    );
    let result = store.query_to_json(&sparql)?;
    let fmt = result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("fmt"))
        .and_then(|v| v.as_str())
        .map(clean_literal)
        .unwrap_or_else(|| "unknown".to_string());
    Ok(fmt)
}

fn query_optional_literal(
    store: &dyn DocumentStore,
    doc_graph: &str,
    predicate_iri: &str,
) -> ruddydoc_core::Result<Option<String>> {
    let sparql = format!(
        "SELECT ?val WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?doc a <{doc_class}>. \
             ?doc <{predicate_iri}> ?val \
           }} \
         }} LIMIT 1",
        doc_class = ont::iri(ont::CLASS_DOCUMENT),
    );
    let result = store.query_to_json(&sparql)?;
    Ok(result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("val"))
        .and_then(|v| v.as_str())
        .map(clean_literal))
}

fn query_page_count(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<Option<i64>> {
    let sparql = format!(
        "SELECT ?count WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?doc a <{doc_class}>. \
             ?doc <{page_count}> ?count \
           }} \
         }} LIMIT 1",
        doc_class = ont::iri(ont::CLASS_DOCUMENT),
        page_count = ont::iri(ont::PROP_PAGE_COUNT),
    );
    let result = store.query_to_json(&sparql)?;
    Ok(result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("count"))
        .and_then(|v| v.as_str())
        .map(parse_int))
}

fn query_elements(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<Vec<JsonLdElement>> {
    // Query all text elements with their types and reading order
    let sparql = format!(
        "SELECT ?type ?text ?order ?level ?lang WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?el <{reading_order}> ?order. \
             ?el a ?type. \
             ?el <{text_content}> ?text. \
             OPTIONAL {{ ?el <{heading_level}> ?level }} \
             OPTIONAL {{ ?el <{code_language}> ?lang }} \
             FILTER(?type IN ( \
               <{section_header}>, \
               <{paragraph}>, \
               <{list_item}>, \
               <{code}>, \
               <{title}>, \
               <{footnote}>, \
               <{caption}>, \
               <{formula}> \
             )) \
           }} \
         }} ORDER BY ?order",
        reading_order = ont::iri(ont::PROP_READING_ORDER),
        text_content = ont::iri(ont::PROP_TEXT_CONTENT),
        heading_level = ont::iri(ont::PROP_HEADING_LEVEL),
        code_language = ont::iri(ont::PROP_CODE_LANGUAGE),
        section_header = ont::iri(ont::CLASS_SECTION_HEADER),
        paragraph = ont::iri(ont::CLASS_PARAGRAPH),
        list_item = ont::iri(ont::CLASS_LIST_ITEM),
        code = ont::iri(ont::CLASS_CODE),
        title = ont::iri(ont::CLASS_TITLE),
        footnote = ont::iri(ont::CLASS_FOOTNOTE),
        caption = ont::iri(ont::CLASS_CAPTION),
        formula = ont::iri(ont::CLASS_FORMULA),
    );

    let result = store.query_to_json(&sparql)?;
    let mut elements = Vec::new();

    if let Some(rows) = result.as_array() {
        for row in rows {
            let type_str = row.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let text = row
                .get("text")
                .and_then(|v| v.as_str())
                .map(clean_literal)
                .unwrap_or_default();
            let order = row
                .get("order")
                .and_then(|v| v.as_str())
                .map(parse_int)
                .unwrap_or(0);

            let rdoc_type = extract_rdoc_type(type_str);

            let heading_level = row.get("level").and_then(|v| v.as_str()).map(parse_int);

            let code_language = row.get("lang").and_then(|v| v.as_str()).map(clean_literal);

            elements.push(JsonLdElement {
                at_type: format!("rdoc:{rdoc_type}"),
                text_content: text,
                heading_level,
                reading_order: order,
                code_language,
            });
        }
    }

    Ok(elements)
}

/// Extract the local name from a full ontology IRI for use as an RDF type.
fn extract_rdoc_type(type_iri: &str) -> String {
    // The type IRI looks like "<https://ruddydoc.chapeaux.io/ontology#SectionHeader>"
    // or "https://ruddydoc.chapeaux.io/ontology#SectionHeader"
    if let Some(idx) = type_iri.rfind('#') {
        let local = &type_iri[idx + 1..];
        // Strip trailing '>' if present
        local.trim_end_matches('>').to_string()
    } else {
        type_iri
            .trim_start_matches('<')
            .trim_end_matches('>')
            .to_string()
    }
}
