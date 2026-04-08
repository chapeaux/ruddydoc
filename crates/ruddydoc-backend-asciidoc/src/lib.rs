//! AsciiDoc parser backend for RuddyDoc.
//!
//! Custom line-based parser for AsciiDoc documents. Maps AsciiDoc
//! structural elements to the RuddyDoc document ontology graph.

use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

/// AsciiDoc document backend.
pub struct AsciiDocBackend;

impl AsciiDocBackend {
    /// Create a new AsciiDoc backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsciiDocBackend {
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

/// Parse context used during graph construction.
struct ParseContext<'a> {
    store: &'a dyn DocumentStore,
    doc_graph: &'a str,
    doc_hash: &'a str,
    reading_order: usize,
    parent_stack: Vec<String>,
    last_sibling_at_depth: Vec<Option<String>>,
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

/// Detect the heading level from a line starting with `=` signs.
/// `= Title` -> level 0 (document title, mapped to Title class)
/// `== Section` -> level 1
/// `=== Subsection` -> level 2
/// etc.
fn detect_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('=') {
        return None;
    }

    let eq_count = trimmed.chars().take_while(|&c| c == '=').count();
    if eq_count == 0 || eq_count > 6 {
        return None;
    }

    let rest = &trimmed[eq_count..];
    // Must have a space after the equals signs
    if !rest.starts_with(' ') {
        return None;
    }

    let text = rest.trim().to_string();
    if text.is_empty() {
        return None;
    }

    Some((eq_count, text))
}

/// Check if a line is an unordered list item (starts with `* ` or `- `).
fn detect_unordered_list_item(line: &str) -> Option<String> {
    let trimmed = line.trim();
    trimmed
        .strip_prefix("* ")
        .or_else(|| trimmed.strip_prefix("- "))
        .map(|rest| rest.trim().to_string())
}

/// Check if a line is an ordered list item (starts with `. `).
fn detect_ordered_list_item(line: &str) -> Option<String> {
    let trimmed = line.trim();
    trimmed
        .strip_prefix(". ")
        .map(|rest| rest.trim().to_string())
}

/// Check if a line is a block delimiter.
fn is_block_delimiter(line: &str, delimiter: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 4
        && trimmed
            .chars()
            .all(|c| c == delimiter.chars().next().unwrap_or(' '))
        && trimmed.starts_with(delimiter)
}

/// Check if a line is a listing block delimiter (----).
fn is_listing_delimiter(line: &str) -> bool {
    is_block_delimiter(line, "----")
}

/// Check if a line is a quote block delimiter (____).
fn is_quote_delimiter(line: &str) -> bool {
    is_block_delimiter(line, "____")
}

/// Check if a line is an example block delimiter (====).
fn is_example_delimiter(line: &str) -> bool {
    let trimmed = line.trim();
    // Must not be a heading (headings have a space after `=` signs)
    trimmed.len() >= 4 && trimmed.chars().all(|c| c == '=')
}

/// Check if a line is a sidebar block delimiter (****).
fn is_sidebar_delimiter(line: &str) -> bool {
    is_block_delimiter(line, "****")
}

/// Check if a line is a table delimiter.
fn is_table_delimiter(line: &str) -> bool {
    line.trim() == "|==="
}

/// Check if a line is an attribute line (`:key: value`).
fn is_attribute_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with(':')
        && trimmed.len() > 2
        && trimmed[1..].contains(':')
        && !trimmed[1..].starts_with(':')
}

/// Check if a line is an include directive.
fn is_include_directive(line: &str) -> bool {
    line.trim().starts_with("include::")
}

/// Check if a line is an image macro.
fn detect_image_macro(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("image::") {
        // image::path[alt text]
        if let Some(bracket_start) = rest.find('[') {
            let path = rest[..bracket_start].to_string();
            let alt = if let Some(bracket_end) = rest.find(']') {
                rest[bracket_start + 1..bracket_end].to_string()
            } else {
                String::new()
            };
            return Some((path, alt));
        }
    }
    None
}

