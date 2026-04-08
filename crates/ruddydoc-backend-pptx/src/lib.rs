//! PPTX parser backend for RuddyDoc.
//!
//! Parses OOXML presentation files (`.pptx`) using `zip` and `quick-xml`.
//! A PPTX file is a ZIP archive containing XML files that describe the
//! presentation structure, including slides, speaker notes, and media.

use std::collections::HashMap;
use std::io::{Cursor, Read as _};

use quick_xml::Reader;
use quick_xml::events::Event;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

/// PPTX document backend.
pub struct PptxBackend;

impl PptxBackend {
    /// Create a new PPTX backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PptxBackend {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Hex encoding / SHA-256 hashing (matching HTML backend pattern)
// ---------------------------------------------------------------------------

/// Compute a SHA-256 hash of the content bytes, returning a hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode(result)
}

/// Hex-encode bytes.
fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

// ---------------------------------------------------------------------------
// Parse context (matching HTML backend pattern)
// ---------------------------------------------------------------------------

/// State machine context for PPTX parsing.
struct ParseContext<'a> {
    store: &'a dyn DocumentStore,
    doc_graph: &'a str,
    doc_hash: &'a str,
    /// Sequential reading order counter.
    reading_order: usize,
    /// All element IRIs in order (for final document linking).
    all_elements: Vec<String>,
}

impl<'a> ParseContext<'a> {
    fn new(store: &'a dyn DocumentStore, doc_graph: &'a str, doc_hash: &'a str) -> Self {
        Self {
            store,
            doc_graph,
            doc_hash,
            reading_order: 0,
            all_elements: Vec::new(),
        }
    }

    /// Generate a unique element IRI.
    fn element_iri(&self, kind: &str) -> String {
        ruddydoc_core::element_iri(self.doc_hash, &format!("{kind}-{}", self.reading_order))
    }

    /// Insert an element into the graph with its type, reading order, and
    /// document association.
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

    /// Link an element to a page via onPage.
    fn set_on_page(&self, element_iri: &str, page_iri: &str) -> ruddydoc_core::Result<()> {
        self.store.insert_triple_into(
            element_iri,
            &ont::iri(ont::PROP_ON_PAGE),
            page_iri,
            self.doc_graph,
        )
    }
}

// ---------------------------------------------------------------------------
// PPTX slide ordering
// ---------------------------------------------------------------------------

/// A relationship entry from a .rels file.
struct Relationship {
    id: String,
    target: String,
}

/// Parse a relationships (.rels) XML file, returning a map from rId -> target path.
fn parse_rels(xml_bytes: &[u8]) -> ruddydoc_core::Result<HashMap<String, String>> {
    let mut reader = Reader::from_reader(xml_bytes);
    let mut buf = Vec::new();
    let mut rels = HashMap::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e))
                if e.local_name().as_ref() == b"Relationship" =>
            {
                let rel = parse_relationship_attrs(e)?;
                rels.insert(rel.id, rel.target);
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error in .rels: {e}").into()),
            _ => {}
        }
        buf.clear();
    }

    Ok(rels)
}

/// Extract id and target attributes from a Relationship element.
fn parse_relationship_attrs(
    e: &quick_xml::events::BytesStart<'_>,
) -> ruddydoc_core::Result<Relationship> {
    let mut id = String::new();
    let mut target = String::new();

    for attr in e.attributes() {
        let attr = attr.map_err(|e| format!("attribute error: {e}"))?;
        let key = attr.key.local_name();
        let val = attr
            .unescape_value()
            .map_err(|e| format!("unescape error: {e}"))?;
        match key.as_ref() {
            b"Id" => id = val.into_owned(),
            b"Target" => target = val.into_owned(),
            _ => {}
        }
    }

    if id.is_empty() {
        return Err("Relationship element missing Id attribute".into());
    }

    Ok(Relationship { id, target })
}

/// Determine the ordered list of slide file paths from the presentation.
///
/// Reads `ppt/presentation.xml` for `<p:sldId>` elements (which give the
/// ordered `r:id` references) and `ppt/_rels/presentation.xml.rels` to
/// resolve each `r:id` to a file path.
fn determine_slide_order<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> ruddydoc_core::Result<Vec<String>> {
    // Read presentation.xml to get ordered rId references
    let ordered_rids = parse_slide_id_list(archive)?;

    // Read presentation.xml.rels to map rIds to slide file paths
    let rels = read_presentation_rels(archive)?;

    // Resolve each rId to a slide path
    let mut slide_paths = Vec::new();
    for rid in &ordered_rids {
        if let Some(target) = rels.get(rid) {
            // target is relative to ppt/, e.g. "slides/slide1.xml"
            let full_path = if target.starts_with('/') {
                target.trim_start_matches('/').to_string()
            } else {
                format!("ppt/{target}")
            };
            slide_paths.push(full_path);
        }
    }

    Ok(slide_paths)
}

