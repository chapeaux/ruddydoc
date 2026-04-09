//! HTML exporter: reconstruct semantic HTML5 from the document graph.
//!
//! Queries all elements in reading order and produces a well-formed HTML5
//! document with proper semantic markup.

use ruddydoc_core::{DocumentExporter, DocumentStore, OutputFormat};
use ruddydoc_ontology as ont;

/// HTML exporter producing semantic HTML5 output.
pub struct HtmlExporter;

impl DocumentExporter for HtmlExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::Html
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        let title = query_title(store, doc_graph)?;
        let mut body = String::new();

        // Query all elements with their types and reading order
        let sparql = format!(
            "SELECT ?el ?type ?text ?order ?level ?lang WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 ?el <{reading_order}> ?order. \
                 ?el a ?type. \
                 OPTIONAL {{ ?el <{text_content}> ?text }} \
                 OPTIONAL {{ ?el <{heading_level}> ?level }} \
                 OPTIONAL {{ ?el <{code_language}> ?lang }} \
                 FILTER(?type IN ( \
                   <{section_header}>, \
                   <{paragraph}>, \
                   <{list_item}>, \
                   <{code}>, \
                   <{picture}>, \
                   <{table}>, \
                   <{group}>, \
                   <{ordered_list}>, \
                   <{unordered_list}> \
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
            picture = ont::iri(ont::CLASS_PICTURE_ELEMENT),
            table = ont::iri(ont::CLASS_TABLE_ELEMENT),
            group = ont::iri(ont::CLASS_GROUP),
            ordered_list = ont::iri(ont::CLASS_ORDERED_LIST),
            unordered_list = ont::iri(ont::CLASS_UNORDERED_LIST),
        );

        let result = store.query_to_json(&sparql)?;

        // Track list state for proper nesting
        let mut in_unordered_list = false;
        let mut in_ordered_list = false;

        if let Some(rows) = result.as_array() {
            for row in rows {
                let type_str = row.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let text = row
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(clean_literal)
                    .unwrap_or_default();
                let el_iri = row.get("el").and_then(|v| v.as_str()).unwrap_or("");

                // Close open lists if we encounter a non-list-item element
                if !type_str.contains("ListItem")
                    && !type_str.contains("OrderedList")
                    && !type_str.contains("UnorderedList")
                {
                    if in_unordered_list {
                        body.push_str("</ul>\n");
                        in_unordered_list = false;
                    }
                    if in_ordered_list {
                        body.push_str("</ol>\n");
                        in_ordered_list = false;
                    }
                }

                if type_str.contains("SectionHeader") {
                    let level = row
                        .get("level")
                        .and_then(|v| v.as_str())
                        .map(|s| parse_int(s) as usize)
                        .unwrap_or(1)
                        .clamp(1, 6);
                    body.push_str(&format!("<h{level}>{}</h{level}>\n", escape_html(&text)));
                } else if type_str.contains("Paragraph") {
                    body.push_str(&format!("<p>{}</p>\n", escape_html(&text)));
                } else if type_str.contains("ListItem") {
                    let is_ordered = is_in_ordered_list(store, doc_graph, el_iri)?;
                    if is_ordered {
                        if !in_ordered_list {
                            // Close any open unordered list
                            if in_unordered_list {
                                body.push_str("</ul>\n");
                                in_unordered_list = false;
                            }
                            body.push_str("<ol>");
                            in_ordered_list = true;
                        }
                    } else if !in_unordered_list {
                        // Close any open ordered list
                        if in_ordered_list {
                            body.push_str("</ol>\n");
                            in_ordered_list = false;
                        }
                        body.push_str("<ul>");
                        in_unordered_list = true;
                    }
                    body.push_str(&format!("<li>{}</li>", escape_html(&text)));
                } else if type_str.contains("Code") {
                    let lang = row
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .map(clean_literal)
                        .unwrap_or_default();
                    if lang.is_empty() {
                        body.push_str(&format!("<pre><code>{}</code></pre>\n", escape_html(&text)));
                    } else {
                        body.push_str(&format!(
                            "<pre><code class=\"language-{lang}\">{}</code></pre>\n",
                            escape_html(&text)
                        ));
                    }
                } else if type_str.contains("TableElement") {
                    export_table(store, doc_graph, el_iri, &mut body)?;
                } else if type_str.contains("PictureElement") {
                    export_picture(store, doc_graph, el_iri, &mut body)?;
                } else if type_str.contains("Group")
                    && !type_str.contains("OrderedList")
                    && !type_str.contains("UnorderedList")
                {
                    // Group that is not a list — treat as blockquote
                    export_blockquote_children(store, doc_graph, el_iri, &mut body)?;
                } else if type_str.contains("OrderedList") || type_str.contains("UnorderedList") {
                    // List containers: content comes from ListItem rows; skip
                }
            }

            // Close any trailing open lists
            if in_unordered_list {
                body.push_str("</ul>\n");
            }
            if in_ordered_list {
                body.push_str("</ol>\n");
            }
        }

        let escaped_title = escape_html(&title);
        let lang = query_document_language(store, doc_graph)?;
        let lang_attr = lang.as_deref().unwrap_or("en");
        let html = format!(
            "<!DOCTYPE html>\n\
             <html lang=\"{lang_attr}\">\n\
             <head>\n\
             <meta charset=\"utf-8\">\n\
             <title>{escaped_title}</title>\n\
             </head>\n\
             <body>\n\
             <article>\n\
             {body}\
             </article>\n\
             </body>\n\
             </html>\n"
        );

        Ok(html)
    }
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Query the document language from the `rdoc:language` property.
fn query_document_language(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<Option<String>> {
    let sparql = format!(
        "SELECT ?lang WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?doc a <{doc_class}>. \
             ?doc <{language}> ?lang \
           }} \
         }} LIMIT 1",
        doc_class = ont::iri(ont::CLASS_DOCUMENT),
        language = ont::iri(ont::PROP_LANGUAGE),
    );
    let result = store.query_to_json(&sparql)?;
    Ok(result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("lang"))
        .and_then(|v| v.as_str())
        .map(clean_literal))
}

