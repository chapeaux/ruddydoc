//! HTML parser backend for RuddyDoc.
//!
//! Uses the `scraper` crate for DOM parsing to extract document structure
//! from HTML into the RuddyDoc document ontology graph.

use scraper::{ElementRef, Html, Selector};
use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

/// HTML document backend.
pub struct HtmlBackend;

impl HtmlBackend {
    /// Create a new HTML backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for HtmlBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a SHA-256 hash of the content bytes, returning a hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Hex-encode bytes.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
    }
}

/// State machine context for HTML parsing.
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

/// Map an HTML tag name to an ontology class, returning `None` for tags we skip.
fn tag_to_class(tag: &str) -> Option<&'static str> {
    match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(ont::CLASS_SECTION_HEADER),
        "p" => Some(ont::CLASS_PARAGRAPH),
        "ul" => Some(ont::CLASS_UNORDERED_LIST),
        "ol" => Some(ont::CLASS_ORDERED_LIST),
        "li" => Some(ont::CLASS_LIST_ITEM),
        "table" => Some(ont::CLASS_TABLE_ELEMENT),
        "blockquote" => Some(ont::CLASS_GROUP),
        "article" | "section" => Some(ont::CLASS_GROUP),
        "header" => Some(ont::CLASS_PAGE_HEADER),
        "footer" => Some(ont::CLASS_PAGE_FOOTER),
        "pre" => None,  // handled specially when contains <code>
        "code" => None, // handled inside <pre>
        "img" => Some(ont::CLASS_PICTURE_ELEMENT),
        "figure" => None,     // handled specially
        "figcaption" => None, // handled inside <figure>
        "a" => Some(ont::CLASS_HYPERLINK),
        "title" => Some(ont::CLASS_TITLE),
        _ => None,
    }
}

/// Extract the heading level (1-6) from a tag name like "h1".
fn heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Collect all direct text content from an element (concatenation of text nodes
/// at any depth within this element).
fn collect_text(element: &ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

/// Check whether an element is a `<pre>` that contains a `<code>` child.
fn is_code_block(element: &ElementRef<'_>) -> bool {
    if element.value().name() != "pre" {
        return false;
    }
    let sel = Selector::parse("code").expect("valid selector");
    element.select(&sel).next().is_some()
}

/// Extract the language from a `<code>` element's class attribute.
/// Conventions: `language-rust`, `lang-rust`, or just `rust` as a class.
fn extract_code_language(element: &ElementRef<'_>) -> Option<String> {
    let sel = Selector::parse("code").expect("valid selector");
    let code_el = element.select(&sel).next()?;
    let classes = code_el.value().attr("class")?;
    for class in classes.split_whitespace() {
        if let Some(lang) = class.strip_prefix("language-") {
            return Some(lang.to_string());
        }
        if let Some(lang) = class.strip_prefix("lang-") {
            return Some(lang.to_string());
        }
    }
    // If none matched the prefix pattern, use the first class as-is
    classes.split_whitespace().next().map(|s| s.to_string())
}

/// Process a `<table>` element, emitting table and cell triples.
fn process_table(
    ctx: &mut ParseContext<'_>,
    element: &ElementRef<'_>,
) -> ruddydoc_core::Result<()> {
    let table_iri = ctx.element_iri("table");
    ctx.emit_element(&table_iri, ont::CLASS_TABLE_ELEMENT)?;

    let tr_sel = Selector::parse("tr").expect("valid selector");
    let th_sel = Selector::parse("th").expect("valid selector");
    let td_sel = Selector::parse("td").expect("valid selector");

    let mut row_count: usize = 0;
    let mut max_col: usize = 0;

    for tr in element.select(&tr_sel) {
        let mut col: usize = 0;

        // Process th cells
        for th in tr.select(&th_sel) {
            let text = collect_text(&th);
            let row_span = th
                .value()
                .attr("rowspan")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1);
            let col_span = th
                .value()
                .attr("colspan")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1);

            emit_table_cell(
                ctx, &table_iri, &text, row_count, col, true, row_span, col_span,
            )?;
            col += col_span;
        }

        // Process td cells
        for td in tr.select(&td_sel) {
            let text = collect_text(&td);
            let row_span = td
                .value()
                .attr("rowspan")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1);
            let col_span = td
                .value()
                .attr("colspan")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1);

            emit_table_cell(
                ctx, &table_iri, &text, row_count, col, false, row_span, col_span,
            )?;
            col += col_span;
        }

        if col > max_col {
            max_col = col;
        }
        row_count += 1;
    }

    // Set row count and column count on the table
    ctx.store.insert_literal(
        &table_iri,
        &ont::iri(ont::PROP_ROW_COUNT),
        &row_count.to_string(),
        "integer",
        ctx.doc_graph,
    )?;
    ctx.store.insert_literal(
        &table_iri,
        &ont::iri(ont::PROP_COLUMN_COUNT),
        &max_col.to_string(),
        "integer",
        ctx.doc_graph,
    )?;

    Ok(())
}

