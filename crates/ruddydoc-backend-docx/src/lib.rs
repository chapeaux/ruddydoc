//! DOCX parser backend for RuddyDoc.
//!
//! Parses OOXML (ZIP containing XML) documents using `zip` and `quick-xml`.
//! Extracts document structure (headings, paragraphs, lists, tables, images,
//! hyperlinks, footnotes, headers/footers) and maps them to the RuddyDoc
//! document ontology graph.

use std::collections::HashMap;
use std::io::Read as _;

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

// -----------------------------------------------------------------------
// DocxBackend
// -----------------------------------------------------------------------

/// DOCX document backend.
pub struct DocxBackend;

impl DocxBackend {
    /// Create a new DOCX backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for DocxBackend {
    fn default() -> Self {
        Self::new()
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

/// Shared state machine context for DOCX parsing.
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
// Style mapping
// -----------------------------------------------------------------------

/// Map a DOCX style name to a heading level (1-9), or None if not a heading.
///
/// Handles common variations: "Heading1", "Heading 1", "heading1", "heading 1".
/// Also handles "Title" as level 0.
fn heading_level_from_style(style_name: &str) -> Option<u8> {
    let lower = style_name.to_ascii_lowercase();

    if lower == "title" {
        return Some(0);
    }

    // Try "heading1" .. "heading9" (no space)
    if let Some(rest) = lower.strip_prefix("heading") {
        let trimmed = rest.trim();
        if let Ok(level) = trimmed.parse::<u8>()
            && (1..=9).contains(&level)
        {
            return Some(level);
        }
    }

    None
}

/// Determine whether a style name indicates a list paragraph.
fn is_list_style(style_name: &str) -> bool {
    let lower = style_name.to_ascii_lowercase();
    lower == "listparagraph" || lower == "list paragraph"
}

// -----------------------------------------------------------------------
// Style parsing
// -----------------------------------------------------------------------

/// Parse `word/styles.xml` to build a map of style ID to style name.
fn parse_styles(xml: &str) -> HashMap<String, String> {
    let mut styles = HashMap::new();
    let mut reader = Reader::from_str(xml);

    let mut current_style_id: Option<String> = None;
    let mut in_name = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = local_name_str(e);
                match local.as_str() {
                    "style" => {
                        current_style_id = get_attr(e, "styleId");
                    }
                    "name" if current_style_id.is_some() => {
                        if let Some(val) = get_attr(e, "val")
                            && let Some(id) = current_style_id.as_ref()
                        {
                            styles.insert(id.clone(), val);
                        }
                        // For Empty events, we already got what we need.
                        // For Start events, we might also get text children
                        // but OOXML <w:name> uses w:val attribute.
                        if matches!(reader.read_event(), Ok(Event::Start(_))) {
                            in_name = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local.as_str() {
                    "style" => {
                        current_style_id = None;
                    }
                    "name" => {
                        in_name = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    let _ = in_name; // suppress unused variable

    styles
}

// -----------------------------------------------------------------------
// Relationship parsing
// -----------------------------------------------------------------------

/// A relationship entry from a .rels file.
#[derive(Debug, Clone)]
struct Relationship {
    target: String,
    rel_type: String,
}

/// Parse a relationships XML file to build a map of relationship ID to target.
fn parse_relationships(xml: &str) -> HashMap<String, Relationship> {
    let mut rels = HashMap::new();
    let mut reader = Reader::from_str(xml);

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = local_name_str(e);
                if local == "Relationship" {
                    let id = get_attr(e, "Id").unwrap_or_default();
                    let target = get_attr(e, "Target").unwrap_or_default();
                    let rel_type = get_attr(e, "Type").unwrap_or_default();
                    if !id.is_empty() {
                        rels.insert(id, Relationship { target, rel_type });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    rels
}

// -----------------------------------------------------------------------
// Numbering parsing
// -----------------------------------------------------------------------

/// Numbering format types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumberFormat {
    /// Ordered (decimal, roman, letter, etc.)
    Ordered,
    /// Unordered (bullet, none)
    Unordered,
}

/// Parse `word/numbering.xml` to build a map of (numId, ilvl) to NumberFormat.
///
/// The numbering.xml has two key structures:
/// - `<w:abstractNum>` definitions with `<w:lvl>` children containing `<w:numFmt>`
/// - `<w:num>` entries mapping numId to abstractNumId
fn parse_numbering(xml: &str) -> HashMap<(String, String), NumberFormat> {
    let mut result = HashMap::new();

    // First pass: parse abstractNum definitions
    // abstractNumId -> { ilvl -> numFmt }
    let mut abstract_nums: HashMap<String, HashMap<String, NumberFormat>> = HashMap::new();

    // Second pass: parse num -> abstractNumId mappings
    let mut num_to_abstract: HashMap<String, String> = HashMap::new();

    let mut reader = Reader::from_str(xml);
    let mut current_abstract_id: Option<String> = None;
    let mut current_num_id: Option<String> = None;
    let mut current_lvl: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = local_name_str(e);
                match local.as_str() {
                    "abstractNum" => {
                        current_abstract_id = get_attr(e, "abstractNumId");
                    }
                    "num" => {
                        current_num_id = get_attr(e, "numId");
                    }
                    "abstractNumId" if current_num_id.is_some() => {
                        if let Some(val) = get_attr(e, "val")
                            && let Some(num_id) = current_num_id.as_ref()
                        {
                            num_to_abstract.insert(num_id.clone(), val);
                        }
                    }
                    "lvl" if current_abstract_id.is_some() => {
                        current_lvl = get_attr(e, "ilvl");
                    }
                    "numFmt" if current_abstract_id.is_some() && current_lvl.is_some() => {
                        if let Some(val) = get_attr(e, "val") {
                            let format = match val.as_str() {
                                "bullet" | "none" => NumberFormat::Unordered,
                                _ => NumberFormat::Ordered,
                            };
                            if let (Some(abs_id), Some(lvl)) =
                                (current_abstract_id.as_ref(), current_lvl.as_ref())
                            {
                                abstract_nums
                                    .entry(abs_id.clone())
                                    .or_default()
                                    .insert(lvl.clone(), format);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local.as_str() {
                    "abstractNum" => {
                        current_abstract_id = None;
                        current_lvl = None;
                    }
                    "num" => {
                        current_num_id = None;
                    }
                    "lvl" => {
                        current_lvl = None;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    // Build the final result: (numId, ilvl) -> NumberFormat
    for (num_id, abs_id) in &num_to_abstract {
        if let Some(levels) = abstract_nums.get(abs_id) {
            for (ilvl, fmt) in levels {
                result.insert((num_id.clone(), ilvl.clone()), *fmt);
            }
        }
    }

    result
}

// -----------------------------------------------------------------------
// Footnote parsing
// -----------------------------------------------------------------------

/// Parsed footnote: ID to text content.
fn parse_footnotes(xml: &str) -> HashMap<String, String> {
    let mut footnotes = HashMap::new();
    let mut reader = Reader::from_str(xml);

    let mut current_id: Option<String> = None;
    let mut text_buf = String::new();
    let mut in_footnote = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name_str(e);
                if local == "footnote" {
                    current_id = get_attr(e, "id");
                    text_buf.clear();
                    in_footnote = true;
                }
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if local == "footnote" && in_footnote {
                    if let Some(id) = current_id.take() {
                        let text = text_buf.trim().to_string();
                        if !text.is_empty() {
                            footnotes.insert(id, text);
                        }
                    }
                    in_footnote = false;
                    text_buf.clear();
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_footnote {
                    let text = e.unescape().unwrap_or_default();
                    text_buf.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    footnotes
}

// -----------------------------------------------------------------------
// Header/footer text extraction
// -----------------------------------------------------------------------

/// Extract all text from a header or footer XML file.
fn extract_header_footer_text(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut text_buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default();
                text_buf.push_str(&text);
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    text_buf.trim().to_string()
}

// -----------------------------------------------------------------------
// Document XML parsing
// -----------------------------------------------------------------------

/// Information extracted from paragraph properties.
#[derive(Default)]
struct ParagraphProps {
    /// Style ID (e.g., "Heading1", "ListParagraph").
    style_id: Option<String>,
    /// Outline level from <w:outlineLvl>.
    outline_level: Option<u8>,
    /// Numbering ID for lists.
    num_id: Option<String>,
    /// Indentation level for lists.
    ilvl: Option<String>,
}

/// State for tracking list context during parsing.
struct ListState {
    /// Current list element IRI, if we are inside a list.
    list_iri: Option<String>,
    /// The numId of the current active list.
    active_num_id: Option<String>,
    /// The ilvl of the current active list.
    active_ilvl: Option<String>,
}

/// Parse the main document body from `word/document.xml`.
#[allow(clippy::too_many_arguments)]
fn parse_document_body(
    document_xml: &str,
    styles: &HashMap<String, String>,
    relationships: &HashMap<String, Relationship>,
    numbering: &HashMap<(String, String), NumberFormat>,
    footnotes: &HashMap<String, String>,
    ctx: &mut ParseContext<'_>,
) -> ruddydoc_core::Result<()> {
    let mut reader = Reader::from_str(document_xml);

    // Paragraph-level state
    let mut in_body = false;
    let mut in_paragraph = false;
    let mut in_ppr = false;
    let mut text_buf = String::new();
    let mut para_props = ParagraphProps::default();

    // Table state
    let mut in_table = false;
    let mut table_iri: Option<String> = None;
    let mut table_row: usize = 0;
    let mut table_col: usize = 0;
    let mut table_max_col: usize = 0;
    let mut in_tr = false;
    let mut in_tc = false;
    let mut tc_text = String::new();
    // Merge tracking
    let mut tc_grid_span: usize = 1;
    let mut tc_vmerge_restart = false;
    let mut tc_vmerge_continue = false;
    // Track row spans: (col) -> rows remaining
    let mut vmerge_tracker: HashMap<usize, usize> = HashMap::new();

    // Image state
    let mut in_drawing = false;

    // Hyperlink state
    let mut in_hyperlink = false;
    let mut hyperlink_target = String::new();
    let mut hyperlink_text = String::new();

    // Footnote reference state
    let mut footnote_ref_id: Option<String> = None;

    // List tracking
    let mut list_state = ListState {
        list_iri: None,
        active_num_id: None,
        active_ilvl: None,
    };

    // Tag depth stack for nested handling
    let mut tag_stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = local_name_str(e);
                tag_stack.push(local.clone());

                match local.as_str() {
                    "body" => {
                        in_body = true;
                    }
                    "p" if in_body && !in_table => {
                        in_paragraph = true;
                        text_buf.clear();
                        para_props = ParagraphProps::default();
                    }
                    "p" if in_tc => {
                        // Paragraph inside a table cell -- collect text
                    }
                    "pPr" if in_paragraph => {
                        in_ppr = true;
                    }
                    "pStyle" if in_ppr => {
                        para_props.style_id = get_attr(e, "val");
                    }
                    "outlineLvl" if in_ppr => {
                        if let Some(val) = get_attr(e, "val") {
                            para_props.outline_level = val.parse::<u8>().ok();
                        }
                    }
                    "numPr" if in_ppr => {
                        // Numbering properties -- children will set numId and ilvl
                    }
                    "ilvl" if in_ppr => {
                        para_props.ilvl = get_attr(e, "val");
                    }
                    "numId" if in_ppr => {
                        para_props.num_id = get_attr(e, "val");
                    }
                    "tbl" if in_body => {
                        in_table = true;
                        table_row = 0;
                        table_col = 0;
                        table_max_col = 0;
                        vmerge_tracker.clear();
                        let iri = ctx.element_iri("table");
                        ctx.emit_element(&iri, ont::CLASS_TABLE_ELEMENT)?;
                        table_iri = Some(iri);
                    }
                    "tr" if in_table => {
                        in_tr = true;
                        table_col = 0;
                    }
                    "tc" if in_tr => {
                        in_tc = true;
                        tc_text.clear();
                        tc_grid_span = 1;
                        tc_vmerge_restart = false;
                        tc_vmerge_continue = false;
                    }
                    "gridSpan" if in_tc => {
                        if let Some(val) = get_attr(e, "val") {
                            tc_grid_span = val.parse::<usize>().unwrap_or(1);
                        }
                    }
                    "vMerge" if in_tc => {
                        match get_attr(e, "val") {
                            Some(v) if v == "restart" => {
                                tc_vmerge_restart = true;
                            }
                            _ => {
                                // No val or val != "restart" means continue
                                tc_vmerge_continue = true;
                            }
                        }
                    }
                    "drawing" if in_paragraph || in_tc => {
                        in_drawing = true;
                    }
                    "docPr" if in_drawing => {
                        // Extract alt text and image info
                        let alt = get_attr(e, "descr").unwrap_or_default();
                        let name = get_attr(e, "name").unwrap_or_default();

                        let alt_text = if !alt.is_empty() { alt } else { name };

                        let iri = ctx.element_iri("picture");
                        ctx.emit_element(&iri, ont::CLASS_PICTURE_ELEMENT)?;

                        if !alt_text.is_empty() {
                            ctx.store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_ALT_TEXT),
                                &alt_text,
                                "string",
                                ctx.doc_graph,
                            )?;
                        }
                    }
                    "blip" if in_drawing => {
                        // Image reference via relationship
                        if let Some(embed) = get_attr(e, "embed")
                            && let Some(rel) = relationships.get(&embed)
                            && let Some(pic_iri) = ctx.all_elements.last()
                        {
                            ctx.store.insert_literal(
                                pic_iri,
                                &ont::iri(ont::PROP_LINK_TARGET),
                                &rel.target,
                                "string",
                                ctx.doc_graph,
                            )?;
                        }
                    }
                    "hyperlink" if in_body => {
                        in_hyperlink = true;
                        hyperlink_text.clear();
                        hyperlink_target.clear();
                        // Hyperlinks can have r:id for external links or w:anchor
                        if let Some(rid) = get_attr(e, "id")
                            && let Some(rel) = relationships.get(&rid)
                        {
                            hyperlink_target = rel.target.clone();
                        }
                        if let Some(anchor) = get_attr(e, "anchor") {
                            hyperlink_target = format!("#{anchor}");
                        }
                    }
                    "footnoteReference" if in_paragraph => {
                        footnote_ref_id = get_attr(e, "id");
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = local_name_str(e);

                match local.as_str() {
                    "pStyle" if in_ppr => {
                        para_props.style_id = get_attr(e, "val");
                    }
                    "outlineLvl" if in_ppr => {
                        if let Some(val) = get_attr(e, "val") {
                            para_props.outline_level = val.parse::<u8>().ok();
                        }
                    }
                    "ilvl" if in_ppr => {
                        para_props.ilvl = get_attr(e, "val");
                    }
                    "numId" if in_ppr => {
                        para_props.num_id = get_attr(e, "val");
                    }
                    "gridSpan" if in_tc => {
                        if let Some(val) = get_attr(e, "val") {
                            tc_grid_span = val.parse::<usize>().unwrap_or(1);
                        }
                    }
                    "vMerge" if in_tc => match get_attr(e, "val") {
                        Some(v) if v == "restart" => {
                            tc_vmerge_restart = true;
                        }
                        _ => {
                            tc_vmerge_continue = true;
                        }
                    },
                    "docPr" if in_drawing => {
                        let alt = get_attr(e, "descr").unwrap_or_default();
                        let name = get_attr(e, "name").unwrap_or_default();

                        let alt_text = if !alt.is_empty() { alt } else { name };

                        let iri = ctx.element_iri("picture");
                        ctx.emit_element(&iri, ont::CLASS_PICTURE_ELEMENT)?;

                        if !alt_text.is_empty() {
                            ctx.store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_ALT_TEXT),
                                &alt_text,
                                "string",
                                ctx.doc_graph,
                            )?;
                        }
                    }
                    "blip" if in_drawing => {
                        if let Some(embed) = get_attr(e, "embed")
                            && let Some(rel) = relationships.get(&embed)
                            && let Some(pic_iri) = ctx.all_elements.last()
                        {
                            ctx.store.insert_literal(
                                pic_iri,
                                &ont::iri(ont::PROP_LINK_TARGET),
                                &rel.target,
                                "string",
                                ctx.doc_graph,
                            )?;
                        }
                    }
                    "footnoteReference" if in_paragraph => {
                        footnote_ref_id = get_attr(e, "id");
                    }
                    "br" if in_paragraph || in_tc => {
                        // Line break within a run
                        if in_tc {
                            tc_text.push('\n');
                        } else {
                            text_buf.push('\n');
                        }
                    }
                    "tab" if in_paragraph || in_tc => {
                        if in_tc {
                            tc_text.push('\t');
                        } else {
                            text_buf.push('\t');
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                tag_stack.pop();

                match local.as_str() {
                    "body" => {
                        in_body = false;
                        // Close any active list
                        close_list(&mut list_state, ctx);
                    }
                    "p" if in_paragraph && !in_table => {
                        in_paragraph = false;
                        in_ppr = false;

                        let text = text_buf.trim().to_string();
                        text_buf.clear();

                        // Resolve style name
                        let style_name = para_props
                            .style_id
                            .as_ref()
                            .and_then(|id| styles.get(id).cloned())
                            .or_else(|| para_props.style_id.clone())
                            .unwrap_or_default();

                        // Determine what kind of element this paragraph is
                        let heading_level =
                            heading_level_from_style(&style_name).or(para_props.outline_level);

                        if let Some(level) = heading_level {
                            // Close any active list before a heading
                            close_list(&mut list_state, ctx);

                            if !text.is_empty() {
                                if level == 0 {
                                    // Title
                                    let iri = ctx.element_iri("title");
                                    ctx.emit_element(&iri, ont::CLASS_TITLE)?;
                                    ctx.set_text_content(&iri, &text)?;
                                } else {
                                    let iri = ctx.element_iri("heading");
                                    ctx.emit_element(&iri, ont::CLASS_SECTION_HEADER)?;
                                    ctx.set_text_content(&iri, &text)?;
                                    ctx.store.insert_literal(
                                        &iri,
                                        &ont::iri(ont::PROP_HEADING_LEVEL),
                                        &level.to_string(),
                                        "integer",
                                        ctx.doc_graph,
                                    )?;
                                }
                            }
                        } else if is_list_style(&style_name) || para_props.num_id.is_some() {
                            // List item
                            let num_id = para_props.num_id.clone().unwrap_or_default();
                            let ilvl = para_props.ilvl.clone().unwrap_or_else(|| "0".to_string());

                            // Check if we need a new list or continue existing
                            let needs_new_list =
                                match (&list_state.active_num_id, &list_state.active_ilvl) {
                                    (Some(active_id), Some(active_lvl)) => {
                                        *active_id != num_id || *active_lvl != ilvl
                                    }
                                    _ => true,
                                };

                            if needs_new_list {
                                // Close previous list if any
                                close_list(&mut list_state, ctx);

                                // Determine list type from numbering
                                let list_format = numbering
                                    .get(&(num_id.clone(), ilvl.clone()))
                                    .copied()
                                    .unwrap_or(NumberFormat::Unordered);

                                let list_class = match list_format {
                                    NumberFormat::Ordered => ont::CLASS_ORDERED_LIST,
                                    NumberFormat::Unordered => ont::CLASS_UNORDERED_LIST,
                                };

                                let list_iri = ctx.element_iri("list");
                                ctx.emit_element(&list_iri, list_class)?;
                                ctx.parent_stack.push(list_iri.clone());

                                list_state.list_iri = Some(list_iri);
                                list_state.active_num_id = Some(num_id);
                                list_state.active_ilvl = Some(ilvl);
                            }

                            if !text.is_empty() {
                                let iri = ctx.element_iri("listitem");
                                ctx.emit_element(&iri, ont::CLASS_LIST_ITEM)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        } else {
                            // Close any active list before a normal paragraph
                            close_list(&mut list_state, ctx);

                            if !text.is_empty() {
                                let iri = ctx.element_iri("paragraph");
                                ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                                ctx.set_text_content(&iri, &text)?;
                            }
                        }

                        // Handle footnote reference if present
                        if let Some(fn_id) = footnote_ref_id.take()
                            && let Some(fn_text) = footnotes.get(&fn_id)
                        {
                            let iri = ctx.element_iri("footnote");
                            ctx.emit_element(&iri, ont::CLASS_FOOTNOTE)?;
                            ctx.set_text_content(&iri, fn_text)?;
                        }
                    }
                    "pPr" => {
                        in_ppr = false;
                    }
                    "tbl" if in_table => {
                        in_table = false;
                        if let Some(t_iri) = table_iri.take() {
                            ctx.store.insert_literal(
                                &t_iri,
                                &ont::iri(ont::PROP_ROW_COUNT),
                                &table_row.to_string(),
                                "integer",
                                ctx.doc_graph,
                            )?;
                            ctx.store.insert_literal(
                                &t_iri,
                                &ont::iri(ont::PROP_COLUMN_COUNT),
                                &table_max_col.to_string(),
                                "integer",
                                ctx.doc_graph,
                            )?;
                        }
                    }
                    "tr" if in_tr => {
                        in_tr = false;
                        table_row += 1;
                        if table_col > table_max_col {
                            table_max_col = table_col;
                        }
                    }
                    "tc" if in_tc => {
                        in_tc = false;

                        // Skip cells that are vertical merge continuations
                        if !tc_vmerge_continue {
                            let text = tc_text.trim().to_string();
                            let row_span = if tc_vmerge_restart {
                                // We will count the actual span later, but for now
                                // we start tracking. Use 1 as placeholder, then
                                // the vmerge tracking will update it.
                                1_usize
                            } else {
                                1
                            };

                            if tc_vmerge_restart {
                                // Record start of vertical merge for this column
                                vmerge_tracker.insert(table_col, 1);
                            }

                            emit_table_cell(
                                ctx,
                                &table_iri,
                                table_row,
                                table_col,
                                &text,
                                false,
                                row_span,
                                tc_grid_span,
                            )?;
                        } else {
                            // Increment the row span for the originating cell
                            if let Some(count) = vmerge_tracker.get_mut(&table_col) {
                                *count += 1;
                                // Update the row span on the original cell
                                let orig_row = table_row - *count + 1;
                                let cell_iri = ruddydoc_core::element_iri(
                                    ctx.doc_hash,
                                    &format!("cell-{orig_row}-{table_col}"),
                                );
                                // Overwrite the rowSpan literal
                                ctx.store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_CELL_ROW_SPAN),
                                    &count.to_string(),
                                    "integer",
                                    ctx.doc_graph,
                                )?;
                            }
                        }

                        table_col += tc_grid_span;
                        tc_text.clear();
                    }
                    "drawing" => {
                        in_drawing = false;
                    }
                    "hyperlink" if in_hyperlink => {
                        in_hyperlink = false;
                        let text = hyperlink_text.trim().to_string();

                        if !hyperlink_target.is_empty() || !text.is_empty() {
                            let iri = ctx.element_iri("link");
                            ctx.emit_element(&iri, ont::CLASS_HYPERLINK)?;

                            if !hyperlink_target.is_empty() {
                                ctx.store.insert_literal(
                                    &iri,
                                    &ont::iri(ont::PROP_LINK_TARGET),
                                    &hyperlink_target,
                                    "string",
                                    ctx.doc_graph,
                                )?;
                            }
                            if !text.is_empty() {
                                ctx.store.insert_literal(
                                    &iri,
                                    &ont::iri(ont::PROP_LINK_TEXT),
                                    &text,
                                    "string",
                                    ctx.doc_graph,
                                )?;
                            }
                        }

                        hyperlink_target.clear();
                        hyperlink_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default();
                // Only collect text if we are inside a <w:t> context.
                // The tag stack helps us determine this.
                let in_t = tag_stack.iter().any(|t| t == "t");
                if in_t {
                    if in_hyperlink {
                        hyperlink_text.push_str(&text);
                    }
                    if in_tc {
                        tc_text.push_str(&text);
                    } else if in_paragraph {
                        text_buf.push_str(&text);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(format!("DOCX XML parse error: {err}").into());
            }
            _ => {}
        }
    }

    Ok(())
}

/// Close the current active list, if any.
fn close_list(list_state: &mut ListState, ctx: &mut ParseContext<'_>) {
    if list_state.list_iri.is_some() {
        ctx.parent_stack.pop();
        list_state.list_iri = None;
        list_state.active_num_id = None;
        list_state.active_ilvl = None;
    }
}

/// Emit a table cell triple into the store.
#[allow(clippy::too_many_arguments)]
fn emit_table_cell(
    ctx: &mut ParseContext<'_>,
    table_iri: &Option<String>,
    row: usize,
    col: usize,
    text: &str,
    is_header: bool,
    row_span: usize,
    col_span: usize,
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
        ctx.store.insert_literal(
            &cell_iri,
            &ont::iri(ont::PROP_CELL_ROW_SPAN),
            &row_span.to_string(),
            "integer",
            g,
        )?;
        ctx.store.insert_literal(
            &cell_iri,
            &ont::iri(ont::PROP_CELL_COL_SPAN),
            &col_span.to_string(),
            "integer",
            g,
        )?;
    }
    Ok(())
}

// -----------------------------------------------------------------------
// ZIP reading helpers
// -----------------------------------------------------------------------

/// Read a file from a ZIP archive, returning its contents as a String.
/// Returns None if the file does not exist in the archive.
fn read_zip_text<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    path: &str,
) -> Option<String> {
    let mut file = archive.by_name(path).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    Some(buf)
}

// -----------------------------------------------------------------------
// DocumentBackend implementation
// -----------------------------------------------------------------------

impl DocumentBackend for DocxBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Docx]
    }

    fn supports_pagination(&self) -> bool {
        true
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(path.extension().and_then(|e| e.to_str()), Some("docx"))
            }
            DocumentSource::Stream { name, .. } => name.ends_with(".docx"),
        }
    }

    fn parse(
        &self,
        source: &DocumentSource,
        store: &dyn DocumentStore,
        doc_graph: &str,
    ) -> ruddydoc_core::Result<DocumentMeta> {
        // Read the source bytes
        let (data, file_path, file_name) = match source {
            DocumentSource::File(path) => {
                let data = std::fs::read(path)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".to_string());
                (data, Some(path.clone()), name)
            }
            DocumentSource::Stream { name, data } => (data.clone(), None, name.clone()),
        };

        let file_size = data.len() as u64;
        let hash_str = compute_hash(&data);
        let doc_hash = DocumentHash(hash_str.clone());

        // Create the document node
        let doc_iri = ruddydoc_core::doc_iri(&hash_str);
        let rdf_type = ont::rdf_iri("type");
        let g = doc_graph;

        store.insert_triple_into(&doc_iri, &rdf_type, &ont::iri(ont::CLASS_DOCUMENT), g)?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_SOURCE_FORMAT),
            "docx",
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

        // Open the ZIP archive
        let cursor = std::io::Cursor::new(&data);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| format!("Failed to open DOCX archive: {e}"))?;

        // Read document.xml (required)
        let document_xml = read_zip_text(&mut archive, "word/document.xml")
            .ok_or("DOCX archive missing word/document.xml")?;

        // Read optional files
        let styles_xml = read_zip_text(&mut archive, "word/styles.xml");
        let rels_xml = read_zip_text(&mut archive, "word/_rels/document.xml.rels");
        let numbering_xml = read_zip_text(&mut archive, "word/numbering.xml");
        let footnotes_xml = read_zip_text(&mut archive, "word/footnotes.xml");

        // Parse supporting files
        let styles = styles_xml.as_deref().map(parse_styles).unwrap_or_default();
        let relationships = rels_xml
            .as_deref()
            .map(parse_relationships)
            .unwrap_or_default();
        let numbering = numbering_xml
            .as_deref()
            .map(parse_numbering)
            .unwrap_or_default();
        let footnotes = footnotes_xml
            .as_deref()
            .map(parse_footnotes)
            .unwrap_or_default();

        // Parse the main document body
        let mut ctx = ParseContext::new(store, g, &hash_str);

        parse_document_body(
            &document_xml,
            &styles,
            &relationships,
            &numbering,
            &footnotes,
            &mut ctx,
        )?;

        // Parse headers and footers
        // Look for header*.xml and footer*.xml files in the rels
        for rel in relationships.values() {
            let target_lower = rel.target.to_ascii_lowercase();
            let rel_type_lower = rel.rel_type.to_ascii_lowercase();

            if target_lower.starts_with("header") || rel_type_lower.contains("header") {
                let path = format!("word/{}", rel.target);
                if let Some(xml) = read_zip_text(&mut archive, &path) {
                    let text = extract_header_footer_text(&xml);
                    if !text.is_empty() {
                        let iri = ctx.element_iri("pageheader");
                        ctx.emit_element(&iri, ont::CLASS_PAGE_HEADER)?;
                        ctx.set_text_content(&iri, &text)?;
                    }
                }
            } else if target_lower.starts_with("footer") || rel_type_lower.contains("footer") {
                let path = format!("word/{}", rel.target);
                if let Some(xml) = read_zip_text(&mut archive, &path) {
                    let text = extract_header_footer_text(&xml);
                    if !text.is_empty() {
                        let iri = ctx.element_iri("pagefooter");
                        ctx.emit_element(&iri, ont::CLASS_PAGE_FOOTER)?;
                        ctx.set_text_content(&iri, &text)?;
                    }
                }
            }
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Docx,
            file_size,
            page_count: None,
        })
    }
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;
    use std::io::Write as _;

    // -------------------------------------------------------------------
    // Test DOCX builder helper
    // -------------------------------------------------------------------

    /// Build a minimal DOCX (ZIP) archive in memory.
    struct DocxBuilder {
        files: Vec<(String, String)>,
    }

    impl DocxBuilder {
        fn new() -> Self {
            Self { files: Vec::new() }
        }

        fn add_file(&mut self, path: &str, content: &str) -> &mut Self {
            self.files.push((path.to_string(), content.to_string()));
            self
        }

        /// Add the minimal required files for a DOCX with the given body XML.
        fn with_body(&mut self, body_xml: &str) -> &mut Self {
            let document = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:wpc="http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas"
            xmlns:mo="http://schemas.microsoft.com/office/mac/office/2008/main"
            xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
            xmlns:mv="urn:schemas-microsoft-com:mac:vml"
            xmlns:o="urn:schemas-microsoft-com:office:office"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
            xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"
            xmlns:v="urn:schemas-microsoft-com:vml"
            xmlns:wp14="http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing"
            xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
            xmlns:w10="urn:schemas-microsoft-com:office:word"
            xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"
            xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup"
            xmlns:wpi="http://schemas.microsoft.com/office/word/2010/wordprocessingInk"
            xmlns:wne="http://schemas.microsoft.com/office/word/2006/wordml"
            xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
            xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
            xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
  <w:body>
{body_xml}
  </w:body>
</w:document>"#
            );

            self.add_file("word/document.xml", &document);

            // Content types
            let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;
            self.add_file("[Content_Types].xml", content_types);

            self
        }

        fn with_styles(&mut self, styles_xml: &str) -> &mut Self {
            self.add_file("word/styles.xml", styles_xml);
            self
        }

        fn with_rels(&mut self, rels_xml: &str) -> &mut Self {
            self.add_file("word/_rels/document.xml.rels", rels_xml);
            self
        }

        fn with_numbering(&mut self, numbering_xml: &str) -> &mut Self {
            self.add_file("word/numbering.xml", numbering_xml);
            self
        }

        fn with_footnotes(&mut self, footnotes_xml: &str) -> &mut Self {
            self.add_file("word/footnotes.xml", footnotes_xml);
            self
        }

        fn with_header(&mut self, index: usize, header_xml: &str) -> &mut Self {
            self.add_file(&format!("word/header{index}.xml"), header_xml);
            self
        }

        fn with_footer(&mut self, index: usize, footer_xml: &str) -> &mut Self {
            self.add_file(&format!("word/footer{index}.xml"), footer_xml);
            self
        }

        fn build(&self) -> Vec<u8> {
            let mut buf = Vec::new();
            {
                let cursor = std::io::Cursor::new(&mut buf);
                let mut zip = zip::ZipWriter::new(cursor);
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);

                for (path, content) in &self.files {
                    zip.start_file(path, options).unwrap();
                    zip.write_all(content.as_bytes()).unwrap();
                }
                zip.finish().unwrap();
            }
            buf
        }
    }

    // -------------------------------------------------------------------
    // Default styles XML for tests
    // -------------------------------------------------------------------

    fn default_styles() -> &'static str {
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Title">
    <w:name w:val="Title"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading2">
    <w:name w:val="heading 2"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading3">
    <w:name w:val="heading 3"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Normal">
    <w:name w:val="Normal"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="ListParagraph">
    <w:name w:val="List Paragraph"/>
  </w:style>
</w:styles>"#
    }

    // -------------------------------------------------------------------
    // Parse helper
    // -------------------------------------------------------------------

    fn parse_docx(data: &[u8]) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = DocxBackend::new();
        let source = DocumentSource::Stream {
            name: "test.docx".to_string(),
            data: data.to_vec(),
        };

        let hash_str = compute_hash(data);
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    // -------------------------------------------------------------------
    // Tests: Headings and paragraphs
    // -------------------------------------------------------------------

    #[test]
    fn parse_heading_and_paragraph() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
      <w:r><w:t>Hello World</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:t>This is a paragraph.</w:t></w:r>
    </w:p>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        // Check heading
        let sparql = format!(
            "SELECT ?text ?level WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text. \
                 ?h <{}> ?level \
               }} \
             }}",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
            ont::iri(ont::PROP_HEADING_LEVEL),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Hello World"));
        let level = rows[0]["level"].as_str().expect("level");
        assert!(level.contains('1'));

        // Check paragraph
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
        let result_p = store.query_to_json(&sparql_p)?;
        let rows_p = result_p.as_array().expect("expected array");
        assert_eq!(rows_p.len(), 1);
        let p_text = rows_p[0]["text"].as_str().expect("text");
        assert!(p_text.contains("This is a paragraph."));

        Ok(())
    }

    #[test]
    fn parse_multiple_heading_levels() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
      <w:r><w:t>H1</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr><w:pStyle w:val="Heading2"/></w:pPr>
      <w:r><w:t>H2</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr><w:pStyle w:val="Heading3"/></w:pPr>
      <w:r><w:t>H3</w:t></w:r>
    </w:p>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

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
        assert_eq!(rows.len(), 3);

        Ok(())
    }

    #[test]
    fn parse_title_style() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:pPr><w:pStyle w:val="Title"/></w:pPr>
      <w:r><w:t>Document Title</w:t></w:r>
    </w:p>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

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
        assert!(text.contains("Document Title"));

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: Text extraction from runs
    // -------------------------------------------------------------------

    #[test]
    fn text_extraction_concatenates_runs() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:r><w:t xml:space="preserve">Hello </w:t></w:r>
      <w:r><w:rPr><w:b/></w:rPr><w:t>world</w:t></w:r>
      <w:r><w:t xml:space="preserve"> again</w:t></w:r>
    </w:p>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

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
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Hello world again"));

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: Lists
    // -------------------------------------------------------------------

    #[test]
    fn parse_list_with_numbering() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:pPr>
        <w:pStyle w:val="ListParagraph"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>
      </w:pPr>
      <w:r><w:t>Item one</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr>
        <w:pStyle w:val="ListParagraph"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>
      </w:pPr>
      <w:r><w:t>Item two</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr>
        <w:pStyle w:val="ListParagraph"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>
      </w:pPr>
      <w:r><w:t>Item three</w:t></w:r>
    </w:p>"#;

        let numbering = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:numFmt w:val="bullet"/>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1">
    <w:abstractNumId w:val="0"/>
  </w:num>
</w:numbering>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .with_numbering(numbering)
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        // Check that we have an UnorderedList
        let sparql_list = format!(
            "ASK {{ GRAPH <{graph}> {{ ?l a <{}> }} }}",
            ont::iri(ont::CLASS_UNORDERED_LIST),
        );
        let result = store.query_to_json(&sparql_list)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check list items
        let sparql_items = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?li a <{}>. \
                 ?li <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_LIST_ITEM),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql_items)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 3);