/// Query the document title (from document name or first heading).
fn query_title(store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
    // Try to get the file name first
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
        .map(clean_literal);

    if let Some(n) = name
        && !n.is_empty()
        && n != "unknown"
    {
        return Ok(n);
    }

    // Fallback: use the first heading text
    let sparql_h = format!(
        "SELECT ?text WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?el a <{header_class}>. \
             ?el <{text_content}> ?text. \
             ?el <{reading_order}> ?order \
           }} \
         }} ORDER BY ?order LIMIT 1",
        header_class = ont::iri(ont::CLASS_SECTION_HEADER),
        text_content = ont::iri(ont::PROP_TEXT_CONTENT),
        reading_order = ont::iri(ont::PROP_READING_ORDER),
    );
    let result_h = store.query_to_json(&sparql_h)?;
    Ok(result_h
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("text"))
        .and_then(|v| v.as_str())
        .map(clean_literal)
        .unwrap_or_else(|| "Untitled".to_string()))
}

/// Check if a list item's parent is an OrderedList.
fn is_in_ordered_list(
    store: &dyn DocumentStore,
    doc_graph: &str,
    el_iri: &str,
) -> ruddydoc_core::Result<bool> {
    let el_iri_clean = el_iri.trim_start_matches('<').trim_end_matches('>');

    let sparql = format!(
        "ASK {{ GRAPH <{doc_graph}> {{ \
           <{el_iri_clean}> <{parent}> ?p. \
           ?p a <{ordered_list}> \
         }} }}",
        parent = ont::iri(ont::PROP_PARENT_ELEMENT),
        ordered_list = ont::iri(ont::CLASS_ORDERED_LIST),
    );
    let result = store.query_to_json(&sparql)?;
    Ok(result.as_bool().unwrap_or(false))
}

