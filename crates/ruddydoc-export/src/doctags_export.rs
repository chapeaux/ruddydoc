//! DocTags exporter: produce SmolDocling/GraniteDocling-compatible DocTag output.
//!
//! DocTags is a structured text format used by SmolDocling and GraniteDocling
//! for representing document structure as a flat tagged stream. Elements are
//! wrapped in `<page>` tags for paginated documents and enclosed in
//! `<doctag>` as the root element.

use ruddydoc_core::{DocumentExporter, DocumentStore, OutputFormat};
use ruddydoc_ontology as ont;

/// DocTags exporter producing SmolDocling/GraniteDocling-compatible output.
pub struct DocTagsExporter;

impl DocumentExporter for DocTagsExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::DocTags
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        let mut output = String::new();
        output.push_str("<doctag>");

        // Check if document is paginated
        let pages = query_pages(store, doc_graph)?;

        if pages.is_empty() {
            // Non-paginated: wrap all elements in a single <page>
            output.push_str("<page>");
            emit_elements(store, doc_graph, None, &mut output)?;
            output.push_str("</page>");
        } else {
            // Paginated: wrap each page's elements
            for page_num in &pages {
                output.push_str("<page>");
                emit_elements(store, doc_graph, Some(*page_num), &mut output)?;
                output.push_str("</page>");
            }
        }

        output.push_str("</doctag>\n");
        Ok(output)
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

/// Parse a boolean from a SPARQL literal result.
fn parse_bool(s: &str) -> bool {
    let cleaned = clean_literal(s);
    cleaned == "true" || cleaned == "1"
}