/// Emit a single table cell triple.
#[allow(clippy::too_many_arguments)]
fn emit_table_cell(
    ctx: &mut ParseContext<'_>,
    table_iri: &str,
    text: &str,
    row: usize,
    col: usize,
    is_header: bool,
    row_span: usize,
    col_span: usize,
) -> ruddydoc_core::Result<()> {
    let cell_iri = ruddydoc_core::element_iri(ctx.doc_hash, &format!("cell-{row}-{col}"));
    let rdf_type = ont::rdf_iri("type");
    let g = ctx.doc_graph;

    ctx.store
        .insert_triple_into(&cell_iri, &rdf_type, &ont::iri(ont::CLASS_TABLE_CELL), g)?;
    ctx.store
        .insert_triple_into(table_iri, &ont::iri(ont::PROP_HAS_CELL), &cell_iri, g)?;
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

    // Always emit span attributes
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

    Ok(())
}

/// Process a `<figure>` element, emitting a PictureElement and optional Caption.
fn process_figure(
    ctx: &mut ParseContext<'_>,
    element: &ElementRef<'_>,
) -> ruddydoc_core::Result<()> {
    let img_sel = Selector::parse("img").expect("valid selector");
    let caption_sel = Selector::parse("figcaption").expect("valid selector");

    // Find the img inside the figure
    if let Some(img) = element.select(&img_sel).next() {
        let iri = ctx.element_iri("picture");
        ctx.emit_element(&iri, ont::CLASS_PICTURE_ELEMENT)?;

        let alt = img.value().attr("alt").unwrap_or("");
        if !alt.is_empty() {
            ctx.store.insert_literal(
                &iri,
                &ont::iri(ont::PROP_ALT_TEXT),
                alt,
                "string",
                ctx.doc_graph,
            )?;
        }

        let src = img.value().attr("src").unwrap_or("");
        if !src.is_empty() {
            ctx.store.insert_literal(
                &iri,
                &ont::iri(ont::PROP_LINK_TARGET),
                src,
                "string",
                ctx.doc_graph,
            )?;

            // Infer picture format from URL extension
            if let Some(ext) = infer_image_format(src) {
                ctx.store.insert_literal(
                    &iri,
                    &ont::iri(ont::PROP_PICTURE_FORMAT),
                    &ext,
                    "string",
                    ctx.doc_graph,
                )?;
            }
        }

        // Process figcaption if present
        if let Some(caption_el) = element.select(&caption_sel).next() {
            let caption_text = collect_text(&caption_el);
            if !caption_text.is_empty() {
                let caption_iri = ctx.element_iri("caption");
                ctx.emit_element(&caption_iri, ont::CLASS_CAPTION)?;
                ctx.set_text_content(&caption_iri, &caption_text)?;

                // Link picture to caption
                ctx.store.insert_triple_into(
                    &iri,
                    &ont::iri(ont::PROP_HAS_CAPTION),
                    &caption_iri,
                    ctx.doc_graph,
                )?;
            }
        }
    } else {
        // Figure without img -- just emit as a group
        let iri = ctx.element_iri("group");
        ctx.emit_element(&iri, ont::CLASS_GROUP)?;
    }

    Ok(())
}

/// Infer image format from a URL or file path extension.
fn infer_image_format(url: &str) -> Option<String> {
    // Strip query parameters and fragments
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);

    path.rsplit('.')
        .next()
        .map(|s| s.to_lowercase())
        .filter(|ext| {
            matches!(
                ext.as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "tiff" | "bmp"
            )
        })
}