/// Export a table as HTML with thead/tbody.
fn export_table(
    store: &dyn DocumentStore,
    doc_graph: &str,
    table_iri: &str,
    output: &mut String,
) -> ruddydoc_core::Result<()> {
    let table_iri_clean = table_iri.trim_start_matches('<').trim_end_matches('>');

    let sparql = format!(
        "SELECT ?text ?row ?col ?isH WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             <{table_iri_clean}> <{has_cell}> ?cell. \
             ?cell <{cell_text}> ?text. \
             ?cell <{cell_row}> ?row. \
             ?cell <{cell_col}> ?col. \
             ?cell <{is_header}> ?isH \
           }} \
         }} ORDER BY ?row ?col",
        has_cell = ont::iri(ont::PROP_HAS_CELL),
        cell_text = ont::iri(ont::PROP_CELL_TEXT),
        cell_row = ont::iri(ont::PROP_CELL_ROW),
        cell_col = ont::iri(ont::PROP_CELL_COLUMN),
        is_header = ont::iri(ont::PROP_IS_HEADER),
    );
    let result = store.query_to_json(&sparql)?;

    if let Some(rows) = result.as_array() {
        if rows.is_empty() {
            return Ok(());
        }

        // Group cells by row
        let mut table: std::collections::BTreeMap<i64, Vec<(i64, String, bool)>> =
            std::collections::BTreeMap::new();

        for row in rows {
            let text = row
                .get("text")
                .and_then(|v| v.as_str())
                .map(clean_literal)
                .unwrap_or_default();
            let r = row
                .get("row")
                .and_then(|v| v.as_str())
                .map(parse_int)
                .unwrap_or(0);
            let c = row
                .get("col")
                .and_then(|v| v.as_str())
                .map(parse_int)
                .unwrap_or(0);
            let is_h = row
                .get("isH")
                .and_then(|v| v.as_str())
                .map(parse_bool)
                .unwrap_or(false);

            table.entry(r).or_default().push((c, text, is_h));
        }

        // Sort cells within each row by column
        for cells in table.values_mut() {
            cells.sort_by_key(|(c, _, _)| *c);
        }

        output.push_str("<table>\n");

        // Separate header rows from body rows
        let mut header_rows = Vec::new();
        let mut body_rows = Vec::new();

        for (row_idx, cells) in &table {
            let all_header = cells.iter().all(|(_, _, is_h)| *is_h);
            if all_header {
                header_rows.push((*row_idx, cells));
            } else {
                body_rows.push((*row_idx, cells));
            }
        }

        // Emit thead
        if !header_rows.is_empty() {
            output.push_str("<thead>");
            for (_row_idx, cells) in &header_rows {
                output.push_str("<tr>");
                for (_, text, _) in *cells {
                    output.push_str(&format!("<th>{}</th>", escape_html(text)));
                }
                output.push_str("</tr>");
            }
            output.push_str("</thead>\n");
        }

        // Emit tbody
        if !body_rows.is_empty() {
            output.push_str("<tbody>");
            for (_row_idx, cells) in &body_rows {
                output.push_str("<tr>");
                for (_, text, _) in *cells {
                    output.push_str(&format!("<td>{}</td>", escape_html(text)));
                }
                output.push_str("</tr>");
            }
            output.push_str("</tbody>\n");
        }

        output.push_str("</table>\n");
    }

    Ok(())
}

