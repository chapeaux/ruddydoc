//! WebVTT subtitle parser backend for RuddyDoc.
//!
//! Custom parser for WebVTT cue files. Each cue is mapped to a
//! `rdoc:Paragraph` with `rdoc:startTime` and `rdoc:endTime` properties.

use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

/// WebVTT document backend.
pub struct WebVttBackend;

impl WebVttBackend {
    /// Create a new WebVTT backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebVttBackend {
    fn default() -> Self {
        Self::new()
    }
}

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

/// Strip HTML tags from cue text.
fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// A parsed WebVTT cue.
struct Cue {
    start: String,
    end: String,
    text: String,
}

/// Parse WebVTT content into a list of cues.
///
/// Handles:
/// - WEBVTT header line
/// - Optional cue identifiers
/// - Timestamp parsing (HH:MM:SS.mmm or MM:SS.mmm)
/// - Multi-line cues (concatenated with newlines)
/// - Cue settings (ignored)
/// - HTML tags in cues (stripped)
/// - NOTE blocks (skipped)
fn parse_cues(
    content: &str,
) -> std::result::Result<Vec<Cue>, Box<dyn std::error::Error + Send + Sync>> {
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() || !lines[0].starts_with("WEBVTT") {
        return Err("invalid WebVTT: missing WEBVTT header".into());
    }

    let mut cues = Vec::new();
    let mut i = 1;

    // Skip header metadata (lines after WEBVTT until first blank line)
    while i < lines.len() && !lines[i].is_empty() {
        i += 1;
    }

    while i < lines.len() {
        // Skip blank lines
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }

        // Skip NOTE blocks
        if lines[i].starts_with("NOTE") {
            i += 1;
            while i < lines.len() && !lines[i].trim().is_empty() {
                i += 1;
            }
            continue;
        }

        // Skip STYLE blocks
        if lines[i].starts_with("STYLE") {
            i += 1;
            while i < lines.len() && !lines[i].trim().is_empty() {
                i += 1;
            }
            continue;
        }

        // Try to find a timestamp line. If the current line contains "-->",
        // it is the timestamp. Otherwise, it might be a cue identifier;
        // check the next line.
        let timestamp_line;
        if lines[i].contains("-->") {
            timestamp_line = lines[i];
            i += 1;
        } else {
            // Possible cue identifier; the next line should be the timestamp
            i += 1;
            if i < lines.len() && lines[i].contains("-->") {
                timestamp_line = lines[i];
                i += 1;
            } else {
                // Not a cue we recognize; skip
                continue;
            }
        }

        // Parse the timestamp line: "start --> end [settings]"
        let (start, end) = parse_timestamp_line(timestamp_line)?;

        // Collect cue text lines until a blank line or end of input
        let mut text_lines = Vec::new();
        while i < lines.len() && !lines[i].trim().is_empty() {
            text_lines.push(lines[i]);
            i += 1;
        }

        let raw_text = text_lines.join("\n");
        let text = strip_html_tags(&raw_text);

        if !text.trim().is_empty() {
            cues.push(Cue {
                start,
                end,
                text: text.trim().to_string(),
            });
        }
    }

    Ok(cues)
}

/// Parse a timestamp line like "00:00:01.000 --> 00:00:05.000 position:10%"
/// and return (start, end) timestamps.
fn parse_timestamp_line(
    line: &str,
) -> std::result::Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return Err(format!("invalid timestamp line: {line}").into());
    }

    let start = parts[0].trim().to_string();
    // End timestamp may have cue settings appended; take only the timestamp part
    let end_part = parts[1].trim();
    let end = end_part
        .split_whitespace()
        .next()
        .ok_or_else(|| format!("missing end timestamp in: {line}"))?
        .to_string();

    // Validate timestamp format (basic check)
    validate_timestamp(&start)?;
    validate_timestamp(&end)?;

    Ok((start, end))
}

