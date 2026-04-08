//! Markdown parser backend for RuddyDoc.
//!
//! Uses `pulldown-cmark` with GFM extensions to parse Markdown documents
//! into the RuddyDoc document ontology graph.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

/// Markdown document backend.
pub struct MarkdownBackend;

impl MarkdownBackend {
    /// Create a new Markdown backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for MarkdownBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a `HeadingLevel` to a numeric level (1-6).
fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Compute a SHA-256 hash of the content bytes, returning a hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Hex-encode bytes (we inline this to avoid another dependency).
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
    }
}

/// State machine context for Markdown parsing.
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
    /// Whether we are inside a heading.
    in_heading: Option<u8>,
    /// Whether we are inside a code block, with optional language.
    in_code_block: Option<Option<String>>,
    /// Whether we are inside a paragraph.
    in_paragraph: bool,
    /// Whether we are inside a block quote.
    in_block_quote: bool,
    /// List nesting: each entry is Some(start_number) for ordered, None for unordered.
    list_stack: Vec<Option<u64>>,
    /// Whether we are inside a list item.
    in_list_item: bool,
    /// Table state.
    in_table: bool,
    table_is_head: bool,
    table_row: usize,
    table_col: usize,
    table_max_col: usize,
    table_iri: Option<String>,
    /// Image state.
    in_image: bool,
    image_dest: String,
    image_title: String,
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
            parent_stack: Vec::new(),
            last_sibling_at_depth: Vec::new(),
            text_buf: String::new(),
            in_heading: None,
            in_code_block: None,
            in_paragraph: false,
            in_block_quote: false,
            list_stack: Vec::new(),
            in_list_item: false,
            in_table: false,
            table_is_head: false,
            table_row: 0,
            table_col: 0,
            table_max_col: 0,
            table_iri: None,
            in_image: false,
            image_dest: String::new(),
            image_title: String::new(),
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

impl DocumentBackend for MarkdownBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Markdown]
    }

    fn supports_pagination(&self) -> bool {
        false
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("md" | "markdown")
                )
            }
            DocumentSource::Stream { name, .. } => {
                name.ends_with(".md") || name.ends_with(".markdown")
            }
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
            "markdown",
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

        // Parse the Markdown
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_FOOTNOTES);

        let parser = Parser::new_ext(&content, options);
        let events: Vec<Event<'_>> = parser.collect();

        let mut ctx = ParseContext::new(store, g, &hash_str);

        for event in &events {
            match event {
                Event::Start(tag) => {
                    handle_start_tag(&mut ctx, tag)?;
                }
                Event::End(tag_end) => {
                    handle_end_tag(&mut ctx, tag_end)?;
                }
                Event::Text(text) => {
                    ctx.text_buf.push_str(text);
                }
                Event::Code(code) => {
                    // Inline code: add as text
                    ctx.text_buf.push_str(code);
                }
                Event::SoftBreak | Event::HardBreak => {
                    ctx.text_buf.push('\n');
                }
                Event::Rule => {
                    // Horizontal rule: skip (minimal representation)
                }
                Event::Html(html) | Event::InlineHtml(html) => {
                    ctx.text_buf.push_str(html);
                }
                _ => {}
            }
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Markdown,
            file_size,
            page_count: None,
        })
    }
}