/// Process metadata from the `<head>` element.
fn process_head(ctx: &mut ParseContext<'_>, document: &Html) -> ruddydoc_core::Result<()> {
    let doc_iri = ruddydoc_core::doc_iri(ctx.doc_hash);
    let g = ctx.doc_graph;

    // Extract <title>
    let title_sel = Selector::parse("title").expect("valid selector");
    if let Some(title_el) = document.select(&title_sel).next() {
        let title_text = collect_text(&title_el);
        if !title_text.is_empty() {
            let iri = ctx.element_iri("title");
            ctx.emit_element(&iri, ont::CLASS_TITLE)?;
            ctx.set_text_content(&iri, &title_text)?;
        }
    }

    // Extract <meta> tags
    let meta_sel = Selector::parse("meta").expect("valid selector");
    for meta_el in document.select(&meta_sel) {
        let name = meta_el.value().attr("name").unwrap_or("");
        let content = meta_el.value().attr("content").unwrap_or("");

        if content.is_empty() {
            continue;
        }

        match name.to_lowercase().as_str() {
            "description" => {
                ctx.store.insert_literal(
                    &doc_iri,
                    &ont::iri("description"),
                    content,
                    "string",
                    g,
                )?;
            }
            "author" => {
                ctx.store
                    .insert_literal(&doc_iri, &ont::iri("author"), content, "string", g)?;
            }
            "keywords" => {
                ctx.store
                    .insert_literal(&doc_iri, &ont::iri("keywords"), content, "string", g)?;
            }
            _ => {}
        }
    }

    Ok(())
}

/// Tags whose children we process recursively as structural elements.
const CONTAINER_TAGS: &[&str] = &[
    "body",
    "main",
    "div",
    "section",
    "article",
    "header",
    "footer",
    "blockquote",
    "nav",
    "aside",
    "ul",
    "ol",
];

/// Tags that we handle specially and should NOT be recursed into from the
/// generic child-processing loop.
const SPECIAL_TAGS: &[&str] = &["table", "figure", "pre"];

