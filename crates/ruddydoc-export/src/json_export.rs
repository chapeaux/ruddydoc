//! JSON exporter producing docling-compatible output.
//!
//! Queries the document graph via SPARQL and produces a JSON structure
//! that is structurally similar to Python docling's `DoclingDocument`.

use serde::Serialize;

use ruddydoc_core::{DocumentExporter, DocumentStore, OutputFormat};
use ruddydoc_ontology as ont;

/// JSON exporter producing docling-compatible output.
pub struct JsonExporter;

#[derive(Serialize)]
struct DoclingJson {
    name: String,
    source_format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    texts: Vec<TextItem>,
    tables: Vec<TableItem>,
    pictures: Vec<PictureItem>,
}

#[derive(Serialize)]
struct TextItem {
    #[serde(rename = "type")]
    element_type: String,
    text: String,
    reading_order: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading_level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code_language: Option<String>,
}

#[derive(Serialize)]
struct TableItem {
    cells: Vec<CellItem>,
    row_count: i64,
    col_count: i64,
}

#[derive(Serialize)]
struct CellItem {
    text: String,
    row: i64,
    col: i64,
    is_header: bool,
}

#[derive(Serialize)]
struct PictureItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alt_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_target: Option<String>,
}

impl DocumentExporter for JsonExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::Json
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        let name = query_document_name(store, doc_graph)?;
        let source_format = query_source_format(store, doc_graph)?;
        let language = query_document_language(store, doc_graph)?;
        let texts = query_texts(store, doc_graph)?;
        let tables = query_tables(store, doc_graph)?;
        let pictures = query_pictures(store, doc_graph)?;

        let doc = DoclingJson {
            name,
            source_format,
            language,
            texts,
            tables,
            pictures,
        };

        let json_str = serde_json::to_string_pretty(&doc)?;
        Ok(json_str)
    }
}