/// Check if a line is an attribute/annotation line like `[source,rust]`, `[NOTE]`, etc.
fn detect_attribute_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() > 2 {
        Some(trimmed[1..trimmed.len() - 1].to_string())
    } else {
        None
    }
}

/// Parse a table block into rows of cells.
fn parse_table_rows(lines: &[&str]) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Each table row line starts with `|`
        if !trimmed.starts_with('|') {
            continue;
        }

        let cells: Vec<String> = trimmed
            .split('|')
            .skip(1) // skip the empty string before the first `|`
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty() || trimmed.ends_with('|'))
            .collect();

        if !cells.is_empty() {
            rows.push(cells);
        }
    }

    rows
}

/// State for tracking what kind of list is currently being built.
#[derive(Clone, Copy, PartialEq)]
enum ListKind {
    Ordered,
    Unordered,
}

impl DocumentBackend for AsciiDocBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::AsciiDoc]
    }

    fn supports_pagination(&self) -> bool {
        false
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("adoc" | "asciidoc" | "asc")
                )
            }
            DocumentSource::Stream { name, .. } => {
                name.ends_with(".adoc") || name.ends_with(".asciidoc") || name.ends_with(".asc")
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
            "asciidoc",
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

        let mut ctx = ParseContext::new(store, g, &hash_str);
        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        // Pending attribute (e.g., [source,rust] before a code block)
        let mut pending_attr: Option<String> = None;
        // Track current list context for closing list groups
        let mut current_list: Option<(ListKind, String)> = None;

        while i < lines.len() {
            let line = lines[i];

            // Skip blank lines (and close any open list)
            if line.trim().is_empty() {
                if current_list.is_some() {
                    current_list = None;
                    ctx.parent_stack.pop();
                }
                i += 1;
                continue;
            }

            // Skip attribute header lines (:key: value)
            if is_attribute_line(line) {
                i += 1;
                continue;
            }

            // Skip include directives
            if is_include_directive(line) {
                i += 1;
                continue;
            }

            // Check for attribute/annotation line [...]
            if let Some(attr) = detect_attribute_line(line) {
                pending_attr = Some(attr);
                i += 1;
                continue;
            }

            // Check for image macro
            if let Some((path, alt)) = detect_image_macro(line) {
                // Close any open list
                if current_list.is_some() {
                    current_list = None;
                    ctx.parent_stack.pop();
                }

                let iri = ctx.element_iri("picture");
                ctx.emit_element(&iri, ont::CLASS_PICTURE_ELEMENT)?;

                if !alt.is_empty() {
                    store.insert_literal(&iri, &ont::iri(ont::PROP_ALT_TEXT), &alt, "string", g)?;
                }
                if !path.is_empty() {
                    store.insert_literal(
                        &iri,
                        &ont::iri(ont::PROP_LINK_TARGET),
                        &path,
                        "string",
                        g,
                    )?;
                }

                pending_attr = None;
                i += 1;
                continue;
            }

            // Check for headings
            if let Some((eq_count, text)) = detect_heading(line) {
                // Close any open list
                if current_list.is_some() {
                    current_list = None;
                    ctx.parent_stack.pop();
                }

                if eq_count == 1 {
                    // Document title
                    let iri = ctx.element_iri("title");
                    ctx.emit_element(&iri, ont::CLASS_TITLE)?;
                    ctx.set_text_content(&iri, &text)?;
                } else {
                    // Section header: level = eq_count - 1
                    let level = eq_count - 1;
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

                pending_attr = None;
                i += 1;
                continue;
            }

            // Check for table delimiter
            if is_table_delimiter(line) {
                // Close any open list
                if current_list.is_some() {
                    current_list = None;
                    ctx.parent_stack.pop();
                }

                // Collect table lines until the closing |===
                i += 1;
                let mut table_lines = Vec::new();
                while i < lines.len() && !is_table_delimiter(lines[i]) {
                    table_lines.push(lines[i]);
                    i += 1;
                }
                if i < lines.len() {
                    i += 1; // skip closing |===
                }

                let rows = parse_table_rows(&table_lines);
                let table_iri = ctx.element_iri("table");
                ctx.emit_element(&table_iri, ont::CLASS_TABLE_ELEMENT)?;

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
                    for (col_idx, cell_text) in row.iter().enumerate() {
                        let cell_iri = ruddydoc_core::element_iri(
                            &hash_str,
                            &format!("cell-{row_idx}-{col_idx}"),
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
                            cell_text,
                            "string",
                            g,
                        )?;
                        store.insert_literal(
                            &cell_iri,
                            &ont::iri(ont::PROP_IS_HEADER),
                            if row_idx == 0 { "true" } else { "false" },
                            "boolean",
                            g,
                        )?;
                    }
                }

                pending_attr = None;
                continue;
            }

            // Check for listing block delimiter (----)
            if is_listing_delimiter(line) {
                // Close any open list
                if current_list.is_some() {
                    current_list = None;
                    ctx.parent_stack.pop();
                }

                // Collect code block content
                i += 1;
                let mut code_lines = Vec::new();
                while i < lines.len() && !is_listing_delimiter(lines[i]) {
                    code_lines.push(lines[i]);
                    i += 1;
                }
                if i < lines.len() {
                    i += 1; // skip closing ----
                }

                let code_text = code_lines.join("\n");
                let iri = ctx.element_iri("code");
                ctx.emit_element(&iri, ont::CLASS_CODE)?;
                ctx.set_text_content(&iri, &code_text)?;

                // Check if pending attribute specifies a language
                if let Some(attr) = pending_attr.take()
                    && let Some(lang) = extract_source_language(&attr)
                {
                    store.insert_literal(
                        &iri,
                        &ont::iri(ont::PROP_CODE_LANGUAGE),
                        &lang,
                        "string",
                        g,
                    )?;
                }

                continue;
            }

            // Check for quote block delimiter (____)
            if is_quote_delimiter(line) {
                // Close any open list
                if current_list.is_some() {
                    current_list = None;
                    ctx.parent_stack.pop();
                }

                let group_iri = ctx.element_iri("group");
                ctx.emit_element(&group_iri, ont::CLASS_GROUP)?;
                ctx.parent_stack.push(group_iri);

                // Collect quote content
                i += 1;
                while i < lines.len() && !is_quote_delimiter(lines[i]) {
                    let qline = lines[i].trim();
                    if !qline.is_empty() {
                        let p_iri = ctx.element_iri("paragraph");
                        ctx.emit_element(&p_iri, ont::CLASS_PARAGRAPH)?;
                        ctx.set_text_content(&p_iri, qline)?;
                    }
                    i += 1;
                }
                if i < lines.len() {
                    i += 1; // skip closing ____
                }

                ctx.parent_stack.pop();
                pending_attr = None;
                continue;
            }

            // Check for example block delimiter (====)
            if is_example_delimiter(line) {
                // Close any open list
                if current_list.is_some() {
                    current_list = None;
                    ctx.parent_stack.pop();
                }

                let group_iri = ctx.element_iri("group");
                ctx.emit_element(&group_iri, ont::CLASS_GROUP)?;
                ctx.parent_stack.push(group_iri);

                i += 1;
                while i < lines.len() && !is_example_delimiter(lines[i]) {
                    let eline = lines[i].trim();
                    if !eline.is_empty() {
                        let p_iri = ctx.element_iri("paragraph");
                        ctx.emit_element(&p_iri, ont::CLASS_PARAGRAPH)?;
                        ctx.set_text_content(&p_iri, eline)?;
                    }
                    i += 1;
                }
                if i < lines.len() {
                    i += 1; // skip closing ====
                }

                ctx.parent_stack.pop();
                pending_attr = None;
                continue;
            }

            // Check for sidebar block delimiter (****)
            if is_sidebar_delimiter(line) {
                // Close any open list
                if current_list.is_some() {
                    current_list = None;
                    ctx.parent_stack.pop();
                }

                let group_iri = ctx.element_iri("group");
                ctx.emit_element(&group_iri, ont::CLASS_GROUP)?;
                ctx.parent_stack.push(group_iri);

                i += 1;
                while i < lines.len() && !is_sidebar_delimiter(lines[i]) {
                    let sline = lines[i].trim();
                    if !sline.is_empty() {
                        let p_iri = ctx.element_iri("paragraph");
                        ctx.emit_element(&p_iri, ont::CLASS_PARAGRAPH)?;
                        ctx.set_text_content(&p_iri, sline)?;
                    }
                    i += 1;
                }
                if i < lines.len() {
                    i += 1; // skip closing ****
                }

                ctx.parent_stack.pop();
                pending_attr = None;
                continue;
            }

            // Check for admonition blocks [NOTE], [TIP], [WARNING], etc.
            // These are handled via pending_attr + next paragraph
            if let Some(ref attr) = pending_attr {
                let upper = attr.to_uppercase();
                if matches!(
                    upper.as_str(),
                    "NOTE" | "TIP" | "WARNING" | "IMPORTANT" | "CAUTION"
                ) {
                    // Emit a Group for the admonition, containing the text as a paragraph
                    // Close any open list
                    if current_list.is_some() {
                        current_list = None;
                        ctx.parent_stack.pop();
                    }

                    let group_iri = ctx.element_iri("group");
                    ctx.emit_element(&group_iri, ont::CLASS_GROUP)?;
                    ctx.parent_stack.push(group_iri.clone());

                    // Collect paragraph lines
                    let mut para_lines = Vec::new();
                    while i < lines.len() && !lines[i].trim().is_empty() {
                        para_lines.push(lines[i].trim());
                        i += 1;
                    }

                    let para_text = para_lines.join(" ");
                    if !para_text.is_empty() {
                        let p_iri = ctx.element_iri("paragraph");
                        ctx.emit_element(&p_iri, ont::CLASS_PARAGRAPH)?;
                        ctx.set_text_content(&p_iri, &para_text)?;
                    }

                    ctx.parent_stack.pop();
                    pending_attr = None;
                    continue;
                }
            }

            // Check for unordered list item
            if let Some(item_text) = detect_unordered_list_item(line) {
                // Start a new list if needed or if list type changed
                match &current_list {
                    Some((ListKind::Unordered, _)) => {
                        // already in an unordered list, keep going
                    }
                    Some((ListKind::Ordered, _)) => {
                        // Close ordered list, start unordered
                        ctx.parent_stack.pop();

                        let list_iri = ctx.element_iri("list");
                        ctx.emit_element(&list_iri, ont::CLASS_UNORDERED_LIST)?;
                        ctx.parent_stack.push(list_iri.clone());
                        current_list = Some((ListKind::Unordered, list_iri));
                    }
                    None => {
                        let list_iri = ctx.element_iri("list");
                        ctx.emit_element(&list_iri, ont::CLASS_UNORDERED_LIST)?;
                        ctx.parent_stack.push(list_iri.clone());
                        current_list = Some((ListKind::Unordered, list_iri));
                    }
                }

                let item_iri = ctx.element_iri("listitem");
                ctx.emit_element(&item_iri, ont::CLASS_LIST_ITEM)?;
                ctx.set_text_content(&item_iri, &item_text)?;

                pending_attr = None;
                i += 1;
                continue;
            }

            // Check for ordered list item
            if let Some(item_text) = detect_ordered_list_item(line) {
                match &current_list {
                    Some((ListKind::Ordered, _)) => {
                        // already in an ordered list
                    }
                    Some((ListKind::Unordered, _)) => {
                        // Close unordered list, start ordered
                        ctx.parent_stack.pop();

                        let list_iri = ctx.element_iri("list");
                        ctx.emit_element(&list_iri, ont::CLASS_ORDERED_LIST)?;
                        ctx.parent_stack.push(list_iri.clone());
                        current_list = Some((ListKind::Ordered, list_iri));
                    }
                    None => {
                        let list_iri = ctx.element_iri("list");
                        ctx.emit_element(&list_iri, ont::CLASS_ORDERED_LIST)?;
                        ctx.parent_stack.push(list_iri.clone());
                        current_list = Some((ListKind::Ordered, list_iri));
                    }
                }

                let item_iri = ctx.element_iri("listitem");
                ctx.emit_element(&item_iri, ont::CLASS_LIST_ITEM)?;
                ctx.set_text_content(&item_iri, &item_text)?;

                pending_attr = None;
                i += 1;
                continue;
            }

            // Default: plain text paragraph
            // Close any open list
            if current_list.is_some() {
                current_list = None;
                ctx.parent_stack.pop();
            }

            // Collect contiguous non-blank, non-structural lines as a paragraph
            let mut para_lines = Vec::new();
            while i < lines.len() {
                let pline = lines[i];
                if pline.trim().is_empty()
                    || detect_heading(pline).is_some()
                    || detect_unordered_list_item(pline).is_some()
                    || detect_ordered_list_item(pline).is_some()
                    || is_listing_delimiter(pline)
                    || is_quote_delimiter(pline)
                    || is_example_delimiter(pline)
                    || is_sidebar_delimiter(pline)
                    || is_table_delimiter(pline)
                    || detect_image_macro(pline).is_some()
                    || detect_attribute_line(pline).is_some()
                    || is_attribute_line(pline)
                    || is_include_directive(pline)
                {
                    break;
                }
                para_lines.push(pline.trim());
                i += 1;
            }

            if !para_lines.is_empty() {
                let para_text = para_lines.join(" ");
                let iri = ctx.element_iri("paragraph");
                ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                ctx.set_text_content(&iri, &para_text)?;
            }

            pending_attr = None;
        }

        // Close any open list at end of document
        if current_list.is_some() {
            ctx.parent_stack.pop();
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::AsciiDoc,
            file_size,
            page_count: None,
        })
    }
}

