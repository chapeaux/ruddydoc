//! XML parser backends (JATS, USPTO, generic) for RuddyDoc.
//!
//! Uses `quick-xml` event-based parsing to convert XML documents into the
//! RuddyDoc document ontology graph. Supports JATS (Journal Article Tag
//! Suite), USPTO patent XML, and a generic XML fallback.

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

// -----------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------

/// Dublin Core Terms namespace for metadata properties.
const DCTERMS: &str = "http://purl.org/dc/terms/";

// -----------------------------------------------------------------------
// XML type detection
// -----------------------------------------------------------------------

/// The detected XML schema type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XmlType {
    /// Journal Article Tag Suite (scientific articles).
    Jats,
    /// US Patent Office XML format.
    Uspto,
    /// Unknown/generic XML.
    Generic,
}

/// Detect the XML schema type by examining the beginning of the content.
///
/// Inspects the DOCTYPE declaration and root element name/namespace to
/// determine whether this is JATS, USPTO, or generic XML.
fn detect_xml_type(content: &str) -> XmlType {
    let lower = content.to_ascii_lowercase();

    // Check DOCTYPE for NLM (JATS predecessor)
    if lower.contains("<!doctype") && lower.contains("-//nlm//") {
        return XmlType::Jats;
    }

    // Parse to find the root element
    let mut reader = Reader::from_str(content);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = local_name_str(e);
                let local_lower = local.to_ascii_lowercase();

                // USPTO detection
                if local_lower == "us-patent-grant" || local_lower == "us-patent-application" {
                    return XmlType::Uspto;
                }

                // JATS detection: root <article> with JATS-related namespace
                if local_lower == "article" {
                    for attr in e.attributes().flatten() {
                        let val = String::from_utf8_lossy(attr.value.as_ref()).to_ascii_lowercase();
                        if val.contains("jats")
                            || val.contains("archiving")
                            || val.contains("publishing")
                            || val.contains("authoring")
                        {
                            return XmlType::Jats;
                        }
                    }
                    // Even without a JATS namespace, an <article> root often
                    // implies JATS in practice.
                    return XmlType::Jats;
                }

                // If the first start element is something else, it is generic.
                return XmlType::Generic;
            }
            Ok(Event::Eof) => return XmlType::Generic,
            Err(_) => return XmlType::Generic,
            _ => {
                // Skip declarations, processing instructions, comments, etc.
            }
        }
    }
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Compute a SHA-256 hash of the content bytes, returning a hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode(result.as_slice())
}

/// Hex-encode bytes.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Extract the local name (without namespace prefix) from a `BytesStart` event.
fn local_name_str(e: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).to_string()
}

/// Get an attribute value by local name from a start tag.
fn get_attr(e: &BytesStart<'_>, name: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        let local_name = attr.key.local_name();
        let key = String::from_utf8_lossy(local_name.as_ref());
        if key == name {
            return Some(String::from_utf8_lossy(attr.value.as_ref()).to_string());
        }
    }
    None
}

// -----------------------------------------------------------------------
// Parse context (shared state machine)
// -----------------------------------------------------------------------

/// Shared state machine context for XML parsing.
struct ParseContext<'a> {
    store: &'a dyn DocumentStore,
    doc_graph: &'a str,
    doc_hash: &'a str,
    /// Sequential reading order counter.
    reading_order: usize,
    /// Stack of parent element IRIs for tree structure.
    parent_stack: Vec<String>,
    /// The last element IRI at each depth, for next/previous linking.
    last_sibling_at_depth: Vec<Option<String>>,
    /// Current text accumulator.
    text_buf: String,
    /// All element IRIs in order.
    all_elements: Vec<String>,
}

impl<'a> ParseContext<'a> {
    fn new(store: &'a dyn DocumentStore, doc_graph: &'a str, doc_hash: &'a str) -> Self {
        Self {
            store,
            doc_graph,
            doc_hash,
            reading_order: 0,
            parent_stack: Vec::new(),
            last_sibling_at_depth: Vec::new(),
            text_buf: String::new(),
            all_elements: Vec::new(),
        }
    }

    /// Generate a unique element IRI.
    fn element_iri(&self, kind: &str) -> String {
        ruddydoc_core::element_iri(self.doc_hash, &format!("{kind}-{}", self.reading_order))
    }

    /// Insert an element into the graph with its type, reading order, and tree links.
    fn emit_element(&mut self, element_iri: &str, class_name: &str) -> ruddydoc_core::Result<()> {
        let rdf_type = ont::rdf_iri("type");
        let class_iri = ont::iri(class_name);
        let doc_iri = ruddydoc_core::doc_iri(self.doc_hash);
        let g = self.doc_graph;

        // rdf:type
        self.store
            .insert_triple_into(element_iri, &rdf_type, &class_iri, g)?;

        // rdoc:readingOrder
        self.store.insert_literal(
            element_iri,
            &ont::iri(ont::PROP_READING_ORDER),
            &self.reading_order.to_string(),
            "integer",
            g,
        )?;

        // rdoc:hasElement (document -> element)
        self.store.insert_triple_into(
            &doc_iri,
            &ont::iri(ont::PROP_HAS_ELEMENT),
            element_iri,
            g,
        )?;

        // Parent-child links
        if let Some(parent) = self.parent_stack.last() {
            self.store.insert_triple_into(
                element_iri,
                &ont::iri(ont::PROP_PARENT_ELEMENT),
                parent,
                g,
            )?;
            self.store.insert_triple_into(
                parent,
                &ont::iri(ont::PROP_CHILD_ELEMENT),
                element_iri,
                g,
            )?;
        }

        // Previous/next sibling links
        let depth = self.parent_stack.len();
        while self.last_sibling_at_depth.len() <= depth {
            self.last_sibling_at_depth.push(None);
        }
        if let Some(prev) = &self.last_sibling_at_depth[depth] {
            self.store.insert_triple_into(
                prev,
                &ont::iri(ont::PROP_NEXT_ELEMENT),
                element_iri,
                g,
            )?;
            self.store.insert_triple_into(
                element_iri,
                &ont::iri(ont::PROP_PREVIOUS_ELEMENT),
                prev,
                g,
            )?;
        }
        self.last_sibling_at_depth[depth] = Some(element_iri.to_string());

        self.all_elements.push(element_iri.to_string());
        self.reading_order += 1;

        Ok(())
    }