/// Extract a clean string from a SPARQL literal result.
///
/// Oxigraph returns typed literals as `"value"^^<datatype>`, so we need
/// to strip the datatype suffix and surrounding quotes.
fn clean_literal(s: &str) -> String {
    // Handle format: "value"^^<http://...>
    if let Some(idx) = s.find("\"^^<") {
        return s[1..idx].to_string();
    }
    // Handle format: "value"
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

fn query_document_name(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<String> {
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
        .map(clean_literal)
        .unwrap_or_else(|| "unknown".to_string());
    Ok(name)
}

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

fn query_source_format(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<String> {
    let sparql = format!(
        "SELECT ?fmt WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?doc a <{doc_class}>. \
             ?doc <{source_format}> ?fmt \
           }} \
         }} LIMIT 1",
        doc_class = ont::iri(ont::CLASS_DOCUMENT),
        source_format = ont::iri(ont::PROP_SOURCE_FORMAT),
    );
    let result = store.query_to_json(&sparql)?;
    let fmt = result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("fmt"))
        .and_then(|v| v.as_str())
        .map(clean_literal)
        .unwrap_or_else(|| "unknown".to_string());
    Ok(fmt)
}

fn query_texts(store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<Vec<TextItem>> {
    // Query all text elements: paragraphs, headings, list items, code blocks
    let text_classes = [
        (ont::CLASS_PARAGRAPH, "paragraph"),
        (ont::CLASS_SECTION_HEADER, "section_header"),
        (ont::CLASS_LIST_ITEM, "list_item"),
        (ont::CLASS_CODE, "code"),
        (ont::CLASS_TITLE, "title"),
    ];

    let mut items = Vec::new();

    for (class, type_name) in &text_classes {
        let sparql = format!(
            "SELECT ?text ?order WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 ?el a <{class_iri}>. \
                 ?el <{text_content}> ?text. \
                 ?el <{reading_order}> ?order \
               }} \
             }} ORDER BY ?order",
            class_iri = ont::iri(class),
            text_content = ont::iri(ont::PROP_TEXT_CONTENT),
            reading_order = ont::iri(ont::PROP_READING_ORDER),
        );
        let result = store.query_to_json(&sparql)?;
        if let Some(rows) = result.as_array() {
            for row in rows {
                let text = row
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(clean_literal)
                    .unwrap_or_default();
                let order = row
                    .get("order")
                    .and_then(|v| v.as_str())
                    .map(parse_int)
                    .unwrap_or(0);

                let mut item = TextItem {
                    element_type: type_name.to_string(),
                    text,
                    reading_order: order,
                    heading_level: None,
                    code_language: None,
                };

                // For headings, also get the level
                if *class == ont::CLASS_SECTION_HEADER {
                    item.heading_level = query_heading_level(store, doc_graph, order)?;
                }

                // For code blocks, also get the language
                if *class == ont::CLASS_CODE {
                    item.code_language = query_code_language(store, doc_graph, order)?;
                }

                items.push(item);
            }
        }
    }

    // Sort by reading order
    items.sort_by_key(|item| item.reading_order);

    Ok(items)
}

fn query_heading_level(
    store: &dyn DocumentStore,
    doc_graph: &str,
    reading_order: i64,
) -> ruddydoc_core::Result<Option<i64>> {
    let sparql = format!(
        "SELECT ?level WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?el a <{class_iri}>. \
             ?el <{reading_order_prop}> ?order. \
             ?el <{heading_level}> ?level. \
             FILTER(?order = {reading_order}) \
           }} \
         }} LIMIT 1",
        class_iri = ont::iri(ont::CLASS_SECTION_HEADER),
        reading_order_prop = ont::iri(ont::PROP_READING_ORDER),
        heading_level = ont::iri(ont::PROP_HEADING_LEVEL),
    );
    let result = store.query_to_json(&sparql)?;
    Ok(result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("level"))
        .and_then(|v| v.as_str())
        .map(parse_int))
}

fn query_code_language(
    store: &dyn DocumentStore,
    doc_graph: &str,
    reading_order: i64,
) -> ruddydoc_core::Result<Option<String>> {
    let sparql = format!(
        "SELECT ?lang WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?el a <{class_iri}>. \
             ?el <{reading_order_prop}> ?order. \
             ?el <{code_language}> ?lang. \
             FILTER(?order = {reading_order}) \
           }} \
         }} LIMIT 1",
        class_iri = ont::iri(ont::CLASS_CODE),
        reading_order_prop = ont::iri(ont::PROP_READING_ORDER),
        code_language = ont::iri(ont::PROP_CODE_LANGUAGE),
    );
    let result = store.query_to_json(&sparql)?;
    Ok(result
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("lang"))
        .and_then(|v| v.as_str())
        .map(clean_literal))
}

fn query_tables(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<Vec<TableItem>> {
    // Find all tables
    let sparql_tables = format!(
        "SELECT ?table ?rows ?cols WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?table a <{table_class}>. \
             ?table <{row_count}> ?rows. \
             ?table <{col_count}> ?cols \
           }} \
         }}",
        table_class = ont::iri(ont::CLASS_TABLE_ELEMENT),
        row_count = ont::iri(ont::PROP_ROW_COUNT),
        col_count = ont::iri(ont::PROP_COLUMN_COUNT),
    );
    let table_results = store.query_to_json(&sparql_tables)?;

    let mut tables = Vec::new();

    if let Some(table_rows) = table_results.as_array() {
        for table_row in table_rows {
            let table_iri = table_row
                .get("table")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let row_count = table_row
                .get("rows")
                .and_then(|v| v.as_str())
                .map(parse_int)
                .unwrap_or(0);
            let col_count = table_row
                .get("cols")
                .and_then(|v| v.as_str())
                .map(parse_int)
                .unwrap_or(0);

            // Strip angle brackets from IRI if present
            let table_iri_clean = table_iri.trim_start_matches('<').trim_end_matches('>');

            // Query cells for this table
            let sparql_cells = format!(
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
            let cell_results = store.query_to_json(&sparql_cells)?;

            let mut cells = Vec::new();
            if let Some(cell_rows) = cell_results.as_array() {
                for cell_row_data in cell_rows {
                    let text = cell_row_data
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(clean_literal)
                        .unwrap_or_default();
                    let row = cell_row_data
                        .get("row")
                        .and_then(|v| v.as_str())
                        .map(parse_int)
                        .unwrap_or(0);
                    let col = cell_row_data
                        .get("col")
                        .and_then(|v| v.as_str())
                        .map(parse_int)
                        .unwrap_or(0);
                    let is_header = cell_row_data
                        .get("isH")
                        .and_then(|v| v.as_str())
                        .map(parse_bool)
                        .unwrap_or(false);

                    cells.push(CellItem {
                        text,
                        row,
                        col,
                        is_header,
                    });
                }
            }

            tables.push(TableItem {
                cells,
                row_count,
                col_count,
            });
        }
    }

    Ok(tables)
}

fn query_pictures(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<Vec<PictureItem>> {
    let sparql = format!(
        "SELECT ?pic ?fmt ?alt ?target WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?pic a <{pic_class}>. \
             OPTIONAL {{ ?pic <{pic_format}> ?fmt }} \
             OPTIONAL {{ ?pic <{alt_text}> ?alt }} \
             OPTIONAL {{ ?pic <{link_target}> ?target }} \
           }} \
         }}",
        pic_class = ont::iri(ont::CLASS_PICTURE_ELEMENT),
        pic_format = ont::iri(ont::PROP_PICTURE_FORMAT),
        alt_text = ont::iri(ont::PROP_ALT_TEXT),
        link_target = ont::iri(ont::PROP_LINK_TARGET),
    );
    let result = store.query_to_json(&sparql)?;

    let mut pictures = Vec::new();

    if let Some(rows) = result.as_array() {
        for row in rows {
            let format = row
                .get("fmt")
                .and_then(|v| v.as_str())
                .filter(|s| *s != "null")
                .map(clean_literal);
            let alt_text = row
                .get("alt")
                .and_then(|v| v.as_str())
                .filter(|s| *s != "null")
                .map(clean_literal);
            let link_target = row
                .get("target")
                .and_then(|v| v.as_str())
                .filter(|s| *s != "null")
                .map(clean_literal);

            pictures.push(PictureItem {
                format,
                alt_text,
                link_target,
            });
        }
    }

    Ok(pictures)
}
