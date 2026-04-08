//! Plain text exporter: extract all text content in reading order.
//!
//! Queries all text-bearing elements in reading order and produces plain
//! text output with elements separated by newlines.

use ruddydoc_core::{DocumentExporter, DocumentStore, OutputFormat};
use ruddydoc_ontology as ont;

/// Plain text exporter.
pub struct TextExporter;

impl DocumentExporter for TextExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::Text
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        let mut output = String::new();

        // Query all text-bearing elements in reading order.
        // Include paragraphs, headings, list items, code blocks, and captions.
        let sparql = format!(
            "SELECT ?text ?order WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 ?el <{reading_order}> ?order. \
                 ?el <{text_content}> ?text. \
                 ?el a ?type. \
                 FILTER(?type IN ( \
                   <{section_header}>, \
                   <{paragraph}>, \
                   <{list_item}>, \
                   <{code}>, \
                   <{title}> \
                 )) \
               }} \
             }} ORDER BY ?order",
            reading_order = ont::iri(ont::PROP_READING_ORDER),
            text_content = ont::iri(ont::PROP_TEXT_CONTENT),
            section_header = ont::iri(ont::CLASS_SECTION_HEADER),
            paragraph = ont::iri(ont::CLASS_PARAGRAPH),
            list_item = ont::iri(ont::CLASS_LIST_ITEM),
            code = ont::iri(ont::CLASS_CODE),
            title = ont::iri(ont::CLASS_TITLE),
        );

        let result = store.query_to_json(&sparql)?;

        if let Some(rows) = result.as_array() {
            for (i, row) in rows.iter().enumerate() {
                let text = row
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(clean_literal)
                    .unwrap_or_default();

                if !text.is_empty() {
                    if i > 0 {
                        output.push('\n');
                    }
                    output.push_str(&text);
                }
            }
        }

        // Also include table cell text
        let sparql_cells = format!(
            "SELECT ?table ?order WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 ?table a <{table_class}>. \
                 ?table <{reading_order}> ?order \
               }} \
             }} ORDER BY ?order",
            table_class = ont::iri(ont::CLASS_TABLE_ELEMENT),
            reading_order = ont::iri(ont::PROP_READING_ORDER),
        );

        let table_result = store.query_to_json(&sparql_cells)?;

        if let Some(table_rows) = table_result.as_array() {
            for table_row in table_rows {
                let table_iri = table_row
                    .get("table")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let table_iri_clean = table_iri.trim_start_matches('<').trim_end_matches('>');

                let sparql_cell_text = format!(
                    "SELECT ?text ?row ?col WHERE {{ \
                       GRAPH <{doc_graph}> {{ \
                         <{table_iri_clean}> <{has_cell}> ?cell. \
                         ?cell <{cell_text}> ?text. \
                         ?cell <{cell_row}> ?row. \
                         ?cell <{cell_col}> ?col \
                       }} \
                     }} ORDER BY ?row ?col",
                    has_cell = ont::iri(ont::PROP_HAS_CELL),
                    cell_text = ont::iri(ont::PROP_CELL_TEXT),
                    cell_row = ont::iri(ont::PROP_CELL_ROW),
                    cell_col = ont::iri(ont::PROP_CELL_COLUMN),
                );

                let cell_result = store.query_to_json(&sparql_cell_text)?;

                if let Some(cells) = cell_result.as_array() {
                    for cell in cells {
                        let text = cell
                            .get("text")
                            .and_then(|v| v.as_str())
                            .map(clean_literal)
                            .unwrap_or_default();

                        if !text.is_empty() {
                            if !output.is_empty() {
                                output.push('\n');
                            }
                            output.push_str(&text);
                        }
                    }
                }
            }
        }

        // Ensure single trailing newline
        let trimmed = output.trim_end().to_string();
        Ok(if trimmed.is_empty() {
            trimmed
        } else {
            format!("{trimmed}\n")
        })
    }
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