/// Parse `ppt/presentation.xml` to extract the ordered slide rId list.
fn parse_slide_id_list<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> ruddydoc_core::Result<Vec<String>> {
    let xml = read_zip_entry(archive, "ppt/presentation.xml")?;
    let mut reader = Reader::from_reader(xml.as_bytes());
    let mut buf = Vec::new();
    let mut rids = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e))
                if e.local_name().as_ref() == b"sldId" =>
            {
                // Look for the r:id attribute (may be namespaced as r:id)
                for attr in e.attributes() {
                    let attr = attr.map_err(|e| format!("attribute error: {e}"))?;
                    let key = attr.key.local_name();
                    if key.as_ref() == b"id" {
                        let prefix = attr.key.prefix();
                        if prefix.is_some_and(|p| p.as_ref() == b"r") {
                            let val = attr
                                .unescape_value()
                                .map_err(|e| format!("unescape: {e}"))?;
                            rids.push(val.into_owned());
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error in presentation.xml: {e}").into()),
            _ => {}
        }
        buf.clear();
    }

    Ok(rids)
}

/// Read the presentation relationships file.
fn read_presentation_rels<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> ruddydoc_core::Result<HashMap<String, String>> {
    let xml = read_zip_entry(archive, "ppt/_rels/presentation.xml.rels")?;
    parse_rels(xml.as_bytes())
}

/// Read a file from the ZIP archive, returning its content as a string.
fn read_zip_entry<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> ruddydoc_core::Result<String> {
    let mut file = archive
        .by_name(path)
        .map_err(|e| format!("cannot read {path} from ZIP: {e}"))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

/// Try to read a file from the ZIP archive, returning None if it doesn't exist.
fn try_read_zip_entry<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> ruddydoc_core::Result<Option<String>> {
    match archive.by_name(path) {
        Ok(mut file) => {
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            Ok(Some(content))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(format!("error reading {path}: {e}").into()),
    }
}

// ---------------------------------------------------------------------------
// Text extraction helpers
// ---------------------------------------------------------------------------

/// Extract all text from `<a:t>` elements within an XML fragment.
///
/// This concatenates all text runs within all paragraphs, returning each
/// paragraph's text separately. This is the fundamental text extraction
/// routine used for shapes, table cells, and notes.
fn extract_paragraphs_from_xml(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current_para = String::new();
    let mut depth: u32 = 0;
    let mut in_para = false;
    let mut para_depth: u32 = 0;
    let mut in_run = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name = e.local_name();
                match name.as_ref() {
                    b"p" if !in_para => {
                        in_para = true;
                        para_depth = depth;
                        current_para.clear();
                    }
                    b"r" if in_para => {
                        in_run = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_para
                    && in_run
                    && let Ok(text) = e.unescape()
                {
                    current_para.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.local_name();
                if in_para {
                    if name.as_ref() == b"r" {
                        in_run = false;
                    }
                    if depth == para_depth && name.as_ref() == b"p" {
                        in_para = false;
                        let trimmed = current_para.trim().to_string();
                        if !trimmed.is_empty() {
                            paragraphs.push(trimmed);
                        }
                        current_para.clear();
                    }
                }
                depth -= 1;
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    paragraphs
}

/// Extract all text from an XML fragment as a single string (paragraphs
/// joined with newlines).
fn extract_text_from_xml(xml: &str) -> String {
    extract_paragraphs_from_xml(xml).join("\n")
}

// ---------------------------------------------------------------------------
// Slide XML parsing
// ---------------------------------------------------------------------------

/// The type of a placeholder shape.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PlaceholderType {
    Title,
    CenterTitle,
    SubTitle,
    Body,
    Other(String),
}

/// A shape extracted from a slide.
#[derive(Debug)]
enum SlideShape {
    TextShape {
        placeholder: Option<PlaceholderType>,
        paragraphs: Vec<String>,
    },
    Table {
        rows: Vec<Vec<TableCellData>>,
    },
    Picture {
        alt_text: String,
    },
}

/// Data for a single table cell.
#[derive(Debug, Clone)]
struct TableCellData {
    text: String,
    grid_span: usize,
    row_span: usize,
}

/// Parse a single slide XML and extract its shapes.
///
/// Uses a two-level approach: first finds top-level shapes (sp, pic, tbl)
/// within the shape tree using depth tracking, then parses each shape's
/// content independently.
fn parse_slide_xml(xml: &str) -> ruddydoc_core::Result<Vec<SlideShape>> {
    // We need to find the top-level shapes within <p:spTree>.
    // Strategy: walk the XML, track depth, and when we find a shape element
    // at the right level, collect all events until it closes, then parse
    // the collected fragment.

    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut shapes = Vec::new();

    // Track whether we're in the shape tree
    let mut in_sp_tree = false;
    let mut sp_tree_depth: u32 = 0;

    // When collecting a shape, we track its depth and accumulate raw XML
    let mut collecting: Option<ShapeCollector> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name();

                if name.as_ref() == b"spTree" && !in_sp_tree {
                    in_sp_tree = true;
                    sp_tree_depth = 1;
                } else if in_sp_tree {
                    sp_tree_depth += 1;

                    if let Some(ref mut collector) = collecting {
                        collector.depth += 1;
                        collector.push_start(e);
                    } else {
                        // Check if this is a shape we want to collect
                        match name.as_ref() {
                            b"sp" | b"pic" | b"tbl" => {
                                let mut collector = ShapeCollector::new(name.as_ref());
                                collector.push_start(e);
                                collecting = Some(collector);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                if in_sp_tree && let Some(ref mut collector) = collecting {
                    collector.push_empty(e);
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Some(ref mut collector) = collecting
                    && let Ok(text) = e.unescape()
                {
                    collector.push_text(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                if in_sp_tree {
                    let closed_shape = if let Some(ref mut collector) = collecting {
                        collector.depth -= 1;
                        if collector.depth == 0 {
                            collector.push_end(e);
                            true
                        } else {
                            collector.push_end(e);
                            false
                        }
                    } else {
                        false
                    };

                    if closed_shape {
                        let collector = collecting.take().expect("checked above");
                        let shape = parse_collected_shape(&collector)?;
                        if let Some(s) = shape {
                            shapes.push(s);
                        }
                    }

                    sp_tree_depth -= 1;
                    if sp_tree_depth == 0 {
                        in_sp_tree = false;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error in slide: {e}").into()),
            _ => {}
        }
        buf.clear();
    }

    Ok(shapes)
}

/// Collects raw XML content for a shape element.
struct ShapeCollector {
    /// The shape type: b"sp", b"pic", or b"tbl"
    shape_type: Vec<u8>,
    /// Nesting depth (starts at 1 when the shape opens)
    depth: u32,
    /// Accumulated XML content
    xml: String,
}

impl ShapeCollector {
    fn new(shape_type: &[u8]) -> Self {
        Self {
            shape_type: shape_type.to_vec(),
            depth: 1,
            xml: String::new(),
        }
    }

    fn push_start(&mut self, e: &quick_xml::events::BytesStart<'_>) {
        self.xml.push('<');
        self.xml
            .push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or("unknown"));
        for attr in e.attributes().flatten() {
            self.xml.push(' ');
            self.xml
                .push_str(std::str::from_utf8(attr.key.as_ref()).unwrap_or(""));
            self.xml.push_str("=\"");
            if let Ok(val) = attr.unescape_value() {
                // Escape XML special chars in attribute values
                for c in val.chars() {
                    match c {
                        '"' => self.xml.push_str("&quot;"),
                        '&' => self.xml.push_str("&amp;"),
                        '<' => self.xml.push_str("&lt;"),
                        '>' => self.xml.push_str("&gt;"),
                        _ => self.xml.push(c),
                    }
                }
            }
            self.xml.push('"');
        }
        self.xml.push('>');
    }

    fn push_empty(&mut self, e: &quick_xml::events::BytesStart<'_>) {
        self.xml.push('<');
        self.xml
            .push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or("unknown"));
        for attr in e.attributes().flatten() {
            self.xml.push(' ');
            self.xml
                .push_str(std::str::from_utf8(attr.key.as_ref()).unwrap_or(""));
            self.xml.push_str("=\"");
            if let Ok(val) = attr.unescape_value() {
                for c in val.chars() {
                    match c {
                        '"' => self.xml.push_str("&quot;"),
                        '&' => self.xml.push_str("&amp;"),
                        '<' => self.xml.push_str("&lt;"),
                        '>' => self.xml.push_str("&gt;"),
                        _ => self.xml.push(c),
                    }
                }
            }
            self.xml.push('"');
        }
        self.xml.push_str("/>");
    }

    fn push_text(&mut self, text: &str) {
        for c in text.chars() {
            match c {
                '&' => self.xml.push_str("&amp;"),
                '<' => self.xml.push_str("&lt;"),
                '>' => self.xml.push_str("&gt;"),
                _ => self.xml.push(c),
            }
        }
    }

    fn push_end(&mut self, e: &quick_xml::events::BytesEnd<'_>) {
        self.xml.push_str("</");
        self.xml
            .push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or("unknown"));
        self.xml.push('>');
    }
}

/// Parse a collected shape into a `SlideShape`.
fn parse_collected_shape(collector: &ShapeCollector) -> ruddydoc_core::Result<Option<SlideShape>> {
    match collector.shape_type.as_slice() {
        b"sp" => parse_text_shape(&collector.xml),
        b"pic" => parse_picture_shape(&collector.xml),
        b"tbl" => parse_table_shape(&collector.xml),
        _ => Ok(None),
    }
}

/// Parse a `<p:sp>` shape element.
fn parse_text_shape(xml: &str) -> ruddydoc_core::Result<Option<SlideShape>> {
    let placeholder = detect_placeholder(xml);
    let paragraphs = extract_paragraphs_from_xml(xml);

    if paragraphs.is_empty() {
        return Ok(None);
    }

    Ok(Some(SlideShape::TextShape {
        placeholder,
        paragraphs,
    }))
}

/// Detect the placeholder type from a shape's XML.
fn detect_placeholder(xml: &str) -> Option<PlaceholderType> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) if e.local_name().as_ref() == b"ph" => {
                return Some(extract_ph_type(e));
            }
            Ok(Event::Start(ref e)) if e.local_name().as_ref() == b"ph" => {
                return Some(extract_ph_type(e));
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    None
}

/// Extract the placeholder type from a `<p:ph>` element.
fn extract_ph_type(e: &quick_xml::events::BytesStart<'_>) -> PlaceholderType {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == b"type"
            && let Ok(val) = attr.unescape_value()
        {
            return match val.as_ref() {
                "title" => PlaceholderType::Title,
                "ctrTitle" => PlaceholderType::CenterTitle,
                "subTitle" => PlaceholderType::SubTitle,
                "body" => PlaceholderType::Body,
                other => PlaceholderType::Other(other.to_string()),
            };
        }
    }
    // No type attribute means default placeholder (often body)
    PlaceholderType::Body
}

/// Parse a `<p:pic>` picture element.
fn parse_picture_shape(xml: &str) -> ruddydoc_core::Result<Option<SlideShape>> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut alt_text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e))
                if e.local_name().as_ref() == b"cNvPr" =>
            {
                for attr in e.attributes().flatten() {
                    if attr.key.local_name().as_ref() == b"descr"
                        && let Ok(val) = attr.unescape_value()
                    {
                        alt_text = val.into_owned();
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(Some(SlideShape::Picture { alt_text }))
}

/// Parse a `<a:tbl>` table element.
fn parse_table_shape(xml: &str) -> ruddydoc_core::Result<Option<SlideShape>> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut rows: Vec<Vec<TableCellData>> = Vec::new();

    // State
    let mut in_tr = false;
    let mut tr_depth: u32 = 0;
    let mut in_tc = false;
    let mut tc_depth: u32 = 0;
    let mut current_row: Vec<TableCellData> = Vec::new();
    let mut cell_xml = String::new();
    let mut cell_grid_span: usize = 1;
    let mut cell_row_span: usize = 1;
    let mut depth: u32 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name = e.local_name();

                if in_tc {
                    tc_depth += 1;
                    // Accumulate inner XML for text extraction
                    cell_xml.push('<');
                    cell_xml.push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or(""));
                    cell_xml.push('>');
                } else if in_tr {
                    if name.as_ref() == b"tc" {
                        in_tc = true;
                        tc_depth = 1;
                        cell_xml.clear();
                        cell_grid_span = 1;
                        cell_row_span = 1;

                        // Check for gridSpan and rowSpan attributes
                        for attr in e.attributes().flatten() {
                            let key = attr.key.local_name();
                            if let Ok(val) = attr.unescape_value() {
                                match key.as_ref() {
                                    b"gridSpan" => {
                                        cell_grid_span = val.parse::<usize>().unwrap_or(1);
                                    }
                                    b"rowSpan" => {
                                        cell_row_span = val.parse::<usize>().unwrap_or(1);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                } else if name.as_ref() == b"tr" {
                    in_tr = true;
                    tr_depth = depth;
                    current_row.clear();
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_tc && let Ok(text) = e.unescape() {
                    // Escape for later re-parsing
                    for c in text.chars() {
                        match c {
                            '&' => cell_xml.push_str("&amp;"),
                            '<' => cell_xml.push_str("&lt;"),
                            '>' => cell_xml.push_str("&gt;"),
                            _ => cell_xml.push(c),
                        }
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                if in_tc {
                    cell_xml.push('<');
                    cell_xml.push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or(""));
                    cell_xml.push_str("/>");
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.local_name();

                if in_tc {
                    tc_depth -= 1;
                    if tc_depth == 0 && name.as_ref() == b"tc" {
                        in_tc = false;
                        // Extract text from the accumulated cell XML
                        let text = extract_text_from_xml(&cell_xml);
                        current_row.push(TableCellData {
                            text,
                            grid_span: cell_grid_span,
                            row_span: cell_row_span,
                        });
                    } else {
                        cell_xml.push_str("</");
                        cell_xml.push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or(""));
                        cell_xml.push('>');
                    }
                } else if in_tr && depth == tr_depth && name.as_ref() == b"tr" {
                    in_tr = false;
                    rows.push(std::mem::take(&mut current_row));
                }

                depth -= 1;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error in table: {e}").into()),
            _ => {}
        }
        buf.clear();
    }

    Ok(Some(SlideShape::Table { rows }))
}

/// Extract text content from `<a:t>` elements within a notes XML body.
///
/// Notes slides have multiple `<p:sp>` shapes. The first is typically
/// the slide image placeholder; the second contains the actual notes.
fn parse_notes_xml(xml: &str) -> ruddydoc_core::Result<String> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut sp_count = 0u32;
    let mut in_sp = false;
    let mut sp_depth: u32 = 0;
    let mut depth: u32 = 0;
    let mut second_sp_xml = String::new();

    // Collect the XML of the second <p:sp> (the notes body)
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name = e.local_name();

                if name.as_ref() == b"sp" && !in_sp {
                    sp_count += 1;
                    in_sp = true;
                    sp_depth = depth;

                    if sp_count >= 2 {
                        second_sp_xml.push('<');
                        second_sp_xml
                            .push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or(""));
                        second_sp_xml.push('>');
                    }
                } else if in_sp && sp_count >= 2 {
                    second_sp_xml.push('<');
                    second_sp_xml.push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or(""));
                    second_sp_xml.push('>');
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_sp
                    && sp_count >= 2
                    && let Ok(text) = e.unescape()
                {
                    for c in text.chars() {
                        match c {
                            '&' => second_sp_xml.push_str("&amp;"),
                            '<' => second_sp_xml.push_str("&lt;"),
                            '>' => second_sp_xml.push_str("&gt;"),
                            _ => second_sp_xml.push(c),
                        }
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                if in_sp && sp_count >= 2 {
                    second_sp_xml.push('<');
                    second_sp_xml.push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or(""));
                    second_sp_xml.push_str("/>");
                }
            }
            Ok(Event::End(ref e)) => {
                if in_sp && sp_count >= 2 {
                    second_sp_xml.push_str("</");
                    second_sp_xml.push_str(std::str::from_utf8(e.name().as_ref()).unwrap_or(""));
                    second_sp_xml.push('>');
                }
                if in_sp && depth == sp_depth {
                    in_sp = false;
                }
                depth -= 1;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error in notes: {e}").into()),
            _ => {}
        }
        buf.clear();
    }

    if second_sp_xml.is_empty() {
        return Ok(String::new());
    }

    Ok(extract_text_from_xml(&second_sp_xml))
}

/// Discover slide files by scanning the ZIP entries when presentation.xml
/// ordering is not available. Falls back to sorting by filename.
fn discover_slides_from_entries<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Vec<String> {
    let mut slides: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name().to_string();
            if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                slides.push(name);
            }
        }
    }
    slides.sort();
    slides
}

// ---------------------------------------------------------------------------
// Backend trait implementation
// ---------------------------------------------------------------------------

impl DocumentBackend for PptxBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Pptx]
    }

    fn supports_pagination(&self) -> bool {
        true
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(path.extension().and_then(|e| e.to_str()), Some("pptx"))
            }
            DocumentSource::Stream { name, .. } => name.ends_with(".pptx"),
        }
    }

    fn parse(
        &self,
        source: &DocumentSource,
        store: &dyn DocumentStore,
        doc_graph: &str,
    ) -> ruddydoc_core::Result<DocumentMeta> {
        // Read the raw bytes
        let (data, file_path, file_name) = match source {
            DocumentSource::File(path) => {
                let data = std::fs::read(path)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown.pptx".to_string());
                (data, Some(path.clone()), name)
            }
            DocumentSource::Stream { name, data } => (data.clone(), None, name.clone()),
        };

        let file_size = data.len() as u64;
        let hash_str = compute_hash(&data);
        let doc_hash = DocumentHash(hash_str.clone());

        // Open as ZIP archive
        let cursor = Cursor::new(&data);
        let mut archive =
            ZipArchive::new(cursor).map_err(|e| format!("invalid ZIP/PPTX file: {e}"))?;

        // Determine slide order
        let slide_paths = determine_slide_order(&mut archive).unwrap_or_else(|_| {
            // Fallback: discover slides from ZIP entries
            discover_slides_from_entries(&mut archive)
        });

        let page_count = slide_paths.len() as u32;

        // Create the document node
        let doc_iri = ruddydoc_core::doc_iri(&hash_str);
        let rdf_type = ont::rdf_iri("type");
        let g = doc_graph;

        store.insert_triple_into(&doc_iri, &rdf_type, &ont::iri(ont::CLASS_DOCUMENT), g)?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_SOURCE_FORMAT),
            "pptx",
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
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_PAGE_COUNT),
            &page_count.to_string(),
            "integer",
            g,
        )?;

        let mut ctx = ParseContext::new(store, g, &hash_str);

        // Process each slide
        for (slide_index, slide_path) in slide_paths.iter().enumerate() {
            let page_number = slide_index + 1;

            // Create a Page node
            let page_iri = ruddydoc_core::element_iri(&hash_str, &format!("page-{page_number}"));
            store.insert_triple_into(&page_iri, &rdf_type, &ont::iri(ont::CLASS_PAGE), g)?;
            store.insert_literal(
                &page_iri,
                &ont::iri(ont::PROP_PAGE_NUMBER),
                &page_number.to_string(),
                "integer",
                g,
            )?;
            store.insert_triple_into(&doc_iri, &ont::iri(ont::PROP_HAS_PAGE), &page_iri, g)?;

            // Read the slide XML
            let slide_xml = match read_zip_entry(&mut archive, slide_path) {
                Ok(xml) => xml,
                Err(_) => continue,
            };

            // Parse shapes from the slide
            let shapes = parse_slide_xml(&slide_xml)?;

            // Emit RDF for each shape
            for shape in &shapes {
                match shape {
                    SlideShape::TextShape {
                        placeholder,
                        paragraphs,
                    } => {
                        let is_title = matches!(
                            placeholder,
                            Some(PlaceholderType::Title) | Some(PlaceholderType::CenterTitle)
                        );
                        let is_subtitle = matches!(placeholder, Some(PlaceholderType::SubTitle));

                        if is_title {
                            // Emit as SectionHeader with headingLevel=1
                            let text = paragraphs.join("\n");
                            if !text.is_empty() {
                                let iri = ctx.element_iri("title");
                                ctx.emit_element(&iri, ont::CLASS_SECTION_HEADER)?;
                                ctx.set_text_content(&iri, &text)?;
                                ctx.set_on_page(&iri, &page_iri)?;
                                store.insert_literal(
                                    &iri,
                                    &ont::iri(ont::PROP_HEADING_LEVEL),
                                    "1",
                                    "integer",
                                    g,
                                )?;
                            }
                        } else if is_subtitle {
                            // Emit subtitle paragraphs as Paragraph elements
                            for para in paragraphs {
                                let iri = ctx.element_iri("paragraph");
                                ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                                ctx.set_text_content(&iri, para)?;
                                ctx.set_on_page(&iri, &page_iri)?;
                            }
                        } else {
                            // Regular body text
                            for para in paragraphs {
                                let iri = ctx.element_iri("paragraph");
                                ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                                ctx.set_text_content(&iri, para)?;
                                ctx.set_on_page(&iri, &page_iri)?;
                            }
                        }
                    }
                    SlideShape::Table { rows } => {
                        let table_iri = ctx.element_iri("table");
                        ctx.emit_element(&table_iri, ont::CLASS_TABLE_ELEMENT)?;
                        ctx.set_on_page(&table_iri, &page_iri)?;

                        let row_count = rows.len();
                        let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);

                        store.insert_literal(
                            &table_iri,
                            &ont::iri(ont::PROP_ROW_COUNT),
                            &row_count.to_string(),
                            "integer",
                            g,
                        )?;
                        store.insert_literal(
                            &table_iri,
                            &ont::iri(ont::PROP_COLUMN_COUNT),
                            &col_count.to_string(),
                            "integer",
                            g,
                        )?;

                        for (row_idx, row) in rows.iter().enumerate() {
                            for (col_idx, cell) in row.iter().enumerate() {
                                let cell_iri = ruddydoc_core::element_iri(
                                    &hash_str,
                                    &format!("slide{page_number}-cell-{row_idx}-{col_idx}"),
                                );
                                store.insert_triple_into(
                                    &cell_iri,
                                    &rdf_type,
                                    &ont::iri(ont::CLASS_TABLE_CELL),
                                    g,
                                )?;
                                store.insert_triple_into(
                                    &table_iri,
                                    &ont::iri(ont::PROP_HAS_CELL),
                                    &cell_iri,
                                    g,
                                )?;
                                store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_CELL_ROW),
                                    &row_idx.to_string(),
                                    "integer",
                                    g,
                                )?;
                                store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_CELL_COLUMN),
                                    &col_idx.to_string(),
                                    "integer",
                                    g,
                                )?;
                                store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_CELL_TEXT),
                                    &cell.text,
                                    "string",
                                    g,
                                )?;
                                // First row is treated as header
                                let is_header = row_idx == 0;
                                store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_IS_HEADER),
                                    if is_header { "true" } else { "false" },
                                    "boolean",
                                    g,
                                )?;
                                store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_CELL_ROW_SPAN),
                                    &cell.row_span.to_string(),
                                    "integer",
                                    g,
                                )?;
                                store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_CELL_COL_SPAN),
                                    &cell.grid_span.to_string(),
                                    "integer",
                                    g,
                                )?;
                            }
                        }
                    }
                    SlideShape::Picture { alt_text } => {
                        let iri = ctx.element_iri("picture");
                        ctx.emit_element(&iri, ont::CLASS_PICTURE_ELEMENT)?;
                        ctx.set_on_page(&iri, &page_iri)?;
                        if !alt_text.is_empty() {
                            store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_ALT_TEXT),
                                alt_text,
                                "string",
                                g,
                            )?;
                        }
                    }
                }
            }

            // Try to read speaker notes
            let notes_path = format!("ppt/notesSlides/notesSlide{page_number}.xml");
            if let Ok(Some(notes_xml)) = try_read_zip_entry(&mut archive, &notes_path) {
                let notes_text = parse_notes_xml(&notes_xml)?;
                if !notes_text.is_empty() {
                    let notes_iri = ctx.element_iri("footnote");
                    ctx.emit_element(&notes_iri, ont::CLASS_FOOTNOTE)?;
                    ctx.set_text_content(&notes_iri, &notes_text)?;
                    ctx.set_on_page(&notes_iri, &page_iri)?;
                }
            }
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Pptx,
            file_size,
            page_count: Some(page_count),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;

    // -----------------------------------------------------------------------
    // Helper: build a minimal PPTX in memory
    // -----------------------------------------------------------------------

    struct PptxBuilder {
        slides: Vec<String>,
        notes: Vec<Option<String>>,
    }

    impl PptxBuilder {
        fn new() -> Self {
            Self {
                slides: Vec::new(),
                notes: Vec::new(),
            }
        }

        fn add_slide(mut self, xml_body: &str) -> Self {
            self.slides.push(xml_body.to_string());
            self.notes.push(None);
            self
        }

        fn add_slide_with_notes(mut self, xml_body: &str, notes: &str) -> Self {
            self.slides.push(xml_body.to_string());
            self.notes.push(Some(notes.to_string()));
            self
        }

        fn build(self) -> Vec<u8> {
            let buf = Vec::new();
            let cursor = Cursor::new(buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = SimpleFileOptions::default();

            // [Content_Types].xml
            let mut content_types = String::from(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>"#,
            );
            for i in 1..=self.slides.len() {
                content_types.push_str(&format!(
                    r#"
  <Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
                ));
            }
            content_types.push_str("\n</Types>");
            zip.start_file("[Content_Types].xml", opts).unwrap();
            zip.write_all(content_types.as_bytes()).unwrap();

            // ppt/presentation.xml
            let mut pres = String::from(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <p:sldIdLst>"#,
            );
            for i in 1..=self.slides.len() {
                pres.push_str(&format!(
                    r#"
    <p:sldId id="{}" r:id="rId{i}"/>"#,
                    255 + i
                ));
            }
            pres.push_str(
                r#"
  </p:sldIdLst>
</p:presentation>"#,
            );
            zip.start_file("ppt/presentation.xml", opts).unwrap();
            zip.write_all(pres.as_bytes()).unwrap();

            // ppt/_rels/presentation.xml.rels
            let mut rels = String::from(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            );
            for i in 1..=self.slides.len() {
                rels.push_str(&format!(
                    r#"
  <Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i}.xml"/>"#
                ));
            }
            rels.push_str("\n</Relationships>");
            zip.start_file("ppt/_rels/presentation.xml.rels", opts)
                .unwrap();
            zip.write_all(rels.as_bytes()).unwrap();

            // Slide files
            for (i, body) in self.slides.iter().enumerate() {
                let slide_num = i + 1;
                let slide_xml = format!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
       xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <p:cSld>
    <p:spTree>
      {body}
    </p:spTree>
  </p:cSld>
</p:sld>"#
                );
                zip.start_file(format!("ppt/slides/slide{slide_num}.xml"), opts)
                    .unwrap();
                zip.write_all(slide_xml.as_bytes()).unwrap();
            }

            // Notes files
            for (i, notes) in self.notes.iter().enumerate() {
                if let Some(notes_text) = notes {
                    let slide_num = i + 1;
                    let notes_xml = format!(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
         xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:txBody>
          <a:p><a:r><a:t>Slide Image</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
      <p:sp>
        <p:txBody>
          <a:p><a:r><a:t>{notes_text}</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:notes>"#
                    );
                    zip.start_file(format!("ppt/notesSlides/notesSlide{slide_num}.xml"), opts)
                        .unwrap();
                    zip.write_all(notes_xml.as_bytes()).unwrap();
                }
            }

            zip.finish().unwrap().into_inner()
        }
    }

    /// Parse a PPTX from bytes and return the store, metadata, and graph IRI.
    fn parse_pptx(data: &[u8]) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = PptxBackend::new();
        let source = DocumentSource::Stream {
            name: "test.pptx".to_string(),
            data: data.to_vec(),
        };

        let hash_str = compute_hash(data);
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    // -----------------------------------------------------------------------
    // Slide XML helpers for tests
    // -----------------------------------------------------------------------

    fn title_shape(text: &str) -> String {
        format!(
            r#"<p:sp>
        <p:nvSpPr><p:cNvPr id="1" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
        <p:txBody>
          <a:p><a:r><a:t>{text}</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>"#
        )
    }

    fn subtitle_shape(text: &str) -> String {
        format!(
            r#"<p:sp>
        <p:nvSpPr><p:cNvPr id="2" name="Subtitle"/><p:cNvSpPr/><p:nvPr><p:ph type="subTitle"/></p:nvPr></p:nvSpPr>
        <p:txBody>
          <a:p><a:r><a:t>{text}</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>"#
        )
    }

    fn body_shape(text: &str) -> String {
        format!(
            r#"<p:sp>
        <p:nvSpPr><p:cNvPr id="3" name="Body"/><p:cNvSpPr/><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
        <p:txBody>
          <a:p><a:r><a:t>{text}</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>"#
        )
    }

    fn table_shape(headers: &[&str], data_rows: &[&[&str]]) -> String {
        let mut xml = String::from("<a:tbl><a:tr>");
        for h in headers {
            xml.push_str(&format!(
                "<a:tc><a:txBody><a:p><a:r><a:t>{h}</a:t></a:r></a:p></a:txBody></a:tc>"
            ));
        }
        xml.push_str("</a:tr>");
        for row in data_rows {
            xml.push_str("<a:tr>");
            for cell in *row {
                xml.push_str(&format!(
                    "<a:tc><a:txBody><a:p><a:r><a:t>{cell}</a:t></a:r></a:p></a:txBody></a:tc>"
                ));
            }
            xml.push_str("</a:tr>");
        }
        xml.push_str("</a:tbl>");
        xml
    }

    fn picture_shape(alt: &str) -> String {
        format!(
            r#"<p:pic>
        <p:nvPicPr><p:cNvPr id="10" name="Picture" descr="{alt}"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>
        <p:blipFill><a:blip r:embed="rId2"/></p:blipFill>
      </p:pic>"#
        )
    }

    // -----------------------------------------------------------------------
    // Test: 2 slides with title and body
    // -----------------------------------------------------------------------

    #[test]
    fn parse_two_slides_with_title_and_body() -> ruddydoc_core::Result<()> {
        let slide1 = format!(
            "{}\n{}",
            title_shape("Slide One Title"),
            body_shape("Body text of slide one")
        );
        let slide2 = format!(
            "{}\n{}",
            title_shape("Slide Two Title"),
            body_shape("Body text of slide two")
        );

        let pptx_data = PptxBuilder::new()
            .add_slide(&slide1)
            .add_slide(&slide2)
            .build();

        let (store, meta, graph) = parse_pptx(&pptx_data)?;

        // Verify page count
        assert_eq!(meta.page_count, Some(2));
        assert_eq!(meta.format, InputFormat::Pptx);

        // Verify we have 2 pages
        let sparql_pages = format!(
            "SELECT ?page ?num WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?page a <{}>. \
                 ?page <{}> ?num \
               }} \
             }} ORDER BY ?num",
            ont::iri(ont::CLASS_PAGE),
            ont::iri(ont::PROP_PAGE_NUMBER),
        );
        let result = store.query_to_json(&sparql_pages)?;
        let rows = result.as_array().expect("array");
        assert_eq!(rows.len(), 2);

        // Verify titles (as SectionHeaders with headingLevel=1)
        let sparql_titles = format!(
            "SELECT ?text ?level WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text. \
                 ?h <{}> ?level \
               }} \
             }} ORDER BY ?text",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
            ont::iri(ont::PROP_HEADING_LEVEL),
        );
        let result_titles = store.query_to_json(&sparql_titles)?;
        let title_rows = result_titles.as_array().expect("array");
        assert_eq!(title_rows.len(), 2);

        // Verify body paragraphs
        let sparql_paras = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text \
               }} \
             }} ORDER BY ?text",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result_paras = store.query_to_json(&sparql_paras)?;
        let para_rows = result_paras.as_array().expect("array");
        assert_eq!(para_rows.len(), 2);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: slide with table
    // -----------------------------------------------------------------------

    #[test]
    fn parse_slide_with_table() -> ruddydoc_core::Result<()> {
        let tbl = table_shape(&["Name", "Age"], &[&["Alice", "30"], &["Bob", "25"]]);
        let slide_body = format!("{}\n{tbl}", title_shape("Data Slide"));

        let pptx_data = PptxBuilder::new().add_slide(&slide_body).build();
        let (store, _meta, graph) = parse_pptx(&pptx_data)?;

        // Verify table exists
        let sparql_table = format!(
            "ASK {{ GRAPH <{graph}> {{ ?t a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql_table)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Verify rowCount and columnCount
        let sparql_dims = format!(
            "SELECT ?rc ?cc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{cls}>. \
                 ?t <{rc}> ?rc. \
                 ?t <{cc}> ?cc \
               }} \
             }}",
            cls = ont::iri(ont::CLASS_TABLE_ELEMENT),
            rc = ont::iri(ont::PROP_ROW_COUNT),
            cc = ont::iri(ont::PROP_COLUMN_COUNT),
        );
        let result_dims = store.query_to_json(&sparql_dims)?;
        let dim_rows = result_dims.as_array().expect("array");
        assert_eq!(dim_rows.len(), 1);
        let rc = dim_rows[0]["rc"].as_str().expect("rc");
        assert!(rc.contains('3')); // 1 header + 2 data rows
        let cc = dim_rows[0]["cc"].as_str().expect("cc");
        assert!(cc.contains('2'));

        // Verify cells
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
        let result_cells = store.query_to_json(&sparql_cells)?;
        let cell_rows = result_cells.as_array().expect("array");
        assert_eq!(cell_rows.len(), 6); // 3 rows * 2 cols

        // First row should be header
        let first_is_header = cell_rows[0]["isH"].as_str().expect("isH");
        assert!(first_is_header.contains("true"));

        // Second row should not be header
        let data_is_header = cell_rows[2]["isH"].as_str().expect("isH");
        assert!(data_is_header.contains("false"));

        // Verify table is on page
        let sparql_on_page = format!(
            "ASK {{ GRAPH <{graph}> {{ ?t a <{cls}>. ?t <{op}> ?page. ?page a <{pg}> }} }}",
            cls = ont::iri(ont::CLASS_TABLE_ELEMENT),
            op = ont::iri(ont::PROP_ON_PAGE),
            pg = ont::iri(ont::CLASS_PAGE),
        );
        let result_on_page = store.query_to_json(&sparql_on_page)?;
        assert_eq!(result_on_page, serde_json::Value::Bool(true));

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: slide order (page numbers)
    // -----------------------------------------------------------------------

    #[test]
    fn verify_slide_order() -> ruddydoc_core::Result<()> {
        let pptx_data = PptxBuilder::new()
            .add_slide(&title_shape("First Slide"))
            .add_slide(&title_shape("Second Slide"))
            .add_slide(&title_shape("Third Slide"))
            .build();

        let (store, meta, graph) = parse_pptx(&pptx_data)?;
        assert_eq!(meta.page_count, Some(3));

        let sparql = format!(
            "SELECT ?num WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?page a <{}>. \
                 ?page <{}> ?num \
               }} \
             }} ORDER BY ?num",
            ont::iri(ont::CLASS_PAGE),
            ont::iri(ont::PROP_PAGE_NUMBER),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("array");
        assert_eq!(rows.len(), 3);

        // Page numbers should be 1, 2, 3
        for (i, row) in rows.iter().enumerate() {
            let num = row["num"].as_str().expect("num");
            assert!(
                num.contains(&(i + 1).to_string()),
                "expected page {} but got {}",
                i + 1,
                num
            );
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: title detection
    // -----------------------------------------------------------------------

    #[test]
    fn verify_title_detection() -> ruddydoc_core::Result<()> {
        let slide1 = format!(
            "{}\n{}\n{}",
            title_shape("Main Title"),
            subtitle_shape("A subtitle"),
            body_shape("Some body content")
        );
        let pptx_data = PptxBuilder::new().add_slide(&slide1).build();

        let (store, _meta, graph) = parse_pptx(&pptx_data)?;

        // Title should be a SectionHeader with headingLevel=1
        let sparql_title = format!(
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
        let result = store.query_to_json(&sparql_title)?;
        let rows = result.as_array().expect("array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Main Title"));

        let level = rows[0]["level"].as_str().expect("level");
        assert!(level.contains('1'));

        // Subtitle should be a Paragraph
        let sparql_sub = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text. \
                 FILTER(CONTAINS(STR(?text), \"subtitle\")) \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result_sub = store.query_to_json(&sparql_sub)?;
        let sub_rows = result_sub.as_array().expect("array");
        assert_eq!(sub_rows.len(), 1);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: supports_pagination returns true
    // -----------------------------------------------------------------------

    #[test]
    fn supports_pagination_returns_true() {
        let backend = PptxBackend::new();
        assert!(backend.supports_pagination());
    }

    // -----------------------------------------------------------------------
    // Test: onPage is set on elements
    // -----------------------------------------------------------------------

    #[test]
    fn verify_on_page_is_set() -> ruddydoc_core::Result<()> {
        let slide1 = format!(
            "{}\n{}",
            title_shape("Page 1 Title"),
            body_shape("Page 1 body")
        );
        let slide2 = body_shape("Page 2 body");

        let pptx_data = PptxBuilder::new()
            .add_slide(&slide1)
            .add_slide(&slide2)
            .build();

        let (store, _meta, graph) = parse_pptx(&pptx_data)?;

        // All elements should have onPage set
        let sparql = format!(
            "SELECT ?el ?page WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{}> ?page. \
                 ?page a <{}> \
               }} \
             }}",
            ont::iri(ont::PROP_ON_PAGE),
            ont::iri(ont::CLASS_PAGE),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("array");
        // Title + body on slide 1 + body on slide 2 = 3
        assert!(
            rows.len() >= 3,
            "expected at least 3 elements with onPage, got {}",
            rows.len()
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: is_valid accepts .pptx extension
    // -----------------------------------------------------------------------

    #[test]
    fn is_valid_accepts_pptx() {
        let backend = PptxBackend::new();

        assert!(backend.is_valid(&DocumentSource::File("presentation.pptx".into())));
        assert!(!backend.is_valid(&DocumentSource::File("document.docx".into())));
        assert!(!backend.is_valid(&DocumentSource::File("slides.pdf".into())));

        assert!(backend.is_valid(&DocumentSource::Stream {
            name: "test.pptx".to_string(),
            data: vec![],
        }));
        assert!(!backend.is_valid(&DocumentSource::Stream {
            name: "test.docx".to_string(),
            data: vec![],
        }));
    }

    // -----------------------------------------------------------------------
    // Test: error handling for invalid ZIP
    // -----------------------------------------------------------------------

    #[test]
    fn error_on_invalid_zip() {
        let backend = PptxBackend::new();
        let source = DocumentSource::Stream {
            name: "bad.pptx".to_string(),
            data: b"this is not a zip file".to_vec(),
        };
        let store = OxigraphStore::new().unwrap();
        let result = backend.parse(&source, &store, "urn:test:graph");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Test: picture element with alt text
    // -----------------------------------------------------------------------

    #[test]
    fn parse_picture_element() -> ruddydoc_core::Result<()> {
        let slide_body = format!(
            "{}\n{}",
            title_shape("Slide with Pic"),
            picture_shape("A beautiful sunset")
        );

        let pptx_data = PptxBuilder::new().add_slide(&slide_body).build();
        let (store, _meta, graph) = parse_pptx(&pptx_data)?;

        let sparql = format!(
            "SELECT ?alt WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?alt \
               }} \
             }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
            ont::iri(ont::PROP_ALT_TEXT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("array");
        assert_eq!(rows.len(), 1);

        let alt = rows[0]["alt"].as_str().expect("alt");
        assert!(alt.contains("A beautiful sunset"));

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: speaker notes
    // -----------------------------------------------------------------------

    #[test]
    fn parse_speaker_notes() -> ruddydoc_core::Result<()> {
        let pptx_data = PptxBuilder::new()
            .add_slide_with_notes(
                &title_shape("Slide With Notes"),
                "Remember to mention the quarterly results",
            )
            .build();

        let (store, _meta, graph) = parse_pptx(&pptx_data)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?n a <{}>. \
                 ?n <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_FOOTNOTE),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Remember to mention the quarterly results"));

        // Notes should also have onPage
        let sparql_on_page = format!(
            "ASK {{ GRAPH <{graph}> {{ ?n a <{cls}>. ?n <{op}> ?page }} }}",
            cls = ont::iri(ont::CLASS_FOOTNOTE),
            op = ont::iri(ont::PROP_ON_PAGE),
        );
        let result_on_page = store.query_to_json(&sparql_on_page)?;
        assert_eq!(result_on_page, serde_json::Value::Bool(true));

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: center title variant
    // -----------------------------------------------------------------------

    #[test]
    fn center_title_is_detected() -> ruddydoc_core::Result<()> {
        let center_title = r#"<p:sp>
        <p:nvSpPr><p:cNvPr id="1" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr>
        <p:txBody>
          <a:p><a:r><a:t>Centered Title</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>"#;

        let pptx_data = PptxBuilder::new().add_slide(center_title).build();
        let (store, _meta, graph) = parse_pptx(&pptx_data)?;

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
        let rows = result.as_array().expect("array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Centered Title"));

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: document metadata
    // -----------------------------------------------------------------------

    #[test]
    fn document_metadata() -> ruddydoc_core::Result<()> {
        let pptx_data = PptxBuilder::new()
            .add_slide(&title_shape("Slide 1"))
            .build();

        let (store, meta, graph) = parse_pptx(&pptx_data)?;

        assert_eq!(meta.format, InputFormat::Pptx);
        assert_eq!(meta.page_count, Some(1));
        assert!(meta.file_size > 0);

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);

        // Check document has sourceFormat
        let sparql = format!(
            "SELECT ?fmt WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?fmt \
               }} \
             }}",
            ont::iri(ont::PROP_SOURCE_FORMAT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("array");
        assert_eq!(rows.len(), 1);
        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("pptx"));

        // Check document has pageCount
        let sparql_pc = format!(
            "SELECT ?pc WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?pc \
               }} \
             }}",
            ont::iri(ont::PROP_PAGE_COUNT),
        );
        let result_pc = store.query_to_json(&sparql_pc)?;
        let pc_rows = result_pc.as_array().expect("array");
        assert_eq!(pc_rows.len(), 1);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: multi-run paragraph text concatenation
    // -----------------------------------------------------------------------

    #[test]
    fn multi_run_text_concatenation() -> ruddydoc_core::Result<()> {
        let multi_run = r#"<p:sp>
        <p:nvSpPr><p:cNvPr id="1" name="Body"/><p:cNvSpPr/><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
        <p:txBody>
          <a:p>
            <a:r><a:t>First </a:t></a:r>
            <a:r><a:rPr b="1"/><a:t>bold</a:t></a:r>
            <a:r><a:t> text</a:t></a:r>
          </a:p>
        </p:txBody>
      </p:sp>"#;

        let pptx_data = PptxBuilder::new().add_slide(multi_run).build();
        let (store, _meta, graph) = parse_pptx(&pptx_data)?;

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
        let rows = result.as_array().expect("array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("First bold text"));

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test: default impl works
    // -----------------------------------------------------------------------

    #[test]
    fn default_impl_works() {
        let backend = PptxBackend::default();
        assert_eq!(backend.supported_formats(), &[InputFormat::Pptx]);
        assert!(backend.supports_pagination());
    }

    // -----------------------------------------------------------------------
    // Test: hash is deterministic
    // -----------------------------------------------------------------------

    #[test]
    fn hash_is_deterministic() {
        let data = b"hello pptx";
        let h1 = compute_hash(data);
        let h2 = compute_hash(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex is 64 chars
    }

    // -----------------------------------------------------------------------
    // Test: reading order is sequential
    // -----------------------------------------------------------------------

    #[test]
    fn reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let slide1 = format!("{}\n{}", title_shape("Title"), body_shape("Body"));
        let pptx_data = PptxBuilder::new().add_slide(&slide1).build();

        let (store, _meta, graph) = parse_pptx(&pptx_data)?;

        let sparql = format!(
            "SELECT ?el ?order WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{}> ?order \
               }} \
             }} ORDER BY ?order",
            ont::iri(ont::PROP_READING_ORDER),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("array");
        assert!(rows.len() >= 2); // title + body at minimum

        // Verify sequential ordering
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