/// Validate that a string looks like a WebVTT timestamp.
/// Accepts HH:MM:SS.mmm or MM:SS.mmm formats.
fn validate_timestamp(
    ts: &str,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parts: Vec<&str> = ts.split(':').collect();
    match parts.len() {
        2 | 3 => {
            // Last part should contain seconds.milliseconds
            let last = parts.last().ok_or("empty timestamp")?;
            if !last.contains('.') {
                return Err(format!("timestamp missing milliseconds: {ts}").into());
            }
            Ok(())
        }
        _ => Err(format!("invalid timestamp format: {ts}").into()),
    }
}

/// Parse context used during graph construction.
struct ParseContext<'a> {
    store: &'a dyn DocumentStore,
    doc_graph: &'a str,
    doc_hash: &'a str,
    reading_order: usize,
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

    /// Insert an element into the graph with its type and reading order.
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

        // Previous/next sibling links
        if let Some(prev) = self.all_elements.last() {
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

impl DocumentBackend for WebVttBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::WebVtt]
    }

    fn supports_pagination(&self) -> bool {
        false
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(path.extension().and_then(|e| e.to_str()), Some("vtt"))
            }
            DocumentSource::Stream { name, .. } => name.ends_with(".vtt"),
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
            "webvtt",
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

        // Parse the WebVTT cues
        let cues = parse_cues(&content)?;

        let mut ctx = ParseContext::new(store, g, &hash_str);

        for cue in &cues {
            let iri = ctx.element_iri("cue");
            ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
            ctx.set_text_content(&iri, &cue.text)?;

            // startTime
            store.insert_literal(
                &iri,
                &ont::iri(ont::PROP_START_TIME),
                &cue.start,
                "string",
                g,
            )?;

            // endTime
            store.insert_literal(&iri, &ont::iri(ont::PROP_END_TIME), &cue.end, "string", g)?;
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::WebVtt,
            file_size,
            page_count: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;

    fn parse_webvtt(vtt: &str) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = WebVttBackend::new();
        let source = DocumentSource::Stream {
            name: "test.vtt".to_string(),
            data: vtt.as_bytes().to_vec(),
        };

        let hash_str = compute_hash(vtt.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    #[test]
    fn parse_basic_cues() -> ruddydoc_core::Result<()> {
        let vtt = "\
WEBVTT

00:00:01.000 --> 00:00:05.000
Hello, welcome to the presentation.

00:00:05.500 --> 00:00:10.000
Today we'll discuss RuddyDoc.

00:00:10.500 --> 00:00:15.000
Let's get started.
";

        let (store, _meta, graph) = parse_webvtt(vtt)?;

        let sparql = format!(
            "SELECT ?text ?start ?end WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text. \
                 ?p <{}> ?start. \
                 ?p <{}> ?end \
               }} \
             }} ORDER BY ?start",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
            ont::iri(ont::PROP_START_TIME),
            ont::iri(ont::PROP_END_TIME),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 3);

        let text0 = rows[0]["text"].as_str().expect("text");
        assert!(text0.contains("Hello, welcome"));

        let start0 = rows[0]["start"].as_str().expect("start");
        assert!(start0.contains("00:00:01.000"));

        let end0 = rows[0]["end"].as_str().expect("end");
        assert!(end0.contains("00:00:05.000"));

        Ok(())
    }

    #[test]
    fn parse_cues_with_identifiers() -> ruddydoc_core::Result<()> {
        let vtt = "\
WEBVTT

intro
00:00:01.000 --> 00:00:05.000
Hello there.

chapter1
00:00:05.500 --> 00:00:10.000
Chapter one begins.
";

        let (store, _meta, graph) = parse_webvtt(vtt)?;

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
    fn parse_multi_line_cues() -> ruddydoc_core::Result<()> {
        let vtt = "\
WEBVTT

00:00:01.000 --> 00:00:05.000
Line one of the cue.
Line two of the cue.

00:00:06.000 --> 00:00:09.000
Single line.
";

        let (store, _meta, graph) = parse_webvtt(vtt)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text \
               }} \
             }} ORDER BY ?text",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);

        // The multi-line cue should have both lines joined with newline
        let found_multiline = rows.iter().any(|row| {
            let text = row["text"].as_str().unwrap_or("");
            text.contains("Line one") && text.contains("Line two")
        });
        assert!(found_multiline, "multi-line cue text should be preserved");

        Ok(())
    }

    #[test]
    fn html_tags_are_stripped() -> ruddydoc_core::Result<()> {
        let vtt = "\
WEBVTT

00:00:01.000 --> 00:00:05.000
<b>Bold text</b> and <i>italic</i>.
";

        let (store, _meta, graph) = parse_webvtt(vtt)?;

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
        assert!(text.contains("Bold text"));
        assert!(!text.contains("<b>"));
        assert!(!text.contains("</b>"));

        Ok(())
    }

    #[test]
    fn note_blocks_are_skipped() -> ruddydoc_core::Result<()> {
        let vtt = "\
WEBVTT

NOTE
This is a comment.

00:00:01.000 --> 00:00:05.000
Actual content.
";

        let (store, _meta, graph) = parse_webvtt(vtt)?;

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
        assert!(text.contains("Actual content"));

        Ok(())
    }

    #[test]
    fn reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let vtt = "\
WEBVTT