fn handle_start_tag(ctx: &mut ParseContext<'_>, tag: &Tag<'_>) -> ruddydoc_core::Result<()> {
    match tag {
        Tag::Heading { level, .. } => {
            ctx.text_buf.clear();
            ctx.in_heading = Some(heading_level_to_u8(*level));
        }
        Tag::Paragraph => {
            ctx.text_buf.clear();
            ctx.in_paragraph = true;
        }
        Tag::BlockQuote(_) => {
            ctx.in_block_quote = true;
            let iri = ctx.element_iri("group");
            ctx.emit_element(&iri, ont::CLASS_GROUP)?;
            ctx.parent_stack.push(iri);
        }
        Tag::CodeBlock(kind) => {
            ctx.text_buf.clear();
            let lang = match kind {
                CodeBlockKind::Fenced(lang) => {
                    let l = lang.to_string();
                    if l.is_empty() { None } else { Some(l) }
                }
                CodeBlockKind::Indented => None,
            };
            ctx.in_code_block = Some(lang);
        }
        Tag::List(start) => {
            let is_ordered = start.is_some();
            let list_class = if is_ordered {
                ont::CLASS_ORDERED_LIST
            } else {
                ont::CLASS_UNORDERED_LIST
            };
            let iri = ctx.element_iri("list");
            ctx.emit_element(&iri, list_class)?;
            ctx.parent_stack.push(iri);
            ctx.list_stack.push(*start);
        }
        Tag::Item => {
            ctx.text_buf.clear();
            ctx.in_list_item = true;
        }
        Tag::Table(_alignments) => {
            ctx.in_table = true;
            ctx.table_row = 0;
            ctx.table_max_col = 0;
            let iri = ctx.element_iri("table");
            ctx.emit_element(&iri, ont::CLASS_TABLE_ELEMENT)?;
            ctx.table_iri = Some(iri.clone());
            ctx.parent_stack.push(iri);
        }
        Tag::TableHead => {
            ctx.table_is_head = true;
            ctx.table_col = 0;
        }
        Tag::TableRow => {
            ctx.table_col = 0;
        }
        Tag::TableCell => {
            ctx.text_buf.clear();
        }
        Tag::Image {
            dest_url, title, ..
        } => {
            ctx.in_image = true;
            ctx.image_dest = dest_url.to_string();
            ctx.image_title = title.to_string();
            ctx.text_buf.clear();
        }
        // Inline formatting tags: we just continue accumulating text
        _ => {}
    }
    Ok(())
}