        Ok(())
    }

    #[test]
    fn parse_ordered_list() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:pPr>
        <w:pStyle w:val="ListParagraph"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>
      </w:pPr>
      <w:r><w:t>First</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr>
        <w:pStyle w:val="ListParagraph"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>
      </w:pPr>
      <w:r><w:t>Second</w:t></w:r>
    </w:p>"#;

        let numbering = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:numFmt w:val="decimal"/>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1">
    <w:abstractNumId w:val="0"/>
  </w:num>
</w:numbering>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .with_numbering(numbering)
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?l a <{}> }} }}",
            ont::iri(ont::CLASS_ORDERED_LIST),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        Ok(())
    }

    #[test]
    fn list_items_have_parent() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:pPr>
        <w:pStyle w:val="ListParagraph"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>
      </w:pPr>
      <w:r><w:t>Item A</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr>
        <w:pStyle w:val="ListParagraph"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>
      </w:pPr>
      <w:r><w:t>Item B</w:t></w:r>
    </w:p>"#;

        let numbering = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
</w:numbering>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .with_numbering(numbering)
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        let sparql = format!(
            "SELECT ?item ?parent WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?item a <{}>. \
                 ?item <{}> ?parent. \
                 ?parent a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_LIST_ITEM),
            ont::iri(ont::PROP_PARENT_ELEMENT),
            ont::iri(ont::CLASS_UNORDERED_LIST),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: Tables
    // -------------------------------------------------------------------

    #[test]
    fn parse_table() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>Age</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Alice</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>30</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Bob</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>25</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        // Check table exists
        let sparql_table = format!(
            "ASK {{ GRAPH <{graph}> {{ ?t a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql_table)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check cells
        let sparql_cells = format!(
            "SELECT ?text ?row ?col WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> ?row. \
                 ?c <{}> ?col \
               }} \
             }} ORDER BY ?row ?col",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW),
            ont::iri(ont::PROP_CELL_COLUMN),
        );
        let result = store.query_to_json(&sparql_cells)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 6);

        // Check row count
        let sparql_rc = format!(
            "SELECT ?rc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?rc \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_ROW_COUNT),
        );
        let result_rc = store.query_to_json(&sparql_rc)?;
        let rows_rc = result_rc.as_array().expect("expected array");
        assert_eq!(rows_rc.len(), 1);
        let rc = rows_rc[0]["rc"].as_str().expect("rc");
        assert!(rc.contains('3'));

        Ok(())
    }

    #[test]
    fn parse_table_with_col_span() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:tbl>
      <w:tr>
        <w:tc>
          <w:tcPr><w:gridSpan w:val="2"/></w:tcPr>
          <w:p><w:r><w:t>Full Width</w:t></w:r></w:p>
        </w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Left</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>Right</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        // Check that colspan=2 was recorded
        let sparql = format!(
            "SELECT ?text ?cs WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> ?cs \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_COL_SPAN),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");

        let colspan_cell = rows
            .iter()
            .find(|r| r["text"].as_str().is_some_and(|t| t.contains("Full Width")))
            .expect("expected Full Width cell");
        let cs = colspan_cell["cs"].as_str().expect("cs");
        assert!(cs.contains('2'));

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: Hyperlinks
    // -------------------------------------------------------------------

    #[test]
    fn parse_hyperlink() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:hyperlink r:id="rId1">
        <w:r><w:t>Example Link</w:t></w:r>
      </w:hyperlink>
    </w:p>"#;

        let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com" TargetMode="External"/>