00:00:01.000 --> 00:00:03.000
First.

00:00:03.500 --> 00:00:06.000
Second.

00:00:06.500 --> 00:00:09.000
Third.
";

        let (store, _meta, graph) = parse_webvtt(vtt)?;

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
        assert_eq!(rows.len(), 3);

        Ok(())
    }

    #[test]
    fn document_metadata() -> ruddydoc_core::Result<()> {
        let vtt = "\
WEBVTT

00:00:01.000 --> 00:00:05.000
Hello.
";

        let (_store, meta, _graph) = parse_webvtt(vtt)?;

        assert_eq!(meta.format, InputFormat::WebVtt);
        assert!(meta.page_count.is_none());
        assert!(meta.file_path.is_none());

        Ok(())
    }

    #[test]
    fn invalid_header_is_rejected() {
        let vtt = "NOT A WEBVTT FILE\n\n00:00:01.000 --> 00:00:05.000\nHello.\n";
        let result = parse_webvtt(vtt);
        assert!(result.is_err());
    }

    #[test]
    fn is_valid_checks_extension() {
        let backend = WebVttBackend::new();

        let valid_file = DocumentSource::File(std::path::PathBuf::from("test.vtt"));
        assert!(backend.is_valid(&valid_file));

        let invalid_file = DocumentSource::File(std::path::PathBuf::from("test.md"));
        assert!(!backend.is_valid(&invalid_file));

        let valid_stream = DocumentSource::Stream {
            name: "captions.vtt".to_string(),
            data: vec![],
        };
        assert!(backend.is_valid(&valid_stream));
    }

    #[test]
    fn cue_settings_are_ignored() -> ruddydoc_core::Result<()> {
        let vtt = "\
WEBVTT

00:00:01.000 --> 00:00:05.000 position:10% align:start
Content with settings.
";

        let (store, _meta, graph) = parse_webvtt(vtt)?;

        let sparql = format!(
            "SELECT ?end WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?end \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_END_TIME),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let end = rows[0]["end"].as_str().expect("end");
        assert!(end.contains("00:00:05.000"));
        // Should NOT contain cue settings
        assert!(!end.contains("position"));

        Ok(())
    }

    #[test]
    fn previous_next_links() -> ruddydoc_core::Result<()> {
        let vtt = "\
WEBVTT

00:00:01.000 --> 00:00:03.000
First.

00:00:03.500 --> 00:00:06.000
Second.
";

        let (store, _meta, graph) = parse_webvtt(vtt)?;

        let sparql = format!(
            "SELECT ?a ?b WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?a <{}> ?b \
               }} \
             }}",
            ont::iri(ont::PROP_NEXT_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        Ok(())
    }
}