/// Export a picture as an HTML figure or img element.
fn export_picture(
    store: &dyn DocumentStore,
    doc_graph: &str,
    pic_iri: &str,
    output: &mut String,
) -> ruddydoc_core::Result<()> {
    let pic_iri_clean = pic_iri.trim_start_matches('<').trim_end_matches('>');

    let sparql = format!(
        "SELECT ?alt ?target WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             OPTIONAL {{ <{pic_iri_clean}> <{alt_text}> ?alt }} \
             OPTIONAL {{ <{pic_iri_clean}> <{link_target}> ?target }} \
           }} \
         }} LIMIT 1",
        alt_text = ont::iri(ont::PROP_ALT_TEXT),
        link_target = ont::iri(ont::PROP_LINK_TARGET),
    );
    let result = store.query_to_json(&sparql)?;

    if let Some(rows) = result.as_array()
        && let Some(row) = rows.first()
    {
        let alt = row
            .get("alt")
            .and_then(|v| v.as_str())
            .map(clean_literal)
            .unwrap_or_default();
        let target = row
            .get("target")
            .and_then(|v| v.as_str())
            .map(clean_literal)
            .unwrap_or_default();

        // Check for a caption
        let caption = query_picture_caption(store, doc_graph, pic_iri_clean)?;

        if let Some(cap) = caption {
            output.push_str("<figure>\n");
            output.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\">\n",
                escape_html_attr(&target),
                escape_html_attr(&alt),
            ));
            output.push_str(&format!("<figcaption>{}</figcaption>\n", escape_html(&cap)));
            output.push_str("</figure>\n");
        } else {
            output.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\">\n",
                escape_html_attr(&target),
                escape_html_attr(&alt),
            ));
        }
    }

    Ok(())
}

/// Query a caption associated with a picture.
fn query_picture_caption(
    store: &dyn DocumentStore,
    doc_graph: &str,
    pic_iri_clean: &str,
) -> ruddydoc_core::Result<Option<String>> {
    let sparql = format!(
        "SELECT ?capText WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             <{pic_iri_clean}> <{has_caption}> ?cap. \
             ?cap <{text_content}> ?capText \
           }} \
         }} LIMIT 1",
        has_caption = ont::iri(ont::PROP_HAS_CAPTION),
        text_content = ont::iri(ont::PROP_TEXT_CONTENT),
    );
    let result = store.query_to_json(&sparql)?;
    Ok(result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("capText"))
        .and_then(|v| v.as_str())
        .map(clean_literal))
}

/// Export children of a group (blockquote) as a blockquote element.
fn export_blockquote_children(
    store: &dyn DocumentStore,
    doc_graph: &str,
    group_iri: &str,
    output: &mut String,
) -> ruddydoc_core::Result<()> {
    let group_iri_clean = group_iri.trim_start_matches('<').trim_end_matches('>');

    // Query child elements of this group
    let sparql = format!(
        "SELECT ?text WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?child <{parent}> <{group_iri_clean}>. \
             ?child <{text_content}> ?text. \
             ?child <{reading_order}> ?order \
           }} \
         }} ORDER BY ?order",
        parent = ont::iri(ont::PROP_PARENT_ELEMENT),
        text_content = ont::iri(ont::PROP_TEXT_CONTENT),
        reading_order = ont::iri(ont::PROP_READING_ORDER),
    );
    let result = store.query_to_json(&sparql)?;

    output.push_str("<blockquote>");
    if let Some(rows) = result.as_array() {
        for row in rows {
            let text = row
                .get("text")
                .and_then(|v| v.as_str())
                .map(clean_literal)
                .unwrap_or_default();
            output.push_str(&format!("<p>{}</p>", escape_html(&text)));
        }
    }
    output.push_str("</blockquote>\n");

    Ok(())
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Escape HTML entities in text content.
pub(crate) fn escape_html(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            _ => result.push(ch),
        }
    }
    result
}

/// Escape HTML attribute values.
fn escape_html_attr(s: &str) -> String {
    // Same escaping — covers &, <, >, "
    escape_html(s)
}

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

/// Parse a boolean from a SPARQL literal result.
fn parse_bool(s: &str) -> bool {
    let cleaned = clean_literal(s);
    cleaned == "true" || cleaned == "1"
}