/// Extract the programming language from a `[source,lang]` attribute.
fn extract_source_language(attr: &str) -> Option<String> {
    if attr.starts_with("source") {
        let parts: Vec<&str> = attr.splitn(2, ',').collect();
        if parts.len() == 2 {
            let lang = parts[1].trim().to_string();
            if !lang.is_empty() {
                return Some(lang);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;

    fn parse_asciidoc(adoc: &str) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = AsciiDocBackend::new();
        let source = DocumentSource::Stream {
            name: "test.adoc".to_string(),
            data: adoc.as_bytes().to_vec(),
        };

        let hash_str = compute_hash(adoc.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    #[test]
    fn parse_document_title() -> ruddydoc_core::Result<()> {
        let adoc = "= My Document Title\n";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

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
        assert!(text.contains("My Document Title"));

        Ok(())
    }

    #[test]
    fn parse_section_headings() -> ruddydoc_core::Result<()> {
        let adoc = "== Section One\n\n=== Subsection\n\n==== Sub-subsection\n";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

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

        // Level 1, 2, 3
        let level0 = rows[0]["level"].as_str().expect("level");
        assert!(level0.contains('1'));

        let level1 = rows[1]["level"].as_str().expect("level");
        assert!(level1.contains('2'));

        let level2 = rows[2]["level"].as_str().expect("level");
        assert!(level2.contains('3'));

        Ok(())
    }

    #[test]
    fn parse_paragraphs() -> ruddydoc_core::Result<()> {
        let adoc = "\
= Title

This is the first paragraph.

This is the second paragraph.
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

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
    fn parse_unordered_list() -> ruddydoc_core::Result<()> {
        let adoc = "\
* Item one
* Item two
* Item three
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

        // Check UnorderedList exists
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
        let adoc = "\
. First
. Second
. Third
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

        // Check OrderedList exists
        let sparql_list = format!(
            "ASK {{ GRAPH <{graph}> {{ ?l a <{}> }} }}",
            ont::iri(ont::CLASS_ORDERED_LIST),
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
    fn parse_code_block_with_language() -> ruddydoc_core::Result<()> {
        let adoc = "\
[source,rust]
----
fn main() {
    println!(\"Hello\");
}
----
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

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
    fn parse_code_block_without_language() -> ruddydoc_core::Result<()> {
        let adoc = "\
----
some listing content
----
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

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
        assert!(text.contains("some listing content"));

        // No language should be set
        let sparql_lang = format!(
            "ASK {{ GRAPH <{graph}> {{ ?c <{}> ?lang }} }}",
            ont::iri(ont::PROP_CODE_LANGUAGE),
        );
        let result = store.query_to_json(&sparql_lang)?;
        assert_eq!(result, serde_json::Value::Bool(false));

        Ok(())
    }

    #[test]
    fn parse_table() -> ruddydoc_core::Result<()> {
        let adoc = "\
|===
| Name | Age
| Alice | 30
| Bob | 25
|===
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

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
        // 3 rows * 2 cols = 6 cells
        assert_eq!(rows.len(), 6);

        Ok(())
    }

    #[test]
    fn parse_image_macro() -> ruddydoc_core::Result<()> {
        let adoc = "image::screenshot.png[A screenshot of the app]\n";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

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
        assert!(alt.contains("A screenshot of the app"));

        let target = rows[0]["target"].as_str().expect("target");
        assert!(target.contains("screenshot.png"));

        Ok(())
    }

    #[test]
    fn parse_quote_block() -> ruddydoc_core::Result<()> {
        let adoc = "\
____
This is a quoted paragraph.
____
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

        // Should have a Group
        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?g a <{}> }} }}",
            ont::iri(ont::CLASS_GROUP),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // The paragraph inside should be a child of the group
        let sparql_child = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?g a <{}>. \
                 ?g <{}> ?p. \
                 ?p <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_GROUP),
            ont::iri(ont::PROP_CHILD_ELEMENT),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql_child)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("quoted paragraph"));

        Ok(())
    }

    #[test]
    fn reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let adoc = "\
= Title

First paragraph.

== Section

Second paragraph.
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

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
        // Title + paragraph + heading + paragraph = 4
        assert_eq!(rows.len(), 4);

        Ok(())
    }

    #[test]
    fn document_metadata() -> ruddydoc_core::Result<()> {
        let adoc = "= Test\n";
        let (store, meta, graph) = parse_asciidoc(adoc)?;

        assert_eq!(meta.format, InputFormat::AsciiDoc);
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

    #[test]
    fn is_valid_checks_extension() {
        let backend = AsciiDocBackend::new();

        let valid_adoc = DocumentSource::File(std::path::PathBuf::from("doc.adoc"));
        assert!(backend.is_valid(&valid_adoc));

        let valid_asciidoc = DocumentSource::File(std::path::PathBuf::from("doc.asciidoc"));
        assert!(backend.is_valid(&valid_asciidoc));

        let valid_asc = DocumentSource::File(std::path::PathBuf::from("doc.asc"));
        assert!(backend.is_valid(&valid_asc));

        let invalid_file = DocumentSource::File(std::path::PathBuf::from("doc.md"));
        assert!(!backend.is_valid(&invalid_file));

        let valid_stream = DocumentSource::Stream {
            name: "readme.adoc".to_string(),
            data: vec![],
        };
        assert!(backend.is_valid(&valid_stream));
    }

    #[test]
    fn attribute_lines_are_skipped() -> ruddydoc_core::Result<()> {
        let adoc = "\
:author: John Doe
:revdate: 2024-01-01

= Title

Content here.
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

        // Should have title and paragraph, no element for attribute lines
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
        assert_eq!(rows.len(), 2); // Title + paragraph

        Ok(())
    }

    #[test]
    fn list_items_have_parent() -> ruddydoc_core::Result<()> {
        let adoc = "\
* Item one
* Item two
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

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
    fn dash_unordered_list() -> ruddydoc_core::Result<()> {
        let adoc = "\
- Alpha
- Beta
";
        let (store, _meta, graph) = parse_asciidoc(adoc)?;

        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?l a <{}> }} }}",
            ont::iri(ont::CLASS_UNORDERED_LIST),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

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
        assert_eq!(rows.len(), 2);

        Ok(())
    }
}