/// Recursively process a DOM element and its children.
fn process_element(
    ctx: &mut ParseContext<'_>,
    element: &ElementRef<'_>,
) -> ruddydoc_core::Result<()> {
    let tag = element.value().name();

    // Skip script, style, and other non-content tags
    if matches!(tag, "script" | "style" | "noscript" | "template" | "head") {
        return Ok(());
    }

    // Special handling for specific elements
    match tag {
        "table" => {
            return process_table(ctx, element);
        }
        "figure" => {
            return process_figure(ctx, element);
        }
        "pre" if is_code_block(element) => {
            return process_code_block(ctx, element);
        }
        "pre" => {
            // Plain <pre> without <code> -- treat as a paragraph
            let text = collect_text(element);
            if !text.is_empty() {
                let iri = ctx.element_iri("paragraph");
                ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                ctx.set_text_content(&iri, &text)?;
            }
            return Ok(());
        }
        _ => {}
    }

    // Determine if this tag produces an ontology element
    let class = tag_to_class(tag);

    if let Some(class_name) = class {
        match tag {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let text = collect_text(element);
                if !text.is_empty() {
                    let iri = ctx.element_iri("heading");
                    ctx.emit_element(&iri, class_name)?;
                    ctx.set_text_content(&iri, &text)?;
                    if let Some(level) = heading_level(tag) {
                        ctx.store.insert_literal(
                            &iri,
                            &ont::iri(ont::PROP_HEADING_LEVEL),
                            &level.to_string(),
                            "integer",
                            ctx.doc_graph,
                        )?;
                    }
                }
            }
            "p" => {
                let text = collect_text(element);
                if !text.is_empty() {
                    let iri = ctx.element_iri("paragraph");
                    ctx.emit_element(&iri, class_name)?;
                    ctx.set_text_content(&iri, &text)?;
                }
                // Also process inline structural children (e.g. <a> links)
                process_inline_children(ctx, element)?;
            }
            "li" => {
                // Collect only direct text nodes, not text from nested lists
                let text = collect_direct_text(element);
                if !text.is_empty() {
                    let iri = ctx.element_iri("listitem");
                    ctx.emit_element(&iri, class_name)?;
                    ctx.set_text_content(&iri, &text)?;
                }
                // Recurse into structural children (nested lists, etc.)
                process_structural_children(ctx, element)?;
            }
            "img" => {
                let iri = ctx.element_iri("picture");
                ctx.emit_element(&iri, class_name)?;

                let alt = element.value().attr("alt").unwrap_or("");
                if !alt.is_empty() {
                    ctx.store.insert_literal(
                        &iri,
                        &ont::iri(ont::PROP_ALT_TEXT),
                        alt,
                        "string",
                        ctx.doc_graph,
                    )?;
                }

                let src = element.value().attr("src").unwrap_or("");
                if !src.is_empty() {
                    ctx.store.insert_literal(
                        &iri,
                        &ont::iri(ont::PROP_LINK_TARGET),
                        src,
                        "string",
                        ctx.doc_graph,
                    )?;

                    if let Some(ext) = infer_image_format(src) {
                        ctx.store.insert_literal(
                            &iri,
                            &ont::iri(ont::PROP_PICTURE_FORMAT),
                            &ext,
                            "string",
                            ctx.doc_graph,
                        )?;
                    }
                }
            }
            "a" => {
                let iri = ctx.element_iri("link");
                ctx.emit_element(&iri, class_name)?;

                let href = element.value().attr("href").unwrap_or("");
                if !href.is_empty() {
                    ctx.store.insert_literal(
                        &iri,
                        &ont::iri(ont::PROP_LINK_TARGET),
                        href,
                        "string",
                        ctx.doc_graph,
                    )?;
                }

                let text = collect_text(element);
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
            "title" => {
                // Already handled in process_head; skip here if encountered
                // in body to avoid duplication.
            }
            // Container elements: ul, ol, blockquote, section, article, header, footer
            "ul" | "ol" | "blockquote" | "section" | "article" | "header" | "footer" => {
                let iri = ctx.element_iri(tag);
                ctx.emit_element(&iri, class_name)?;

                // Push as parent and recurse into children
                ctx.parent_stack.push(iri);
                process_children(ctx, element)?;
                ctx.parent_stack.pop();
                return Ok(());
            }
            _ => {}
        }
    }

    // If this is a container tag without a specific class, just recurse
    if class.is_none() && is_container(tag) {
        process_children(ctx, element)?;
    }

    Ok(())
}

/// Process the children of a container element.
fn process_children(
    ctx: &mut ParseContext<'_>,
    element: &ElementRef<'_>,
) -> ruddydoc_core::Result<()> {
    for child in element.children() {
        if let Some(child_el) = ElementRef::wrap(child) {
            process_element(ctx, &child_el)?;
        }
    }
    Ok(())
}

/// Process inline children of a text element (e.g., `<a>` links inside `<p>`).
fn process_inline_children(
    ctx: &mut ParseContext<'_>,
    element: &ElementRef<'_>,
) -> ruddydoc_core::Result<()> {
    for child in element.children() {
        if let Some(child_el) = ElementRef::wrap(child) {
            let tag = child_el.value().name();
            if tag == "a" || tag == "img" {
                process_element(ctx, &child_el)?;
            }
        }
    }
    Ok(())
}

/// Process structural children of a list item (e.g., nested `<ul>` or `<ol>`).
fn process_structural_children(
    ctx: &mut ParseContext<'_>,
    element: &ElementRef<'_>,
) -> ruddydoc_core::Result<()> {
    for child in element.children() {
        if let Some(child_el) = ElementRef::wrap(child) {
            let tag = child_el.value().name();
            if matches!(
                tag,
                "ul" | "ol" | "table" | "figure" | "pre" | "blockquote" | "a" | "img"
            ) {
                process_element(ctx, &child_el)?;
            }
        }
    }
    Ok(())
}

/// Collect only direct text nodes from an element, excluding text from nested
/// structural children (like nested lists).
fn collect_direct_text(element: &ElementRef<'_>) -> String {
    use scraper::node::Node;
    let mut text = String::new();
    for child in element.children() {
        match child.value() {
            Node::Text(t) => {
                text.push_str(t);
            }
            Node::Element(el) => {
                // Include text from inline elements but not from structural ones
                let tag = el.name.local.as_ref();
                if !matches!(tag, "ul" | "ol" | "table" | "figure" | "pre" | "blockquote")
                    && let Some(child_el) = ElementRef::wrap(child)
                {
                    text.push_str(&collect_text(&child_el));
                }
            }
            _ => {}
        }
    }
    text.trim().to_string()
}