    /// Set text content on an element.
    fn set_text_content(&self, element_iri: &str, text: &str) -> ruddydoc_core::Result<()> {
        self.store.insert_literal(
            element_iri,
            &ont::iri(ont::PROP_TEXT_CONTENT),
            text,
            "string",
            self.doc_graph,
        )
    }
}

// -----------------------------------------------------------------------
// XmlBackend
// -----------------------------------------------------------------------

/// XML document backend handling JATS, USPTO, and generic XML.
pub struct XmlBackend;

impl XmlBackend {
    /// Create a new XML backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for XmlBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentBackend for XmlBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Xml]
    }

    fn supports_pagination(&self) -> bool {
        false
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(path.extension().and_then(|e| e.to_str()), Some("xml"))
            }
            DocumentSource::Stream { name, .. } => name.ends_with(".xml"),
        }
    }

    fn parse(
        &self,
        source: &DocumentSource,
        store: &dyn DocumentStore,
        doc_graph: &str,
    ) -> ruddydoc_core::Result<DocumentMeta> {
        // Read the source content
        let (content, file_path, file_name) = match source {
            DocumentSource::File(path) => {
                let content = std::fs::read_to_string(path)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".to_string());
                (content, Some(path.clone()), name)
            }
            DocumentSource::Stream { name, data } => {
                let content = String::from_utf8(data.clone())?;
                (content, None, name.clone())
            }
        };

        let file_size = content.len() as u64;
        let hash_str = compute_hash(content.as_bytes());
        let doc_hash = DocumentHash(hash_str.clone());

        // Create the document node
        let doc_iri = ruddydoc_core::doc_iri(&hash_str);
        let rdf_type = ont::rdf_iri("type");
        let g = doc_graph;

        store.insert_triple_into(&doc_iri, &rdf_type, &ont::iri(ont::CLASS_DOCUMENT), g)?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_SOURCE_FORMAT),
            "xml",
            "string",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_DOCUMENT_HASH),
            &hash_str,
            "string",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_FILE_NAME),
            &file_name,
            "string",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_FILE_SIZE),
            &file_size.to_string(),
            "integer",
            g,
        )?;

        // Dispatch to the appropriate parser
        let xml_type = detect_xml_type(&content);
        match xml_type {
            XmlType::Jats => parse_jats(&content, store, g, &hash_str)?,
            XmlType::Uspto => parse_uspto(&content, store, g, &hash_str)?,
            XmlType::Generic => parse_generic(&content, store, g, &hash_str)?,
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Xml,
            file_size,
            page_count: None,
            language: None,
        })
    }
}

// =======================================================================
// JATS Parser
// =======================================================================

