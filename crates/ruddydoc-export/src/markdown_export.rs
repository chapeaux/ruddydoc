//! Markdown exporter: reconstruct Markdown from the document graph.
//!
//! Queries all elements in reading order and produces Markdown text.

use ruddydoc_core::{DocumentExporter, DocumentStore, OutputFormat};
use ruddydoc_ontology as ont;

/// Markdown exporter.
pub struct MarkdownExporter;

impl DocumentExporter for MarkdownExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::Markdown
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        let mut output = String::new();

        // Query all elements with their types and reading order.
        // We use a UNION across the text element classes we care about.
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

        // Track list numbering
        let mut list_item_counter = 0u64;
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

                if type_str.contains("SectionHeader") {
                    let level = row
                        .get("level")
                        .and_then(|v| v.as_str())
                        .map(|s| parse_int(s) as usize)
                        .unwrap_or(1);
                    let prefix = "#".repeat(level);
                    output.push_str(&format!("{prefix} {text}\n\n"));
                } else if type_str.contains("Paragraph") {
                    output.push_str(&text);
                    output.push_str("\n\n");
                } else if type_str.contains("ListItem") {
                    // Determine if parent is ordered or unordered
                    let is_ordered = is_in_ordered_list(store, doc_graph, el_iri)?;
                    if is_ordered {
                        if !in_ordered_list {
                            list_item_counter = 0;
                            in_ordered_list = true;
                        }
                        list_item_counter += 1;
                        output.push_str(&format!("{list_item_counter}. {text}\n"));
                    } else {
                        in_ordered_list = false;
                        output.push_str(&format!("- {text}\n"));
                    }
                } else if type_str.contains("Code") {
                    let lang = row
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .map(clean_literal)
                        .unwrap_or_default();
                    // Ensure a newline before closing backticks for proper
                    // Markdown code block rendering.
                    let text_trimmed = text.trim_end_matches('\n');
                    output.push_str(&format!("```{lang}\n{text_trimmed}\n```\n\n"));
                } else if type_str.contains("TableElement") {
                    // Reconstruct table from cells
                    export_table(store, doc_graph, el_iri, &mut output)?;
                } else if type_str.contains("PictureElement") {
                    export_picture(store, doc_graph, el_iri, &mut output)?;
                } else if type_str.contains("OrderedList") || type_str.contains("UnorderedList") {
                    // List containers: skip, content comes from ListItem
                    if type_str.contains("OrderedList") {
                        in_ordered_list = true;
                        list_item_counter = 0;
                    } else {
                        in_ordered_list = false;
                    }
                } else if type_str.contains("Group") {
                    // Group container: skip
                }
            }
        }

        // Trim trailing whitespace
        let trimmed = output.trim_end().to_string();
        Ok(if trimmed.is_empty() {
            trimmed
        } else {
            format!("{trimmed}\n")
        })
    }
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

/// Export a table as pipe-delimited Markdown.
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

        // Compute column widths for alignment
        let col_count = table
            .values()
            .flat_map(|cells| cells.iter().map(|(c, _, _)| *c + 1))
            .max()
            .unwrap_or(0) as usize;
        let mut col_widths = vec![3usize; col_count]; // minimum width 3 for "---"

        for cells in table.values() {
            for (c, text, _) in cells {
                let idx = *c as usize;
                if idx < col_widths.len() {
                    col_widths[idx] = col_widths[idx].max(text.len());
                }
            }
        }

        let mut wrote_separator = false;
        for cells in table.values() {
            let mut line_parts = Vec::new();
            for (c, text, _) in cells {
                let idx = *c as usize;
                let width = if idx < col_widths.len() {
                    col_widths[idx]
                } else {
                    text.len()
                };
                line_parts.push(format!("{text:<width$}"));
            }
            output.push_str(&format!("| {} |", line_parts.join(" | ")));
            output.push('\n');

            // Add separator after header row
            if !wrote_separator && cells.iter().any(|(_, _, is_h)| *is_h) {
                let sep: Vec<String> = col_widths
                    .iter()
                    .take(cells.len())
                    .map(|w| "-".repeat(*w))
                    .collect();
                output.push_str(&format!("| {} |", sep.join(" | ")));
                output.push('\n');
                wrote_separator = true;
            }
        }
        output.push('\n');
    }

    Ok(())
}

/// Export a picture as Markdown image syntax.
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

        output.push_str(&format!("![{alt}]({target})\n\n"));
    }

    Ok(())
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