/// Query page numbers for paginated documents.
fn query_pages(store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<Vec<i64>> {
    let sparql = format!(
        "SELECT ?num WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?page a <{page_class}>. \
             ?page <{page_number}> ?num \
           }} \
         }} ORDER BY ?num",
        page_class = ont::iri(ont::CLASS_PAGE),
        page_number = ont::iri(ont::PROP_PAGE_NUMBER),
    );
    let result = store.query_to_json(&sparql)?;
    let mut pages = Vec::new();
    if let Some(rows) = result.as_array() {
        for row in rows {
            if let Some(num) = row.get("num").and_then(|v| v.as_str()) {
                pages.push(parse_int(num));
            }
        }
    }
    Ok(pages)
}

/// Emit DocTags for all elements, optionally filtering by page.
fn emit_elements(
    store: &dyn DocumentStore,
    doc_graph: &str,
    page_num: Option<i64>,
    output: &mut String,
) -> ruddydoc_core::Result<()> {
    // Build the page filter clause
    let page_filter = if let Some(pn) = page_num {
        format!(
            "?el <{on_page}> ?page. ?page <{page_number}> ?pn. FILTER(?pn = {pn})",
            on_page = ont::iri(ont::PROP_ON_PAGE),
            page_number = ont::iri(ont::PROP_PAGE_NUMBER),
        )
    } else {
        String::new()
    };

    // Query all elements with types and reading order
    let sparql = format!(
        "SELECT ?el ?type ?text ?order ?level ?lang WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?el <{reading_order}> ?order. \
             ?el a ?type. \
             OPTIONAL {{ ?el <{text_content}> ?text }} \
             OPTIONAL {{ ?el <{heading_level}> ?level }} \
             OPTIONAL {{ ?el <{code_language}> ?lang }} \
             {page_filter} \
             FILTER(?type IN ( \
               <{title}>, \
               <{section_header}>, \
               <{paragraph}>, \
               <{list_item}>, \
               <{code}>, \
               <{table}>, \
               <{picture}>, \
               <{caption}>, \
               <{formula}>, \
               <{footnote}>, \
               <{page_header}>, \
               <{page_footer}> \
             )) \
           }} \
         }} ORDER BY ?order",
        reading_order = ont::iri(ont::PROP_READING_ORDER),
        text_content = ont::iri(ont::PROP_TEXT_CONTENT),
        heading_level = ont::iri(ont::PROP_HEADING_LEVEL),
        code_language = ont::iri(ont::PROP_CODE_LANGUAGE),
        title = ont::iri(ont::CLASS_TITLE),
        section_header = ont::iri(ont::CLASS_SECTION_HEADER),
        paragraph = ont::iri(ont::CLASS_PARAGRAPH),
        list_item = ont::iri(ont::CLASS_LIST_ITEM),
        code = ont::iri(ont::CLASS_CODE),
        table = ont::iri(ont::CLASS_TABLE_ELEMENT),
        picture = ont::iri(ont::CLASS_PICTURE_ELEMENT),
        caption = ont::iri(ont::CLASS_CAPTION),
        formula = ont::iri(ont::CLASS_FORMULA),
        footnote = ont::iri(ont::CLASS_FOOTNOTE),
        page_header = ont::iri(ont::CLASS_PAGE_HEADER),
        page_footer = ont::iri(ont::CLASS_PAGE_FOOTER),
    );

    let result = store.query_to_json(&sparql)?;

    if let Some(rows) = result.as_array() {
        for row in rows {
            let type_str = row.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let text = row
                .get("text")
                .and_then(|v| v.as_str())
                .map(clean_literal)
                .unwrap_or_default();
            let el_iri = row.get("el").and_then(|v| v.as_str()).unwrap_or("");

            if type_str.contains("Title") && !type_str.contains("SectionHeader") {
                emit_tag("loc_title", &text, output);
            } else if type_str.contains("SectionHeader") {
                emit_tag("loc_section_header", &text, output);
            } else if type_str.contains("Paragraph") {
                emit_tag("loc_body", &text, output);
            } else if type_str.contains("ListItem") {
                emit_tag("loc_list_item", &text, output);
            } else if type_str.contains("Code") {
                emit_tag("loc_code", &text, output);
            } else if type_str.contains("Formula") {
                emit_tag("loc_formula", &text, output);
            } else if type_str.contains("Caption") {
                emit_tag("loc_caption", &text, output);
            } else if type_str.contains("Footnote") {
                emit_tag("loc_footnote", &text, output);
            } else if type_str.contains("PageHeader") {
                emit_tag("loc_header", &text, output);
            } else if type_str.contains("PageFooter") {
                emit_tag("loc_footer", &text, output);
            } else if type_str.contains("PictureElement") {
                // Picture: emit with alt text if available
                let alt = query_picture_alt(store, doc_graph, el_iri)?;
                emit_tag("loc_picture", &alt.unwrap_or_default(), output);
            } else if type_str.contains("TableElement") {
                emit_table(store, doc_graph, el_iri, output)?;
            }
        }
    }

    Ok(())
}

/// Emit a simple DocTag element.
fn emit_tag(tag: &str, content: &str, output: &mut String) {
    output.push_str(&format!("<{tag}>{}</{tag}>\n", escape_doctag(content)));
}

/// Escape characters that could interfere with DocTag parsing.
fn escape_doctag(s: &str) -> String {
    s.replace('<', "&lt;").replace('>', "&gt;")
}

/// Query the alt text of a picture element.
fn query_picture_alt(
    store: &dyn DocumentStore,
    doc_graph: &str,
    pic_iri: &str,
) -> ruddydoc_core::Result<Option<String>> {
    let pic_iri_clean = pic_iri.trim_start_matches('<').trim_end_matches('>');
    let sparql = format!(
        "SELECT ?alt WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             <{pic_iri_clean}> <{alt_text}> ?alt \
           }} \
         }} LIMIT 1",
        alt_text = ont::iri(ont::PROP_ALT_TEXT),
    );
    let result = store.query_to_json(&sparql)?;
    Ok(result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("alt"))
        .and_then(|v| v.as_str())
        .map(clean_literal))
}

/// Emit a table as DocTags with rows and cells.
fn emit_table(
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

        output.push_str("<loc_table>");
        for cells in table.values() {
            output.push_str("<loc_row>");
            for (_, text, is_header) in cells {
                let tag = if *is_header {
                    "loc_col_header"
                } else {
                    "loc_cell"
                };
                output.push_str(&format!("<{tag}>{}</{tag}>", escape_doctag(text)));
            }
            output.push_str("</loc_row>");
        }
        output.push_str("</loc_table>\n");
    }

    Ok(())
}