/// Parse a JATS (Journal Article Tag Suite) XML document.
#[allow(unused_variables, unused_assignments)]
fn parse_jats(
    content: &str,
    store: &dyn DocumentStore,
    doc_graph: &str,
    doc_hash: &str,
) -> ruddydoc_core::Result<()> {
    let mut ctx = ParseContext::new(store, doc_graph, doc_hash);
    let doc_iri = ruddydoc_core::doc_iri(doc_hash);
    let g = doc_graph;

    let mut reader = Reader::from_str(content);

    // State tracking
    let mut tag_stack: Vec<String> = Vec::new();
    let mut in_article_title = false;
    let mut in_abstract = false;
    let mut in_sec = false;
    let mut in_sec_title = false;
    let mut in_p = false;
    let mut in_list = false;
    let mut in_list_item = false;
    let mut in_table_wrap = false;
    let mut in_td = false;
    let mut in_th = false;
    let mut in_fig = false;
    let mut in_caption = false;
    let mut in_disp_formula = false;
    let mut in_code = false;
    let mut in_fn = false;
    let mut in_xref = false;
    let mut in_contrib = false;
    let mut in_surname = false;
    let mut in_given_names = false;
    let mut in_pub_date = false;
    let mut in_year = false;
    let mut in_month = false;
    let mut in_day = false;

    // Metadata accumulators
    let mut contrib_surname = String::new();
    let mut contrib_given = String::new();
    let mut pub_year = String::new();
    let mut pub_month = String::new();
    let mut pub_day = String::new();

    // Table state
    let mut table_iri: Option<String> = None;
    let mut table_row: usize = 0;
    let mut table_col: usize = 0;
    let mut table_max_col: usize = 0;

    // List tracking
    #[allow(unused_assignments)]
    let mut list_is_ordered = false;

    // Section heading depth tracking
    let mut sec_depth: usize = 0;

    // Xref citation key
    let mut xref_rid = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name_str(e);
                tag_stack.push(local.clone());

                match local.as_str() {
                    "article-title" => {
                        ctx.text_buf.clear();
                        in_article_title = true;
                    }
                    "abstract" => {
                        in_abstract = true;
                        let iri = ctx.element_iri("group");
                        ctx.emit_element(&iri, ont::CLASS_GROUP)?;
                        ctx.parent_stack.push(iri);
                    }
                    "sec" => {
                        in_sec = true;
                        sec_depth += 1;
                        let iri = ctx.element_iri("group");
                        ctx.emit_element(&iri, ont::CLASS_GROUP)?;
                        ctx.parent_stack.push(iri);
                    }
                    "title" if in_sec || in_abstract => {
                        ctx.text_buf.clear();
                        in_sec_title = true;
                    }
                    "p" if !in_caption && !in_fn => {
                        ctx.text_buf.clear();
                        in_p = true;
                    }
                    "list" => {
                        in_list = true;
                        let list_type = get_attr(e, "list-type").unwrap_or_default();
                        list_is_ordered = list_type == "order" || list_type == "ordered";
                        let class = if list_is_ordered {
                            ont::CLASS_ORDERED_LIST
                        } else {
                            ont::CLASS_UNORDERED_LIST
                        };
                        let iri = ctx.element_iri("list");
                        ctx.emit_element(&iri, class)?;
                        ctx.parent_stack.push(iri);
                    }
                    "list-item" => {
                        ctx.text_buf.clear();
                        in_list_item = true;
                    }
                    "table-wrap" => {
                        in_table_wrap = true;
                        table_row = 0;
                        table_col = 0;
                        table_max_col = 0;
                        let iri = ctx.element_iri("table");
                        ctx.emit_element(&iri, ont::CLASS_TABLE_ELEMENT)?;
                        table_iri = Some(iri.clone());
                        ctx.parent_stack.push(iri);
                    }
                    "tr" if in_table_wrap => {
                        table_col = 0;
                    }
                    "td" if in_table_wrap => {
                        ctx.text_buf.clear();
                        in_td = true;
                    }
                    "th" if in_table_wrap => {
                        ctx.text_buf.clear();
                        in_th = true;
                    }
                    "fig" => {
                        in_fig = true;
                        let iri = ctx.element_iri("picture");
                        ctx.emit_element(&iri, ont::CLASS_PICTURE_ELEMENT)?;
                        ctx.parent_stack.push(iri);
                    }
                    "caption" => {
                        ctx.text_buf.clear();
                        in_caption = true;
                    }
                    "disp-formula" => {
                        ctx.text_buf.clear();
                        in_disp_formula = true;
                    }
                    "code" => {
                        ctx.text_buf.clear();
                        in_code = true;
                    }
                    "fn" => {
                        ctx.text_buf.clear();
                        in_fn = true;
                    }
                    "xref" => {
                        in_xref = true;
                        xref_rid = get_attr(e, "rid").unwrap_or_default();
                        ctx.text_buf.clear();
                    }
                    // Front matter metadata
                    "contrib" => {
                        in_contrib = true;
                        contrib_surname.clear();
                        contrib_given.clear();
                    }
                    "surname" if in_contrib => {
                        ctx.text_buf.clear();
                        in_surname = true;
                    }
                    "given-names" if in_contrib => {
                        ctx.text_buf.clear();
                        in_given_names = true;
                    }
                    "pub-date" => {
                        in_pub_date = true;
                        pub_year.clear();
                        pub_month.clear();
                        pub_day.clear();
                    }
                    "year" if in_pub_date => {
                        ctx.text_buf.clear();
                        in_year = true;
                    }
                    "month" if in_pub_date => {
                        ctx.text_buf.clear();
                        in_month = true;
                    }
                    "day" if in_pub_date => {
                        ctx.text_buf.clear();
                        in_day = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                tag_stack.pop();

                match local.as_str() {
                    "article-title" => {
                        if in_article_title {
                            in_article_title = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("title");
                                ctx.emit_element(&iri, ont::CLASS_TITLE)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }
                    }
                    "abstract" => {
                        if in_abstract {
                            in_abstract = false;
                            ctx.parent_stack.pop();
                        }
                    }
                    "sec" => {
                        if in_sec {
                            sec_depth -= 1;
                            if sec_depth == 0 {
                                in_sec = false;
                            }
                            ctx.parent_stack.pop();
                        }
                    }
                    "title" if in_sec_title => {
                        in_sec_title = false;
                        let text = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                        if !text.is_empty() {
                            let level = sec_depth.min(6);
                            let iri = ctx.element_iri("heading");
                            ctx.emit_element(&iri, ont::CLASS_SECTION_HEADER)?;
                            ctx.set_text_content(&iri, &text)?;
                            store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_HEADING_LEVEL),
                                &level.to_string(),
                                "integer",
                                g,
                            )?;
                        }
                    }
                    "p" => {
                        if in_p {
                            in_p = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("paragraph");
                                ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }
                    }
                    "list" => {
                        if in_list {
                            in_list = false;
                            ctx.parent_stack.pop();
                        }
                    }
                    "list-item" => {
                        if in_list_item {
                            in_list_item = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("listitem");
                                ctx.emit_element(&iri, ont::CLASS_LIST_ITEM)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }
                    }
                    "table-wrap" => {
                        if in_table_wrap {
                            in_table_wrap = false;
                            if let Some(t_iri) = table_iri.take() {
                                store.insert_literal(
                                    &t_iri,
                                    &ont::iri(ont::PROP_ROW_COUNT),
                                    &table_row.to_string(),
                                    "integer",
                                    g,
                                )?;
                                store.insert_literal(
                                    &t_iri,
                                    &ont::iri(ont::PROP_COLUMN_COUNT),
                                    &table_max_col.to_string(),
                                    "integer",
                                    g,
                                )?;
                            }
                            ctx.parent_stack.pop();
                        }
                    }
                    "tr" if in_table_wrap => {
                        table_row += 1;
                        if table_col > table_max_col {
                            table_max_col = table_col;
                        }
                    }
                    "td" if in_td => {
                        in_td = false;
                        let text = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                        emit_table_cell(&ctx, &table_iri, table_row, table_col, &text, false)?;
                        table_col += 1;
                    }
                    "th" if in_th => {
                        in_th = false;
                        let text = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                        emit_table_cell(&ctx, &table_iri, table_row, table_col, &text, true)?;
                        table_col += 1;
                    }
                    "fig" => {
                        if in_fig {
                            in_fig = false;
                            ctx.parent_stack.pop();
                        }
                    }
                    "caption" => {
                        if in_caption {
                            in_caption = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("caption");
                                ctx.emit_element(&iri, ont::CLASS_CAPTION)?;
                                ctx.set_text_content(&iri, &text)?;

                                // Link caption to parent figure if applicable
                                if in_fig && let Some(parent) = ctx.parent_stack.last() {
                                    store.insert_triple_into(
                                        parent,
                                        &ont::iri(ont::PROP_HAS_CAPTION),
                                        &iri,
                                        g,
                                    )?;
                                }
                            }
                        }
                    }
                    "disp-formula" => {
                        if in_disp_formula {
                            in_disp_formula = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("formula");
                                ctx.emit_element(&iri, ont::CLASS_FORMULA)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }
                    }
                    "code" => {
                        if in_code {
                            in_code = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("code");
                                ctx.emit_element(&iri, ont::CLASS_CODE)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }
                    }
                    "fn" => {
                        if in_fn {
                            in_fn = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("footnote");
                                ctx.emit_element(&iri, ont::CLASS_FOOTNOTE)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }
                    }
                    "xref" => {
                        if in_xref {
                            in_xref = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !xref_rid.is_empty() {
                                let iri = ctx.element_iri("reference");
                                ctx.emit_element(&iri, ont::CLASS_REFERENCE)?;
                                if !text.is_empty() {
                                    ctx.set_text_content(&iri, &text)?;
                                }
                                store.insert_literal(
                                    &iri,
                                    &ont::iri(ont::PROP_CITATION_KEY),
                                    &xref_rid,
                                    "string",
                                    g,
                                )?;
                                xref_rid.clear();
                            }
                        }
                    }
                    "surname" if in_surname => {
                        in_surname = false;
                        contrib_surname = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                    }
                    "given-names" if in_given_names => {
                        in_given_names = false;
                        contrib_given = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                    }
                    "contrib" => {
                        if in_contrib {
                            in_contrib = false;
                            let author = if !contrib_given.is_empty() {
                                format!("{} {}", contrib_given.trim(), contrib_surname.trim())
                            } else {
                                contrib_surname.trim().to_string()
                            };
                            if !author.is_empty() {
                                store.insert_literal(
                                    &doc_iri,
                                    &format!("{DCTERMS}creator"),
                                    &author,
                                    "string",
                                    g,
                                )?;
                            }
                        }
                    }
                    "year" if in_year => {
                        in_year = false;
                        pub_year = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                    }
                    "month" if in_month => {
                        in_month = false;
                        pub_month = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                    }
                    "day" if in_day => {
                        in_day = false;
                        pub_day = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                    }
                    "pub-date" => {
                        if in_pub_date {
                            in_pub_date = false;
                            let date = build_date(&pub_year, &pub_month, &pub_day);
                            if !date.is_empty() {
                                store.insert_literal(
                                    &doc_iri,
                                    &format!("{DCTERMS}date"),
                                    &date,
                                    "string",
                                    g,
                                )?;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default();
                ctx.text_buf.push_str(&text);
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(format!("XML parse error: {err}").into());
            }
            _ => {}
        }
    }

    Ok(())
}