/// Check if a tag name is a container whose children we recurse into.
fn is_container(tag: &str) -> bool {
    CONTAINER_TAGS.contains(&tag) || SPECIAL_TAGS.contains(&tag)
}

/// Process a `<pre><code>` block as a Code element.
fn process_code_block(
    ctx: &mut ParseContext<'_>,
    element: &ElementRef<'_>,
) -> ruddydoc_core::Result<()> {
    let text = collect_text(element);
    let iri = ctx.element_iri("code");
    ctx.emit_element(&iri, ont::CLASS_CODE)?;
    ctx.set_text_content(&iri, &text)?;

    if let Some(lang) = extract_code_language(element) {
        ctx.store.insert_literal(
            &iri,
            &ont::iri(ont::PROP_CODE_LANGUAGE),
            &lang,
            "string",
            ctx.doc_graph,
        )?;
    }

    Ok(())
}

impl DocumentBackend for HtmlBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Html]
    }

    fn supports_pagination(&self) -> bool {
        false
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("html" | "htm" | "xhtml")
                )
            }
            DocumentSource::Stream { name, .. } => {
                name.ends_with(".html") || name.ends_with(".htm") || name.ends_with(".xhtml")
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
            "html",
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

        // Parse the HTML
        let document = Html::parse_document(&content);

        let mut ctx = ParseContext::new(store, g, &hash_str);

        // Process <head> metadata first
        process_head(&mut ctx, &document)?;

        // Process <body> content (or the whole document for fragments)
        let body_sel = Selector::parse("body").expect("valid selector");
        if let Some(body) = document.select(&body_sel).next() {
            process_children(&mut ctx, &body)?;
        } else {
            // HTML fragment: process root children directly
            let html_sel = Selector::parse("html").expect("valid selector");
            if let Some(html_root) = document.select(&html_sel).next() {
                process_children(&mut ctx, &html_root)?;
            } else {
                // Raw fragment: process all root children
                for child in document.tree.root().children() {
                    if let Some(child_el) = ElementRef::wrap(child) {
                        process_element(&mut ctx, &child_el)?;
                    }
                }
            }
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Html,
            file_size,
            page_count: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;

    fn parse_html(html: &str) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = HtmlBackend::new();
        let source = DocumentSource::Stream {
            name: "test.html".to_string(),
            data: html.as_bytes().to_vec(),
        };

        let hash_str = compute_hash(html.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    #[test]
    fn parse_heading() -> ruddydoc_core::Result<()> {
        let html = "<html><body><h1>Hello World</h1></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

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
    fn parse_multiple_headings() -> ruddydoc_core::Result<()> {
        let html = "<html><body><h1>H1</h1><h2>H2</h2><h3>H3</h3></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

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
    fn parse_paragraph() -> ruddydoc_core::Result<()> {
        let html = "<html><body><p>This is a paragraph.</p></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

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
    fn parse_unordered_list() -> ruddydoc_core::Result<()> {
        let html = "<html><body><ul><li>Item one</li><li>Item two</li><li>Item three</li></ul></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

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
        let html = "<html><body><ol><li>First</li><li>Second</li></ol></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

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
        let html = r#"<html><body>
            <table>
                <tr><th>Name</th><th>Age</th></tr>
                <tr><td>Alice</td><td>30</td></tr>
                <tr><td>Bob</td><td>25</td></tr>
            </table>
        </body></html>"#;
        let (store, _meta, graph) = parse_html(html)?;

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
    fn parse_table_with_colspan_rowspan() -> ruddydoc_core::Result<()> {
        let html = r#"<html><body>
            <table>
                <tr><th colspan="2">Full Width Header</th></tr>
                <tr><td rowspan="2">Spans 2 rows</td><td>Normal</td></tr>
                <tr><td>Below</td></tr>
            </table>
        </body></html>"#;
        let (store, _meta, graph) = parse_html(html)?;

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

        // Find the cell with colspan=2
        let colspan_cell = rows
            .iter()
            .find(|r| {
                r["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("Full Width Header"))
            })
            .expect("expected Full Width Header cell");
        let cs = colspan_cell["cs"].as_str().expect("cs");
        assert!(cs.contains('2'));

        // Find the cell with rowspan=2
        let sparql_rs = format!(
            "SELECT ?text ?rs WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> ?rs \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW_SPAN),
        );
        let result_rs = store.query_to_json(&sparql_rs)?;
        let rows_rs = result_rs.as_array().expect("expected array");

        let rowspan_cell = rows_rs
            .iter()
            .find(|r| {
                r["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("Spans 2 rows"))
            })
            .expect("expected Spans 2 rows cell");
        let rs = rowspan_cell["rs"].as_str().expect("rs");
        assert!(rs.contains('2'));

        Ok(())
    }

    #[test]
    fn parse_image() -> ruddydoc_core::Result<()> {
        let html = r#"<html><body><img src="image.png" alt="Alt text"></body></html>"#;
        let (store, _meta, graph) = parse_html(html)?;

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

        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("png"));

        Ok(())
    }

    #[test]
    fn parse_code_block() -> ruddydoc_core::Result<()> {
        let html = r#"<html><body><pre><code class="language-rust">fn main() {}</code></pre></body></html>"#;
        let (store, _meta, graph) = parse_html(html)?;

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
    fn parse_blockquote() -> ruddydoc_core::Result<()> {
        let html = "<html><body><blockquote><p>This is a quote.</p></blockquote></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?g a <{}> }} }}",
            ont::iri(ont::CLASS_GROUP),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn parse_hyperlink() -> ruddydoc_core::Result<()> {
        let html =
            r#"<html><body><p><a href="https://example.com">Example Link</a></p></body></html>"#;
        let (store, _meta, graph) = parse_html(html)?;

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

    #[test]
    fn parse_figure_with_caption() -> ruddydoc_core::Result<()> {
        let html = r#"<html><body>
            <figure>
                <img src="photo.jpg" alt="A photo">
                <figcaption>This is a caption</figcaption>
            </figure>
        </body></html>"#;
        let (store, _meta, graph) = parse_html(html)?;

        // Check picture element
        let sparql_pic = format!(
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
        let result = store.query_to_json(&sparql_pic)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        // Check caption
        let sparql_cap = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_CAPTION),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result_cap = store.query_to_json(&sparql_cap)?;
        let cap_rows = result_cap.as_array().expect("expected array");
        assert_eq!(cap_rows.len(), 1);

        let cap_text = cap_rows[0]["text"].as_str().expect("text");
        assert!(cap_text.contains("This is a caption"));

        // Check hasCaption link
        let sparql_link = format!(
            "ASK {{ GRAPH <{graph}> {{ ?pic <{}> ?cap }} }}",
            ont::iri(ont::PROP_HAS_CAPTION),
        );
        let result_link = store.query_to_json(&sparql_link)?;
        assert_eq!(result_link, serde_json::Value::Bool(true));

        Ok(())
    }

    #[test]
    fn parse_head_metadata() -> ruddydoc_core::Result<()> {
        let html = r#"<html>
            <head>
                <title>My Page Title</title>
                <meta name="description" content="A test page">
                <meta name="author" content="Test Author">
                <meta name="keywords" content="rust, html, parser">
            </head>
            <body><p>Content</p></body>
        </html>"#;
        let (store, meta, graph) = parse_html(html)?;

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);

        // Check title element
        let sparql_title = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_TITLE),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql_title)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let title_text = rows[0]["text"].as_str().expect("text");
        assert!(title_text.contains("My Page Title"));

        // Check description metadata
        let sparql_desc = format!(
            "SELECT ?desc WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?desc \
               }} \
             }}",
            ont::iri("description"),
        );
        let result_desc = store.query_to_json(&sparql_desc)?;
        let desc_rows = result_desc.as_array().expect("expected array");
        assert_eq!(desc_rows.len(), 1);
        let desc = desc_rows[0]["desc"].as_str().expect("desc");
        assert!(desc.contains("A test page"));

        // Check author metadata
        let sparql_author = format!(
            "SELECT ?author WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?author \
               }} \
             }}",
            ont::iri("author"),
        );
        let result_author = store.query_to_json(&sparql_author)?;
        let author_rows = result_author.as_array().expect("expected array");
        assert_eq!(author_rows.len(), 1);
        let author = author_rows[0]["author"].as_str().expect("author");
        assert!(author.contains("Test Author"));

        // Check keywords metadata
        let sparql_kw = format!(
            "SELECT ?kw WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?kw \
               }} \
             }}",
            ont::iri("keywords"),
        );
        let result_kw = store.query_to_json(&sparql_kw)?;
        let kw_rows = result_kw.as_array().expect("expected array");
        assert_eq!(kw_rows.len(), 1);
        let kw = kw_rows[0]["kw"].as_str().expect("kw");
        assert!(kw.contains("rust, html, parser"));

        Ok(())
    }

    #[test]
    fn reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let html = "<html><body><h1>Heading</h1><p>Para one.</p><p>Para two.</p></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

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
    fn parent_child_links() -> ruddydoc_core::Result<()> {
        let html = "<html><body><ul><li>Item one</li><li>Item two</li></ul></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

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

    #[test]
    fn document_metadata() -> ruddydoc_core::Result<()> {
        let html = "<html><body><h1>Test</h1></body></html>";
        let (store, meta, graph) = parse_html(html)?;

        assert_eq!(meta.format, InputFormat::Html);
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
    fn parse_nested_lists() -> ruddydoc_core::Result<()> {
        let html = r#"<html><body>
            <ul>
                <li>Outer item</li>
                <li>Outer with nested
                    <ul>
                        <li>Inner item 1</li>
                        <li>Inner item 2</li>
                    </ul>
                </li>
            </ul>
        </body></html>"#;
        let (store, _meta, graph) = parse_html(html)?;

        // Should have 2 unordered lists (outer + inner)
        let sparql = format!(
            "SELECT ?l WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?l a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_UNORDERED_LIST),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);

        // Should have list items (at least 3: outer + 2 inner, depending on text extraction)
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
        let result_items = store.query_to_json(&sparql_items)?;
        let item_rows = result_items.as_array().expect("expected array");
        assert!(item_rows.len() >= 3);
        Ok(())
    }

    #[test]
    fn parse_header_footer() -> ruddydoc_core::Result<()> {
        let html = r#"<html><body>
            <header><p>Header content</p></header>
            <p>Main content</p>
            <footer><p>Footer content</p></footer>
        </body></html>"#;
        let (store, _meta, graph) = parse_html(html)?;

        // Check PageHeader
        let sparql_header = format!(
            "ASK {{ GRAPH <{graph}> {{ ?h a <{}> }} }}",
            ont::iri(ont::CLASS_PAGE_HEADER),
        );
        let result = store.query_to_json(&sparql_header)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check PageFooter
        let sparql_footer = format!(
            "ASK {{ GRAPH <{graph}> {{ ?f a <{}> }} }}",
            ont::iri(ont::CLASS_PAGE_FOOTER),
        );
        let result_f = store.query_to_json(&sparql_footer)?;
        assert_eq!(result_f, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn parse_section_and_article() -> ruddydoc_core::Result<()> {
        let html = r#"<html><body>
            <article>
                <h1>Article Title</h1>
                <section>
                    <h2>Section Title</h2>
                    <p>Section content</p>
                </section>
            </article>
        </body></html>"#;
        let (store, _meta, graph) = parse_html(html)?;

        // Both article and section should create Group elements
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
        assert_eq!(rows.len(), 2); // article + section
        Ok(())
    }

    #[test]
    fn parse_html_fragment() -> ruddydoc_core::Result<()> {
        // A fragment without full <html><body> wrapper
        let html = "<h1>Fragment Heading</h1><p>Fragment paragraph.</p>";
        let (store, _meta, graph) = parse_html(html)?;

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
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Fragment Heading"));
        Ok(())
    }

    #[test]
    fn is_valid_accepts_html_extensions() {
        let backend = HtmlBackend::new();

        assert!(backend.is_valid(&DocumentSource::File("test.html".into())));
        assert!(backend.is_valid(&DocumentSource::File("test.htm".into())));
        assert!(backend.is_valid(&DocumentSource::File("test.xhtml".into())));
        assert!(!backend.is_valid(&DocumentSource::File("test.md".into())));

        assert!(backend.is_valid(&DocumentSource::Stream {
            name: "test.html".to_string(),
            data: vec![],
        }));
        assert!(backend.is_valid(&DocumentSource::Stream {
            name: "test.xhtml".to_string(),
            data: vec![],
        }));
        assert!(!backend.is_valid(&DocumentSource::Stream {
            name: "test.md".to_string(),
            data: vec![],
        }));
    }

    #[test]
    fn next_previous_sibling_links() -> ruddydoc_core::Result<()> {
        let html = "<html><body><p>First</p><p>Second</p><p>Third</p></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

        // First paragraph should have a next link
        let sparql = format!(
            "SELECT ?cur ?next WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?cur a <{cls}>. \
                 ?cur <{next}> ?next. \
                 ?next a <{cls}> \
               }} \
             }}",
            cls = ont::iri(ont::CLASS_PARAGRAPH),
            next = ont::iri(ont::PROP_NEXT_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        // Should have 2 next links: first->second, second->third
        assert_eq!(rows.len(), 2);

        // Check previous links
        let sparql_prev = format!(
            "SELECT ?cur ?prev WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?cur a <{cls}>. \
                 ?cur <{prev}> ?prev. \
                 ?prev a <{cls}> \
               }} \
             }}",
            cls = ont::iri(ont::CLASS_PARAGRAPH),
            prev = ont::iri(ont::PROP_PREVIOUS_ELEMENT),
        );
        let result_prev = store.query_to_json(&sparql_prev)?;
        let prev_rows = result_prev.as_array().expect("expected array");
        assert_eq!(prev_rows.len(), 2);

        Ok(())
    }

    #[test]
    fn parse_code_block_no_language() -> ruddydoc_core::Result<()> {
        let html = "<html><body><pre><code>plain code</code></pre></body></html>";
        let (store, _meta, graph) = parse_html(html)?;

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
        assert!(text.contains("plain code"));

        // Should NOT have a language
        let sparql_lang = format!(
            "ASK {{ GRAPH <{graph}> {{ ?c <{}> ?lang }} }}",
            ont::iri(ont::PROP_CODE_LANGUAGE),
        );
        let result_lang = store.query_to_json(&sparql_lang)?;
        assert_eq!(result_lang, serde_json::Value::Bool(false));

        Ok(())
    }

    #[test]
    fn hash_is_deterministic() {
        let data = b"hello world";
        let h1 = compute_hash(data);
        let h2 = compute_hash(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex is 64 chars
    }

    #[test]
    fn default_impl_works() {
        let backend = HtmlBackend::default();
        assert_eq!(backend.supported_formats(), &[InputFormat::Html]);
        assert!(!backend.supports_pagination());
    }

    #[test]
    fn parse_complex_page() -> ruddydoc_core::Result<()> {
        let html = r#"<!DOCTYPE html>
<html>
<head>
    <title>Complex Page</title>
    <meta name="description" content="A complex test page">
</head>
<body>
    <header><p>Site Header</p></header>
    <h1>Main Title</h1>
    <p>Introduction paragraph.</p>
    <h2>Section One</h2>
    <p>Content of section one.</p>
    <ul>
        <li>List item A</li>
        <li>List item B</li>
    </ul>
    <h2>Section Two</h2>
    <pre><code class="language-python">print("hello")</code></pre>
    <table>
        <tr><th>Col 1</th><th>Col 2</th></tr>
        <tr><td>Data 1</td><td>Data 2</td></tr>
    </table>
    <img src="photo.jpg" alt="A photo">
    <footer><p>Site Footer</p></footer>
</body>
</html>"#;
        let (store, meta, graph) = parse_html(html)?;

        assert_eq!(meta.format, InputFormat::Html);

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

        // Title + header + h1 + p + h2 + p + ul + 2 li + h2 + code + table + img + footer
        // = 14 elements minimum
        assert!(
            rows.len() >= 10,
            "expected at least 10 elements, got {}",
            rows.len()
        );

        // Verify reading order values are sequential and start at 0
        for (i, row) in rows.iter().enumerate() {
            let order = row["order"].as_str().expect("order");
            // The order contains the integer value as a string
            let expected = format!("\"{i}\"");
            assert!(
                order.contains(&i.to_string()),
                "expected reading order {i} in {order}, at position {i} (expected contains {})",
                expected
            );
        }

        Ok(())
    }
}