fn handle_end_tag(ctx: &mut ParseContext<'_>, tag: &TagEnd) -> ruddydoc_core::Result<()> {
    match tag {
        TagEnd::Heading(_level) => {
            if let Some(level) = ctx.in_heading.take() {
                let text = ctx.text_buf.trim().to_string();
                ctx.text_buf.clear();

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
        TagEnd::Paragraph => {
            if ctx.in_image {
                // Image paragraph: we handle in Image end tag
                return Ok(());
            }
            ctx.in_paragraph = false;
            let text = ctx.text_buf.trim().to_string();
            ctx.text_buf.clear();

            if !text.is_empty() {
                let iri = ctx.element_iri("paragraph");
                ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                ctx.set_text_content(&iri, &text)?;
            }
        }
        TagEnd::BlockQuote(_) => {
            ctx.in_block_quote = false;
            ctx.parent_stack.pop();
        }
        TagEnd::CodeBlock => {
            if let Some(lang) = ctx.in_code_block.take() {
                let text = ctx.text_buf.clone();
                ctx.text_buf.clear();

                let iri = ctx.element_iri("code");
                ctx.emit_element(&iri, ont::CLASS_CODE)?;
                ctx.set_text_content(&iri, &text)?;

                if let Some(language) = lang {
                    ctx.store.insert_literal(
                        &iri,
                        &ont::iri(ont::PROP_CODE_LANGUAGE),
                        &language,
                        "string",
                        ctx.doc_graph,
                    )?;
                }
            }
        }
        TagEnd::List(_is_ordered) => {
            ctx.list_stack.pop();
            ctx.parent_stack.pop();
        }
        TagEnd::Item => {
            ctx.in_list_item = false;
            let text = ctx.text_buf.trim().to_string();
            ctx.text_buf.clear();

            if !text.is_empty() {
                let iri = ctx.element_iri("listitem");
                ctx.emit_element(&iri, ont::CLASS_LIST_ITEM)?;
                ctx.set_text_content(&iri, &text)?;
            }
        }
        TagEnd::Table => {
            ctx.in_table = false;
            // Set row count and column count on the table element
            if let Some(table_iri) = ctx.table_iri.take() {
                ctx.store.insert_literal(
                    &table_iri,
                    &ont::iri(ont::PROP_ROW_COUNT),
                    &ctx.table_row.to_string(),
                    "integer",
                    ctx.doc_graph,
                )?;
                ctx.store.insert_literal(
                    &table_iri,
                    &ont::iri(ont::PROP_COLUMN_COUNT),
                    &ctx.table_max_col.to_string(),
                    "integer",
                    ctx.doc_graph,
                )?;
            }
            ctx.parent_stack.pop();
        }
        TagEnd::TableHead => {
            ctx.table_is_head = false;
            ctx.table_row += 1;
            if ctx.table_col > ctx.table_max_col {
                ctx.table_max_col = ctx.table_col;
            }
        }
        TagEnd::TableRow => {
            ctx.table_row += 1;
            if ctx.table_col > ctx.table_max_col {
                ctx.table_max_col = ctx.table_col;
            }
        }
        TagEnd::TableCell => {
            let text = ctx.text_buf.trim().to_string();
            ctx.text_buf.clear();

            if let Some(table_iri) = &ctx.table_iri {
                let cell_iri = ruddydoc_core::element_iri(
                    ctx.doc_hash,
                    &format!("cell-{}-{}", ctx.table_row, ctx.table_col),
                );
                let rdf_type = ont::rdf_iri("type");
                let g = ctx.doc_graph;

                ctx.store.insert_triple_into(
                    &cell_iri,
                    &rdf_type,
                    &ont::iri(ont::CLASS_TABLE_CELL),
                    g,
                )?;
                ctx.store.insert_triple_into(
                    table_iri,
                    &ont::iri(ont::PROP_HAS_CELL),
                    &cell_iri,
                    g,
                )?;
                ctx.store.insert_literal(
                    &cell_iri,
                    &ont::iri(ont::PROP_CELL_ROW),
                    &ctx.table_row.to_string(),
                    "integer",
                    g,
                )?;
                ctx.store.insert_literal(
                    &cell_iri,
                    &ont::iri(ont::PROP_CELL_COLUMN),
                    &ctx.table_col.to_string(),
                    "integer",
                    g,
                )?;
                ctx.store.insert_literal(
                    &cell_iri,
                    &ont::iri(ont::PROP_CELL_TEXT),
                    &text,
                    "string",
                    g,
                )?;
                ctx.store.insert_literal(
                    &cell_iri,
                    &ont::iri(ont::PROP_IS_HEADER),
                    if ctx.table_is_head { "true" } else { "false" },
                    "boolean",
                    g,
                )?;
            }
            ctx.table_col += 1;
        }
        TagEnd::Image => {
            ctx.in_image = false;
            let alt_text = ctx.text_buf.trim().to_string();
            ctx.text_buf.clear();

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
            if !ctx.image_dest.is_empty() {
                ctx.store.insert_literal(
                    &iri,
                    &ont::iri(ont::PROP_LINK_TARGET),
                    &ctx.image_dest,
                    "string",
                    ctx.doc_graph,
                )?;
            }
            // Infer format from URL
            if let Some(ext) = ctx
                .image_dest
                .rsplit('.')
                .next()
                .map(|s| s.to_lowercase())
                .filter(|ext| {
                    matches!(
                        ext.as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "tiff" | "bmp"
                    )
                })
            {
                ctx.store.insert_literal(
                    &iri,
                    &ont::iri(ont::PROP_PICTURE_FORMAT),
                    &ext,
                    "string",
                    ctx.doc_graph,
                )?;
            }

            ctx.image_dest.clear();
            ctx.image_title.clear();
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;

    fn parse_markdown(
        markdown: &str,
    ) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = MarkdownBackend::new();
        let source = DocumentSource::Stream {
            name: "test.md".to_string(),
            data: markdown.as_bytes().to_vec(),
        };

        let hash_str = compute_hash(markdown.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    #[test]
    fn parse_heading() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_markdown("# Hello World")?;

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

        Ok(())
    }

    #[test]
    fn parse_paragraph() -> ruddydoc_core::Result<()> {
        let (store, _meta, graph) = parse_markdown("This is a paragraph.")?;

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
        assert!(text.contains("This is a paragraph."));
        Ok(())
    }

    #[test]
    fn parse_multiple_headings() -> ruddydoc_core::Result<()> {
        let md = "# H1\n\n## H2\n\n### H3\n";
        let (store, _meta, graph) = parse_markdown(md)?;

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
    fn parse_code_block() -> ruddydoc_core::Result<()> {
        let md = "```rust\nfn main() {}\n```\n";
        let (store, _meta, graph) = parse_markdown(md)?;

        let sparql = format!(
            "SELECT ?text ?lang WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> ?lang \
               }} \
             }}",
            ont::iri(ont::CLASS_CODE),
            ont::iri(ont::PROP_TEXT_CONTENT),
            ont::iri(ont::PROP_CODE_LANGUAGE),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("fn main()"));

        let lang = rows[0]["lang"].as_str().expect("lang");
        assert!(lang.contains("rust"));
        Ok(())
    }

    #[test]
    fn parse_unordered_list() -> ruddydoc_core::Result<()> {
        let md = "- Item one\n- Item two\n- Item three\n";
        let (store, _meta, graph) = parse_markdown(md)?;

        // Check that we have an UnorderedList
        let sparql_list = format!(
            "ASK {{ GRAPH <{graph}> {{ ?l a <{}> }} }}",
            ont::iri(ont::CLASS_UNORDERED_LIST),
        );
        let result = store.query_to_json(&sparql_list)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check that we have 3 list items
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
        let md = "1. First\n2. Second\n3. Third\n";
        let (store, _meta, graph) = parse_markdown(md)?;

        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?l a <{}> }} }}",
            ont::iri(ont::CLASS_ORDERED_LIST),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn parse_table() -> ruddydoc_core::Result<()> {
        let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |\n| Bob | 25 |\n";
        let (store, _meta, graph) = parse_markdown(md)?;

        // Check that we have a table
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
        // 2 header cells + 4 data cells = 6 total
        assert_eq!(rows.len(), 6);

        // First cell should be a header
        let first_is_header = rows[0]["isH"].as_str().expect("isH");
        assert!(first_is_header.contains("true"));

        Ok(())
    }

    #[test]
    fn parse_image() -> ruddydoc_core::Result<()> {
        let md = "![Alt text](image.png)\n";
        let (store, _meta, graph) = parse_markdown(md)?;

        let sparql = format!(
            "SELECT ?alt ?target ?fmt WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?alt. \
                 ?p <{}> ?target. \
                 ?p <{}> ?fmt \
               }} \
             }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
            ont::iri(ont::PROP_ALT_TEXT),
            ont::iri(ont::PROP_LINK_TARGET),
            ont::iri(ont::PROP_PICTURE_FORMAT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let alt = rows[0]["alt"].as_str().expect("alt");
        assert!(alt.contains("Alt text"));

        let target = rows[0]["target"].as_str().expect("target");
        assert!(target.contains("image.png"));
        Ok(())
    }

    #[test]
    fn parse_block_quote() -> ruddydoc_core::Result<()> {
        let md = "> This is a quote.\n";
        let (store, _meta, graph) = parse_markdown(md)?;

        // The block quote should create a Group
        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?g a <{}> }} }}",
            ont::iri(ont::CLASS_GROUP),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let md = "# Heading\n\nParagraph one.\n\nParagraph two.\n";
        let (store, _meta, graph) = parse_markdown(md)?;

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

    #[test]
    fn document_metadata() -> ruddydoc_core::Result<()> {
        let md = "# Test\n";
        let (store, meta, graph) = parse_markdown(md)?;

        assert_eq!(meta.format, InputFormat::Markdown);
        assert!(meta.page_count.is_none());

        // Check document node exists with sourceFormat
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

    #[test]
    fn parent_child_links() -> ruddydoc_core::Result<()> {
        let md = "- Item one\n- Item two\n";
        let (store, _meta, graph) = parse_markdown(md)?;

        // List items should have the list as parent
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
}