// =======================================================================
// USPTO Parser
// =======================================================================

/// Parse a USPTO patent XML document.
fn parse_uspto(
    content: &str,
    store: &dyn DocumentStore,
    doc_graph: &str,
    doc_hash: &str,
) -> ruddydoc_core::Result<()> {
    let mut ctx = ParseContext::new(store, doc_graph, doc_hash);
    let doc_iri = ruddydoc_core::doc_iri(doc_hash);
    let g = doc_graph;

    let mut reader = Reader::from_str(content);

    // State tracking
    let mut in_invention_title = false;
    let mut in_abstract = false;
    let mut in_description = false;
    let mut in_heading = false;
    let mut in_p = false;
    let mut in_claims = false;
    let mut in_claim = false;
    let mut in_claim_text = false;
    let mut in_table = false;
    let mut in_td = false;
    let mut in_th = false;

    // Track heading level by nesting depth within description
    let mut heading_level: u8 = 1;

    // Table state
    let mut table_iri: Option<String> = None;
    let mut table_row: usize = 0;
    let mut table_col: usize = 0;
    let mut table_max_col: usize = 0;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name_str(e);

                match local.as_str() {
                    "invention-title" => {
                        ctx.text_buf.clear();
                        in_invention_title = true;
                    }
                    "abstract" => {
                        in_abstract = true;
                        let iri = ctx.element_iri("group");
                        ctx.emit_element(&iri, ont::CLASS_GROUP)?;
                        ctx.parent_stack.push(iri);
                    }
                    "description" => {
                        in_description = true;
                        let iri = ctx.element_iri("group");
                        ctx.emit_element(&iri, ont::CLASS_GROUP)?;
                        ctx.parent_stack.push(iri);
                    }
                    "heading" => {
                        ctx.text_buf.clear();
                        in_heading = true;
                        // Extract level from attribute if present
                        if let Some(level_str) = get_attr(e, "level") {
                            heading_level = level_str.parse().unwrap_or(1);
                        }
                    }
                    "p" => {
                        ctx.text_buf.clear();
                        in_p = true;
                    }
                    "claims" => {
                        in_claims = true;
                        let iri = ctx.element_iri("group");
                        ctx.emit_element(&iri, ont::CLASS_GROUP)?;
                        ctx.parent_stack.push(iri);
                    }
                    "claim" => {
                        in_claim = true;
                        let iri = ctx.element_iri("group");
                        ctx.emit_element(&iri, ont::CLASS_GROUP)?;
                        ctx.parent_stack.push(iri);
                    }
                    "claim-text" => {
                        ctx.text_buf.clear();
                        in_claim_text = true;
                    }
                    "tables" | "table" => {
                        if !in_table {
                            in_table = true;
                            table_row = 0;
                            table_col = 0;
                            table_max_col = 0;
                            let iri = ctx.element_iri("table");
                            ctx.emit_element(&iri, ont::CLASS_TABLE_ELEMENT)?;
                            table_iri = Some(iri.clone());
                            ctx.parent_stack.push(iri);
                        }
                    }
                    "tr" if in_table => {
                        table_col = 0;
                    }
                    "td" if in_table => {
                        ctx.text_buf.clear();
                        in_td = true;
                    }
                    "th" if in_table => {
                        ctx.text_buf.clear();
                        in_th = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                match local.as_str() {
                    "invention-title" => {
                        if in_invention_title {
                            in_invention_title = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("title");
                                ctx.emit_element(&iri, ont::CLASS_TITLE)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }
                    }
                    "abstract" => {
                        if in_abstract {
                            in_abstract = false;
                            ctx.parent_stack.pop();
                        }
                    }
                    "description" => {
                        if in_description {
                            in_description = false;
                            ctx.parent_stack.pop();
                        }
                    }
                    "heading" => {
                        if in_heading {
                            in_heading = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("heading");
                                ctx.emit_element(&iri, ont::CLASS_SECTION_HEADER)?;
                                ctx.set_text_content(&iri, &text)?;
                                store.insert_literal(
                                    &iri,
                                    &ont::iri(ont::PROP_HEADING_LEVEL),
                                    &heading_level.to_string(),
                                    "integer",
                                    g,
                                )?;
                            }
                        }
                    }
                    "p" => {
                        if in_p {
                            in_p = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("paragraph");
                                ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }
                    }
                    "claims" => {
                        if in_claims {
                            in_claims = false;
                            ctx.parent_stack.pop();
                        }
                    }
                    "claim" => {
                        if in_claim {
                            in_claim = false;
                            ctx.parent_stack.pop();
                        }
                    }
                    "claim-text" => {
                        if in_claim_text {
                            in_claim_text = false;
                            let text = ctx.text_buf.trim().to_string();
                            ctx.text_buf.clear();
                            if !text.is_empty() {
                                let iri = ctx.element_iri("paragraph");
                                ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }
                    }
                    "tables" | "table" => {
                        if in_table {
                            in_table = false;
                            if let Some(t_iri) = table_iri.take() {
                                store.insert_literal(
                                    &t_iri,
                                    &ont::iri(ont::PROP_ROW_COUNT),
                                    &table_row.to_string(),
                                    "integer",
                                    g,
                                )?;
                                store.insert_literal(
                                    &t_iri,
                                    &ont::iri(ont::PROP_COLUMN_COUNT),
                                    &table_max_col.to_string(),
                                    "integer",
                                    g,
                                )?;
                            }
                            ctx.parent_stack.pop();
                        }
                    }
                    "tr" if in_table => {
                        table_row += 1;
                        if table_col > table_max_col {
                            table_max_col = table_col;
                        }
                    }
                    "td" if in_td => {
                        in_td = false;
                        let text = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                        emit_table_cell(&ctx, &table_iri, table_row, table_col, &text, false)?;
                        table_col += 1;
                    }
                    "th" if in_th => {
                        in_th = false;
                        let text = ctx.text_buf.trim().to_string();
                        ctx.text_buf.clear();
                        emit_table_cell(&ctx, &table_iri, table_row, table_col, &text, true)?;
                        table_col += 1;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default();
                ctx.text_buf.push_str(&text);
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(format!("XML parse error: {err}").into());
            }
            _ => {}
        }
    }

    // Store inventor/applicant metadata from the root element attributes
    // (USPTO often has these in <applicants>/<inventors>, but for a minimal
    // implementation we handle what was parsed above)

    // If we found a date in attributes, store it
    // USPTO dates are often in <us-patent-grant> or <publication-reference>
    // attributes. We do a second pass to extract doc-level metadata.
    extract_uspto_metadata(content, store, &doc_iri, g)?;

    Ok(())
}

/// Extract USPTO document-level metadata (inventor names, dates).
fn extract_uspto_metadata(
    content: &str,
    store: &dyn DocumentStore,
    doc_iri: &str,
    doc_graph: &str,
) -> ruddydoc_core::Result<()> {
    let mut reader = Reader::from_str(content);
    let mut in_inventor = false;
    let mut in_applicant = false;
    let mut in_last_name = false;
    let mut in_first_name = false;
    let mut in_date = false;
    let mut text_buf = String::new();
    let mut first_name = String::new();
    let mut last_name = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name_str(e);
                match local.as_str() {
                    "inventor" | "applicant" => {
                        in_inventor = local == "inventor";
                        in_applicant = local == "applicant";
                        first_name.clear();
                        last_name.clear();
                    }
                    "last-name" if in_inventor || in_applicant => {
                        text_buf.clear();
                        in_last_name = true;
                    }
                    "first-name" if in_inventor || in_applicant => {
                        text_buf.clear();
                        in_first_name = true;
                    }
                    "date" => {
                        text_buf.clear();
                        in_date = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local.as_str() {
                    "last-name" if in_last_name => {
                        in_last_name = false;
                        last_name = text_buf.trim().to_string();
                        text_buf.clear();
                    }
                    "first-name" if in_first_name => {
                        in_first_name = false;
                        first_name = text_buf.trim().to_string();
                        text_buf.clear();
                    }
                    "inventor" | "applicant" => {
                        if in_inventor || in_applicant {
                            let author = if !first_name.is_empty() {
                                format!("{} {}", first_name.trim(), last_name.trim())
                            } else {
                                last_name.trim().to_string()
                            };
                            if !author.is_empty() {
                                store.insert_literal(
                                    doc_iri,
                                    &format!("{DCTERMS}creator"),
                                    &author,
                                    "string",
                                    doc_graph,
                                )?;
                            }
                            in_inventor = false;
                            in_applicant = false;
                        }
                    }
                    "date" if in_date => {
                        in_date = false;
                        let date_text = text_buf.trim().to_string();
                        text_buf.clear();
                        if !date_text.is_empty() {
                            // USPTO dates are often YYYYMMDD format
                            let formatted = if date_text.len() == 8 {
                                format!(
                                    "{}-{}-{}",
                                    &date_text[..4],
                                    &date_text[4..6],
                                    &date_text[6..8]
                                )
                            } else {
                                date_text
                            };
                            store.insert_literal(
                                doc_iri,
                                &format!("{DCTERMS}date"),
                                &formatted,
                                "string",
                                doc_graph,
                            )?;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default();
                text_buf.push_str(&text);
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(())
}

// =======================================================================
// Generic XML Parser
// =======================================================================

/// Parse generic/unrecognized XML as a best-effort extraction.
fn parse_generic(
    content: &str,
    store: &dyn DocumentStore,
    doc_graph: &str,
    doc_hash: &str,
) -> ruddydoc_core::Result<()> {
    let mut ctx = ParseContext::new(store, doc_graph, doc_hash);

    let mut reader = Reader::from_str(content);
    let mut depth: usize = 0;
    let mut text_buf = String::new();
    let mut element_stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name_str(e);
                depth += 1;

                // Create a group for each non-leaf element
                let iri = ctx.element_iri("group");
                ctx.emit_element(&iri, ont::CLASS_GROUP)?;

                // Store the element name as metadata
                store.insert_literal(
                    &iri,
                    &ont::iri(ont::PROP_LABEL_ID),
                    &local,
                    "string",
                    doc_graph,
                )?;

                ctx.parent_stack.push(iri.clone());
                element_stack.push(local);
                text_buf.clear();
            }
            Ok(Event::End(_)) => {
                depth = depth.saturating_sub(1);
                let text = text_buf.trim().to_string();
                text_buf.clear();

                // If this element had text content, emit it as a Paragraph
                if !text.is_empty() {
                    let iri = ctx.element_iri("paragraph");
                    ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                    ctx.set_text_content(&iri, &text)?;
                }

                ctx.parent_stack.pop();
                element_stack.pop();
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default();
                text_buf.push_str(&text);
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(format!("XML parse error: {err}").into());
            }
            _ => {}
        }
    }

    Ok(())
}

// =======================================================================
// Shared helpers
// =======================================================================

/// Build a date string from year/month/day components.
fn build_date(year: &str, month: &str, day: &str) -> String {
    if year.is_empty() {
        return String::new();
    }
    let mut date = year.to_string();
    if !month.is_empty() {
        date.push('-');
        if month.len() == 1 {
            date.push('0');
        }
        date.push_str(month);
        if !day.is_empty() {
            date.push('-');
            if day.len() == 1 {
                date.push('0');
            }
            date.push_str(day);
        }
    }
    date
}

/// Emit a table cell into the store.
fn emit_table_cell(
    ctx: &ParseContext<'_>,
    table_iri: &Option<String>,
    row: usize,
    col: usize,
    text: &str,
    is_header: bool,
) -> ruddydoc_core::Result<()> {
    if let Some(t_iri) = table_iri {
        let cell_iri = ruddydoc_core::element_iri(ctx.doc_hash, &format!("cell-{row}-{col}"));
        let rdf_type = ont::rdf_iri("type");
        let g = ctx.doc_graph;

        ctx.store
            .insert_triple_into(&cell_iri, &rdf_type, &ont::iri(ont::CLASS_TABLE_CELL), g)?;
        ctx.store
            .insert_triple_into(t_iri, &ont::iri(ont::PROP_HAS_CELL), &cell_iri, g)?;
        ctx.store.insert_literal(
            &cell_iri,
            &ont::iri(ont::PROP_CELL_ROW),
            &row.to_string(),
            "integer",
            g,
        )?;
        ctx.store.insert_literal(
            &cell_iri,
            &ont::iri(ont::PROP_CELL_COLUMN),
            &col.to_string(),
            "integer",
            g,
        )?;
        ctx.store
            .insert_literal(&cell_iri, &ont::iri(ont::PROP_CELL_TEXT), text, "string", g)?;
        ctx.store.insert_literal(
            &cell_iri,
            &ont::iri(ont::PROP_IS_HEADER),
            if is_header { "true" } else { "false" },
            "boolean",
            g,
        )?;
    }
    Ok(())
}

// =======================================================================
// Tests
// =======================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;

    // -------------------------------------------------------------------
    // Test helpers
    // -------------------------------------------------------------------

    fn parse_xml(xml: &str) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = XmlBackend::new();
        let source = DocumentSource::Stream {
            name: "test.xml".to_string(),
            data: xml.as_bytes().to_vec(),
        };

        let hash_str = compute_hash(xml.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    // -------------------------------------------------------------------
    // XML type detection
    // -------------------------------------------------------------------

    #[test]
    fn detect_jats_by_article_element() {
        let xml = r#"<?xml version="1.0"?>
<article xmlns:xlink="http://www.w3.org/1999/xlink"
         xmlns:mml="http://www.w3.org/1998/Math/MathML"
         article-type="research-article"
         dtd-version="1.3"
         xmlns="http://jats.nlm.nih.gov/archiving/1.3/">
<front/><body/><back/>
</article>"#;
        assert_eq!(detect_xml_type(xml), XmlType::Jats);
    }

    #[test]
    fn detect_jats_by_doctype() {
        let xml = r#"<!DOCTYPE article PUBLIC "-//NLM//DTD JATS (Z39.96) Journal Archiving and Interchange DTD v1.3 20210610//EN" "JATS-archivearticle1-3.dtd">
<article><front/><body/></article>"#;
        assert_eq!(detect_xml_type(xml), XmlType::Jats);
    }

    #[test]
    fn detect_jats_bare_article() {
        let xml = r#"<?xml version="1.0"?><article><front/><body/></article>"#;
        assert_eq!(detect_xml_type(xml), XmlType::Jats);
    }

    #[test]
    fn detect_uspto_grant() {
        let xml = r#"<?xml version="1.0"?><us-patent-grant><us-bibliographic-data-grant/></us-patent-grant>"#;
        assert_eq!(detect_xml_type(xml), XmlType::Uspto);
    }

    #[test]
    fn detect_uspto_application() {
        let xml = r#"<?xml version="1.0"?><us-patent-application><us-bibliographic-data-application/></us-patent-application>"#;
        assert_eq!(detect_xml_type(xml), XmlType::Uspto);
    }

    #[test]
    fn detect_generic_xml() {
        let xml = r#"<?xml version="1.0"?><root><item>hello</item></root>"#;
        assert_eq!(detect_xml_type(xml), XmlType::Generic);
    }

    // -------------------------------------------------------------------
    // JATS parsing
    // -------------------------------------------------------------------

    const SAMPLE_JATS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<article xmlns="http://jats.nlm.nih.gov/archiving/1.3/"
         article-type="research-article">
  <front>
    <article-meta>
      <title-group>
        <article-title>A Study of Widgets</article-title>
      </title-group>
      <contrib-group>
        <contrib contrib-type="author">
          <name><surname>Smith</surname><given-names>Jane</given-names></name>
        </contrib>
        <contrib contrib-type="author">
          <name><surname>Doe</surname><given-names>John</given-names></name>
        </contrib>
      </contrib-group>
      <pub-date pub-type="epub">
        <day>15</day>
        <month>3</month>
        <year>2024</year>
      </pub-date>
    </article-meta>
  </front>
  <body>
    <sec>
      <title>Introduction</title>
      <p>Widgets are important in modern engineering.</p>
      <p>This paper explores their properties.</p>
    </sec>
    <sec>
      <title>Methods</title>
      <p>We tested 100 widgets using standard protocols.</p>
      <list list-type="bullet">
        <list-item><p>Widget type A</p></list-item>
        <list-item><p>Widget type B</p></list-item>
      </list>
      <table-wrap>
        <table>
          <thead>
            <tr><th>Type</th><th>Count</th></tr>
          </thead>
          <tbody>
            <tr><td>A</td><td>50</td></tr>
            <tr><td>B</td><td>50</td></tr>
          </tbody>
        </table>
      </table-wrap>
      <fig>
        <caption><p>Figure 1: Widget distribution</p></caption>
      </fig>
    </sec>
    <sec>
      <title>Results</title>
      <p>The results show <xref ref-type="bibr" rid="ref1">Smith 2020</xref>.</p>
      <disp-formula>E = mc^2</disp-formula>
      <code>print("hello")</code>
    </sec>
  </body>
  <back>
    <fn-group>
      <fn id="fn1"><p>This is a footnote.</p></fn>
    </fn-group>
  </back>
</article>"#;

    #[test]
    fn jats_title() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_TITLE),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("A Study of Widgets"));
        Ok(())
    }

    #[test]
    fn jats_authors() -> ruddydoc_core::Result<()> {
        let (store, meta, graph) = parse_xml(SAMPLE_JATS)?;
        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);

        let sparql = format!(
            "SELECT ?creator WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{DCTERMS}creator> ?creator \
               }} \
             }}",
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);

        let creators: Vec<&str> = rows
            .iter()
            .map(|r| r["creator"].as_str().expect("creator"))
            .collect();
        assert!(creators.iter().any(|c| c.contains("Jane Smith")));
        assert!(creators.iter().any(|c| c.contains("John Doe")));
        Ok(())
    }

    #[test]
    fn jats_date() -> ruddydoc_core::Result<()> {
        let (store, meta, graph) = parse_xml(SAMPLE_JATS)?;
        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);

        let sparql = format!(
            "SELECT ?date WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{DCTERMS}date> ?date \
               }} \
             }}",
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let date = rows[0]["date"].as_str().expect("date");
        assert!(date.contains("2024"));
        assert!(date.contains("03") || date.contains("3"));
        assert!(date.contains("15"));
        Ok(())
    }

    #[test]
    fn jats_sections_and_headings() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "SELECT ?text ?level WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text. \
                 ?h <{}> ?level \
               }} \
             }} ORDER BY ?level",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
            ont::iri(ont::PROP_HEADING_LEVEL),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 3); // Introduction, Methods, Results
        Ok(())
    }

    #[test]
    fn jats_paragraphs() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        // At least 3 body paragraphs plus the caption p, footnote p, and list item ps
        assert!(
            rows.len() >= 3,
            "expected at least 3 paragraphs, got {}",
            rows.len()
        );
        Ok(())
    }

    #[test]
    fn jats_unordered_list() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?l a <{}> }} }}",
            ont::iri(ont::CLASS_UNORDERED_LIST),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn jats_table() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        // Check table exists
        let sparql_table = format!(
            "ASK {{ GRAPH <{graph}> {{ ?t a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql_table)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check cells
        let sparql_cells = format!(
            "SELECT ?text ?row ?col ?isH WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> ?row. \
                 ?c <{}> ?col. \
                 ?c <{}> ?isH \
               }} \
             }} ORDER BY ?row ?col",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW),
            ont::iri(ont::PROP_CELL_COLUMN),
            ont::iri(ont::PROP_IS_HEADER),
        );
        let result = store.query_to_json(&sparql_cells)?;
        let rows = result.as_array().expect("expected array");
        // 2 header cells + 4 data cells = 6
        assert_eq!(rows.len(), 6);

        // First cell should be a header
        let first_is_header = rows[0]["isH"].as_str().expect("isH");
        assert!(first_is_header.contains("true"));
        Ok(())
    }

    #[test]
    fn jats_figure_and_caption() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?f a <{}> }} }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        let sparql_caption = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_CAPTION),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql_caption)?;
        let rows = result.as_array().expect("expected array");
        assert!(rows.len() >= 1);
        Ok(())
    }

    #[test]
    fn jats_formula() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?f a <{}>. \
                 ?f <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_FORMULA),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("E = mc^2"));
        Ok(())
    }

    #[test]
    fn jats_code() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_CODE),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("print"));
        Ok(())
    }

    #[test]
    fn jats_footnote() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?f a <{}>. \
                 ?f <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_FOOTNOTE),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("footnote"));
        Ok(())
    }

    #[test]
    fn jats_xref() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "SELECT ?key WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?r a <{}>. \
                 ?r <{}> ?key \
               }} \
             }}",
            ont::iri(ont::CLASS_REFERENCE),
            ont::iri(ont::PROP_CITATION_KEY),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let key = rows[0]["key"].as_str().expect("key");
        assert!(key.contains("ref1"));
        Ok(())
    }

    #[test]
    fn jats_document_metadata() -> ruddydoc_core::Result<()> {
        let (store, meta, graph) = parse_xml(SAMPLE_JATS)?;

        assert_eq!(meta.format, InputFormat::Xml);
        assert!(meta.page_count.is_none());

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?fmt WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?fmt \
               }} \
             }}",
            ont::iri(ont::PROP_SOURCE_FORMAT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        Ok(())
    }

    // -------------------------------------------------------------------
    // USPTO parsing
    // -------------------------------------------------------------------

    const SAMPLE_USPTO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<us-patent-grant lang="EN" dtd-version="v4.5"
                  file="US12345678-20240101.XML"
                  status="PRODUCTION"
                  date-produced="20240101"
                  date-publ="20240101">
  <us-bibliographic-data-grant>
    <publication-reference>
      <document-id>
        <country>US</country>
        <doc-number>12345678</doc-number>
        <date>20240101</date>
      </document-id>
    </publication-reference>
    <invention-title>Improved Widget Assembly</invention-title>
    <us-parties>
      <inventors>
        <inventor>
          <addressbook>
            <last-name>Johnson</last-name>
            <first-name>Alice</first-name>
          </addressbook>
        </inventor>
      </inventors>
    </us-parties>
  </us-bibliographic-data-grant>
  <abstract>
    <p>An improved widget assembly comprising a housing and a mechanism.</p>
  </abstract>
  <description>
    <heading level="1">FIELD OF THE INVENTION</heading>
    <p>The present invention relates to widgets.</p>
    <heading level="1">BACKGROUND</heading>
    <p>Prior widgets had many problems.</p>
    <table>
      <thead>
        <tr><th>Part</th><th>Material</th></tr>
      </thead>
      <tbody>
        <tr><td>Housing</td><td>Aluminum</td></tr>
        <tr><td>Gear</td><td>Steel</td></tr>
      </tbody>
    </table>
  </description>
  <claims>
    <claim id="CLM-001" num="1">
      <claim-text>A widget assembly comprising:
        <claim-text>a housing;</claim-text>
        <claim-text>a mechanism within the housing.</claim-text>
      </claim-text>
    </claim>
  </claims>