</Relationships>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .with_rels(rels)
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        let sparql = format!(
            "SELECT ?target ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?target. \
                 ?h <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_HYPERLINK),
            ont::iri(ont::PROP_LINK_TARGET),
            ont::iri(ont::PROP_LINK_TEXT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let target = rows[0]["target"].as_str().expect("target");
        assert!(target.contains("https://example.com"));

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Example Link"));

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: Images / Drawings
    // -------------------------------------------------------------------

    #[test]
    fn parse_image() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:r>
        <w:drawing>
          <wp:inline>
            <wp:docPr id="1" name="Picture 1" descr="A test image"/>
            <a:graphic>
              <a:graphicData>
                <pic:pic>
                  <pic:blipFill>
                    <a:blip r:embed="rId2"/>
                  </pic:blipFill>
                </pic:pic>
              </a:graphicData>
            </a:graphic>
          </wp:inline>
        </w:drawing>
      </w:r>
    </w:p>"#;

        let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
</Relationships>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .with_rels(rels)
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        let sparql = format!(
            "SELECT ?alt ?target WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?alt. \
                 ?p <{}> ?target \
               }} \
             }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
            ont::iri(ont::PROP_ALT_TEXT),
            ont::iri(ont::PROP_LINK_TARGET),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let alt = rows[0]["alt"].as_str().expect("alt");
        assert!(alt.contains("A test image"));

        let target = rows[0]["target"].as_str().expect("target");
        assert!(target.contains("media/image1.png"));

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: Footnotes
    // -------------------------------------------------------------------

    #[test]
    fn parse_footnote_reference() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:r><w:t>Some text with a footnote</w:t></w:r>
      <w:r><w:footnoteReference w:id="1"/></w:r>
    </w:p>"#;

        let footnotes = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:id="0" w:type="separator"/>
  <w:footnote w:id="1">
    <w:p><w:r><w:t>This is the footnote text.</w:t></w:r></w:p>
  </w:footnote>
</w:footnotes>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .with_footnotes(footnotes)
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

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
        assert!(text.contains("This is the footnote text."));

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: Headers and footers
    // -------------------------------------------------------------------

    #[test]
    fn parse_header_and_footer() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:r><w:t>Body content</w:t></w:r>
    </w:p>"#;

        let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>
  <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/>
</Relationships>"#;

        let header_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:p><w:r><w:t>Page Header Text</w:t></w:r></w:p>
</w:hdr>"#;

        let footer_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:p><w:r><w:t>Page Footer Text</w:t></w:r></w:p>
</w:ftr>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .with_rels(rels)
            .with_header(1, header_xml)
            .with_footer(1, footer_xml)
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        // Check header
        let sparql_h = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_PAGE_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result_h = store.query_to_json(&sparql_h)?;
        let rows_h = result_h.as_array().expect("expected array");
        assert_eq!(rows_h.len(), 1);
        let header_text = rows_h[0]["text"].as_str().expect("text");
        assert!(header_text.contains("Page Header Text"));

        // Check footer
        let sparql_f = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?f a <{}>. \
                 ?f <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_PAGE_FOOTER),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result_f = store.query_to_json(&sparql_f)?;
        let rows_f = result_f.as_array().expect("expected array");
        assert_eq!(rows_f.len(), 1);
        let footer_text = rows_f[0]["text"].as_str().expect("text");
        assert!(footer_text.contains("Page Footer Text"));

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: Document metadata
    // -------------------------------------------------------------------

    #[test]
    fn document_metadata() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:r><w:t>Test</w:t></w:r>
    </w:p>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, meta, graph) = parse_docx(&data)?;

        assert_eq!(meta.format, InputFormat::Docx);

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
        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("docx"));

        Ok(())
    }

    #[test]
    fn document_has_file_name() -> ruddydoc_core::Result<()> {
        let body = r#"<w:p><w:r><w:t>Test</w:t></w:r></w:p>"#;
        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, meta, graph) = parse_docx(&data)?;
        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);

        let sparql = format!(
            "SELECT ?name WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?name \
               }} \
             }}",
            ont::iri(ont::PROP_FILE_NAME),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let name = rows[0]["name"].as_str().expect("name");
        assert!(name.contains("test.docx"));

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: is_valid
    // -------------------------------------------------------------------

    #[test]
    fn is_valid_accepts_docx_extension() {
        let backend = DocxBackend::new();

        assert!(backend.is_valid(&DocumentSource::File("test.docx".into())));
        assert!(!backend.is_valid(&DocumentSource::File("test.doc".into())));
        assert!(!backend.is_valid(&DocumentSource::File("test.md".into())));
        assert!(!backend.is_valid(&DocumentSource::File("test.html".into())));

        assert!(backend.is_valid(&DocumentSource::Stream {
            name: "test.docx".to_string(),
            data: vec![],
        }));
        assert!(!backend.is_valid(&DocumentSource::Stream {
            name: "test.doc".to_string(),
            data: vec![],
        }));
    }

    // -------------------------------------------------------------------
    // Tests: Error handling
    // -------------------------------------------------------------------

    #[test]
    fn invalid_zip_returns_error() {
        let store = OxigraphStore::new().unwrap();
        let backend = DocxBackend::new();
        let source = DocumentSource::Stream {
            name: "bad.docx".to_string(),
            data: b"this is not a zip file".to_vec(),
        };

        let result = backend.parse(&source, &store, "urn:test:graph");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("DOCX archive") || err_msg.contains("zip"),
            "error should mention ZIP: {err_msg}"
        );
    }

    #[test]
    fn missing_document_xml_returns_error() {
        let store = OxigraphStore::new().unwrap();
        let backend = DocxBackend::new();

        // Build a ZIP with no word/document.xml
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("dummy.xml", options).unwrap();
            zip.write_all(b"<root/>").unwrap();
            zip.finish().unwrap();
        }

        let source = DocumentSource::Stream {
            name: "missing.docx".to_string(),
            data: buf,
        };

        let result = backend.parse(&source, &store, "urn:test:graph");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("document.xml"),
            "error should mention document.xml: {err_msg}"
        );
    }

    // -------------------------------------------------------------------
    // Tests: Reading order
    // -------------------------------------------------------------------

    #[test]
    fn reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
      <w:r><w:t>Heading</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:t>Para one.</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:t>Para two.</w:t></w:r>
    </w:p>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

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
        assert_eq!(rows.len(), 3); // heading + 2 paragraphs

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: supported_formats and supports_pagination
    // -------------------------------------------------------------------

    #[test]
    fn supported_formats_returns_docx() {
        let backend = DocxBackend::new();
        assert_eq!(backend.supported_formats(), &[InputFormat::Docx]);
    }

    #[test]
    fn supports_pagination_returns_true() {
        let backend = DocxBackend::new();
        assert!(backend.supports_pagination());
    }

    #[test]
    fn default_impl_works() {
        let backend = DocxBackend::default();
        assert_eq!(backend.supported_formats(), &[InputFormat::Docx]);
    }

    // -------------------------------------------------------------------
    // Tests: Style mapping helpers
    // -------------------------------------------------------------------

    #[test]
    fn heading_level_from_various_styles() {
        assert_eq!(heading_level_from_style("Heading1"), Some(1));
        assert_eq!(heading_level_from_style("Heading 1"), Some(1));
        assert_eq!(heading_level_from_style("heading1"), Some(1));
        assert_eq!(heading_level_from_style("heading 2"), Some(2));
        assert_eq!(heading_level_from_style("Heading3"), Some(3));
        assert_eq!(heading_level_from_style("Heading9"), Some(9));
        assert_eq!(heading_level_from_style("Title"), Some(0));
        assert_eq!(heading_level_from_style("Normal"), None);
        assert_eq!(heading_level_from_style("ListParagraph"), None);
    }

    #[test]
    fn list_style_detection() {
        assert!(is_list_style("ListParagraph"));
        assert!(is_list_style("List Paragraph"));
        assert!(is_list_style("listparagraph"));
        assert!(!is_list_style("Normal"));
        assert!(!is_list_style("Heading1"));
    }

    // -------------------------------------------------------------------
    // Tests: Hash is deterministic
    // -------------------------------------------------------------------

    #[test]
    fn hash_is_deterministic() {
        let data = b"hello world";
        let h1 = compute_hash(data);
        let h2 = compute_hash(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex is 64 chars
    }

    // -------------------------------------------------------------------
    // Tests: Heading from outlineLvl
    // -------------------------------------------------------------------

    #[test]
    fn heading_from_outline_level() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:pPr><w:outlineLvl w:val="2"/></w:pPr>
      <w:r><w:t>Outline Heading</w:t></w:r>
    </w:p>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .build();

        let (store, _meta, graph) = parse_docx(&data)?;

        let sparql = format!(
            "SELECT ?text ?level WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text. \
                 ?h <{}> ?level \
               }} \
             }}",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
            ont::iri(ont::PROP_HEADING_LEVEL),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Outline Heading"));

        let level = rows[0]["level"].as_str().expect("level");
        assert!(level.contains('2'));

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tests: Parse styles helper
    // -------------------------------------------------------------------

    #[test]
    fn parse_styles_extracts_ids_and_names() {
        let xml = r#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Normal">
    <w:name w:val="Normal"/>
  </w:style>
</w:styles>"#;

        let styles = parse_styles(xml);
        assert_eq!(styles.get("Heading1"), Some(&"heading 1".to_string()));
        assert_eq!(styles.get("Normal"), Some(&"Normal".to_string()));
    }

    // -------------------------------------------------------------------
    // Tests: Parse relationships helper
    // -------------------------------------------------------------------

    #[test]
    fn parse_relationships_extracts_entries() {
        let xml = r#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://some/type" Target="target1.xml"/>
  <Relationship Id="rId2" Type="http://some/image" Target="media/image1.png"/>
</Relationships>"#;

        let rels = parse_relationships(xml);
        assert_eq!(rels.len(), 2);
        assert_eq!(rels.get("rId1").unwrap().target, "target1.xml");
        assert_eq!(rels.get("rId2").unwrap().target, "media/image1.png");
    }

    // -------------------------------------------------------------------
    // Tests: Parse numbering helper
    // -------------------------------------------------------------------

    #[test]
    fn parse_numbering_extracts_format() {
        let xml = r#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
    <w:lvl w:ilvl="1"><w:numFmt w:val="decimal"/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
</w:numbering>"#;

        let numbering = parse_numbering(xml);
        assert_eq!(
            numbering.get(&("1".to_string(), "0".to_string())),
            Some(&NumberFormat::Unordered)
        );
        assert_eq!(
            numbering.get(&("1".to_string(), "1".to_string())),
            Some(&NumberFormat::Ordered)
        );
    }

    // -------------------------------------------------------------------
    // Tests: Parse footnotes helper
    // -------------------------------------------------------------------

    #[test]
    fn parse_footnotes_extracts_text() {
        let xml = r#"<?xml version="1.0"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:id="0" w:type="separator"/>
  <w:footnote w:id="1">
    <w:p><w:r><w:t>Footnote content</w:t></w:r></w:p>
  </w:footnote>
</w:footnotes>"#;

        let footnotes = parse_footnotes(xml);
        assert_eq!(footnotes.get("1"), Some(&"Footnote content".to_string()));
        // Separator footnotes typically have no text
        assert!(footnotes.get("0").is_none());
    }

    // -------------------------------------------------------------------
    // Tests: Complex document
    // -------------------------------------------------------------------

    #[test]
    fn parse_complex_document() -> ruddydoc_core::Result<()> {
        let body = r#"
    <w:p>
      <w:pPr><w:pStyle w:val="Title"/></w:pPr>
      <w:r><w:t>Complex Document</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
      <w:r><w:t>Introduction</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:t>This is the introduction.</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr><w:pStyle w:val="Heading2"/></w:pPr>
      <w:r><w:t>Details</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr>
        <w:pStyle w:val="ListParagraph"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>
      </w:pPr>
      <w:r><w:t>Item A</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr>
        <w:pStyle w:val="ListParagraph"/>
        <w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>
      </w:pPr>
      <w:r><w:t>Item B</w:t></w:r>
    </w:p>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Col 1</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>Col 2</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Data 1</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>Data 2</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
    <w:p>
      <w:r><w:t>Conclusion paragraph.</w:t></w:r>
    </w:p>"#;

        let numbering = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
</w:numbering>"#;

        let data = DocxBuilder::new()
            .with_body(body)
            .with_styles(default_styles())
            .with_numbering(numbering)
            .build();

        let (store, meta, graph) = parse_docx(&data)?;

        assert_eq!(meta.format, InputFormat::Docx);

        // Count total elements with reading order
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

        // Title + h1 + paragraph + h2 + list + 2 items + table + conclusion paragraph = 9
        assert!(
            rows.len() >= 8,
            "expected at least 8 elements, got {}",
            rows.len()
        );

        // Verify reading order values are sequential and start at 0
        for (i, row) in rows.iter().enumerate() {
            let order = row["order"].as_str().expect("order");
            assert!(
                order.contains(&i.to_string()),
                "expected reading order {i} in {order}"
            );
        }

        Ok(())
    }
}