</us-patent-grant>"#;

    #[test]
    fn uspto_title() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_USPTO)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_TITLE),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Improved Widget Assembly"));
        Ok(())
    }

    #[test]
    fn uspto_inventor() -> ruddydoc_core::Result<()> {
        let (store, meta, graph) = parse_xml(SAMPLE_USPTO)?;
        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);

        let sparql = format!(
            "SELECT ?creator WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{DCTERMS}creator> ?creator \
               }} \
             }}",
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(rows.len() >= 1);
        let creators: Vec<&str> = rows
            .iter()
            .map(|r| r["creator"].as_str().expect("creator"))
            .collect();
        assert!(creators.iter().any(|c| c.contains("Alice Johnson")));
        Ok(())
    }

    #[test]
    fn uspto_date() -> ruddydoc_core::Result<()> {
        let (store, meta, graph) = parse_xml(SAMPLE_USPTO)?;
        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);

        let sparql = format!(
            "SELECT ?date WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{DCTERMS}date> ?date \
               }} \
             }}",
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(rows.len() >= 1);
        let date = rows[0]["date"].as_str().expect("date");
        assert!(date.contains("2024"));
        Ok(())
    }

    #[test]
    fn uspto_headings() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_USPTO)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2); // FIELD OF THE INVENTION, BACKGROUND
        Ok(())
    }

    #[test]
    fn uspto_paragraphs() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_USPTO)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        // abstract paragraph + 2 description paragraphs + 3 claim-text paragraphs
        assert!(
            rows.len() >= 3,
            "expected at least 3 paragraphs, got {}",
            rows.len()
        );
        Ok(())
    }

    #[test]
    fn uspto_claims_as_paragraphs() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_USPTO)?;

        // Claims should produce Group elements for grouping
        let sparql = format!(
            "SELECT ?g WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?g a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_GROUP),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        // abstract group, description group, claims group, claim group
        assert!(rows.len() >= 3);
        Ok(())
    }

    #[test]
    fn uspto_table() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_USPTO)?;

        // Check table exists
        let sparql_table = format!(
            "ASK {{ GRAPH <{graph}> {{ ?t a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql_table)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check cells
        let sparql_cells = format!(
            "SELECT ?text ?isH WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> ?isH \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_IS_HEADER),
        );
        let result = store.query_to_json(&sparql_cells)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 6); // 2 header + 4 data
        Ok(())
    }

    // -------------------------------------------------------------------
    // Generic XML parsing
    // -------------------------------------------------------------------

    #[test]
    fn generic_xml_extracts_text() -> ruddydoc_core::Result<()> {
        let xml =
            r#"<?xml version="1.0"?><root><item>Hello world</item><item>Goodbye</item></root>"#;
        let (store, _meta, graph) = parse_xml(xml)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);
        Ok(())
    }

    #[test]
    fn generic_xml_parent_child() -> ruddydoc_core::Result<()> {
        let xml = r#"<?xml version="1.0"?><root><parent><child>Text</child></parent></root>"#;
        let (store, _meta, graph) = parse_xml(xml)?;

        let sparql = format!(
            "SELECT ?child ?parent WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?child <{}> ?parent \
               }} \
             }}",
            ont::iri(ont::PROP_PARENT_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        // Multiple parent-child relationships expected in the hierarchy
        assert!(!rows.is_empty());
        Ok(())
    }

    #[test]
    fn generic_xml_preserves_element_names() -> ruddydoc_core::Result<()> {
        let xml = r#"<?xml version="1.0"?><root><custom-element>Data</custom-element></root>"#;
        let (store, _meta, graph) = parse_xml(xml)?;

        let sparql = format!(
            "SELECT ?label WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{}> ?label \
               }} \
             }}",
            ont::iri(ont::PROP_LABEL_ID),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        let labels: Vec<&str> = rows.iter().filter_map(|r| r["label"].as_str()).collect();
        assert!(labels.iter().any(|l| l.contains("custom-element")));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Edge cases
    // -------------------------------------------------------------------

    #[test]
    fn empty_xml_document() -> ruddydoc_core::Result<()> {
        let xml = r#"<?xml version="1.0"?><root/>"#;
        let (store, meta, graph) = parse_xml(xml)?;

        assert_eq!(meta.format, InputFormat::Xml);

        // Should have the document node
        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ <{doc_iri}> a <{}> }} }}",
            ont::iri(ont::CLASS_DOCUMENT),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_xml(SAMPLE_JATS)?;

        let sparql = format!(
            "SELECT ?el ?order WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{}> ?order \
               }} \
             }} ORDER BY ?order",
            ont::iri(ont::PROP_READING_ORDER),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        // Should have many elements in reading order
        assert!(rows.len() > 5, "expected many elements, got {}", rows.len());
        Ok(())
    }

    #[test]
    fn is_valid_checks_extension() {
        let backend = XmlBackend::new();

        let valid = DocumentSource::File(std::path::PathBuf::from("test.xml"));
        assert!(backend.is_valid(&valid));

        let invalid = DocumentSource::File(std::path::PathBuf::from("test.md"));
        assert!(!backend.is_valid(&invalid));

        let valid_stream = DocumentSource::Stream {
            name: "doc.xml".to_string(),
            data: vec![],
        };
        assert!(backend.is_valid(&valid_stream));

        let invalid_stream = DocumentSource::Stream {
            name: "doc.txt".to_string(),
            data: vec![],
        };
        assert!(!backend.is_valid(&invalid_stream));
    }

    #[test]
    fn supported_formats_returns_xml() {
        let backend = XmlBackend::new();
        assert_eq!(backend.supported_formats(), &[InputFormat::Xml]);
    }

    #[test]
    fn supports_pagination_returns_false() {
        let backend = XmlBackend::new();
        assert!(!backend.supports_pagination());
    }

    #[test]
    fn jats_abstract_is_group() -> ruddydoc_core::Result<()> {
        let xml = r#"<?xml version="1.0"?>
<article xmlns="http://jats.nlm.nih.gov/archiving/1.3/">
  <front>
    <article-meta>
      <abstract>
        <p>This is the abstract.</p>
      </abstract>
    </article-meta>
  </front>
  <body/>
</article>"#;
        let (store, _meta, graph) = parse_xml(xml)?;

        // Abstract should create a Group
        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?g a <{}> }} }}",
            ont::iri(ont::CLASS_GROUP),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // And the paragraph inside should exist
        let sparql_p = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql_p)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("abstract"));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Helper tests
    // -------------------------------------------------------------------

    #[test]
    fn build_date_full() {
        assert_eq!(build_date("2024", "3", "15"), "2024-03-15");
    }

    #[test]
    fn build_date_year_month() {
        assert_eq!(build_date("2024", "12", ""), "2024-12");
    }

    #[test]
    fn build_date_year_only() {
        assert_eq!(build_date("2024", "", ""), "2024");
    }

    #[test]
    fn build_date_empty() {
        assert_eq!(build_date("", "", ""), "");
    }

    #[test]
    fn build_date_padded_month() {
        assert_eq!(build_date("2024", "03", "01"), "2024-03-01");
    }
}
