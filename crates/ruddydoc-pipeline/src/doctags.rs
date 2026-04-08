//! DocTags parser for converting VLM output into RDF triples.
//!
//! DocTags is a structured text format with XML-like tags produced by
//! SmolDocling/GraniteDocling VLMs. This parser scans for `<tag>` and
//! `</tag>` patterns and maps each element type to its corresponding
//! RuddyDoc ontology class.

use ruddydoc_core::DocumentStore;
use ruddydoc_ontology as ont;

// ---------------------------------------------------------------------------
// Tag-to-ontology mapping
// ---------------------------------------------------------------------------

/// Map a DocTags tag name to an ontology class constant.
///
/// Returns `None` for structural tags (`doctag`, `page`, `loc_table`,
/// `loc_row`) that do not directly map to a leaf element.
fn tag_to_ontology_class(tag: &str) -> Option<&'static str> {
    match tag {
        "loc_title" => Some(ont::CLASS_TITLE),
        "loc_section_header" => Some(ont::CLASS_SECTION_HEADER),
        "loc_body" => Some(ont::CLASS_PARAGRAPH),
        "loc_list_item" => Some(ont::CLASS_LIST_ITEM),
        "loc_caption" => Some(ont::CLASS_CAPTION),
        "loc_formula" => Some(ont::CLASS_FORMULA),
        "loc_code" => Some(ont::CLASS_CODE),
        "loc_footnote" => Some(ont::CLASS_FOOTNOTE),
        "loc_header" => Some(ont::CLASS_PAGE_HEADER),
        "loc_footer" => Some(ont::CLASS_PAGE_FOOTER),
        "loc_picture" => Some(ont::CLASS_PICTURE_ELEMENT),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Parsed element (internal)
// ---------------------------------------------------------------------------

/// A parsed element from DocTags output.
#[derive(Debug, Clone)]
struct ParsedElement {
    /// The ontology class for this element.
    class: &'static str,
    /// The text content of the element.
    text: String,
    /// Sequential order index within the page.
    order: usize,
    /// The page number (1-based) this element belongs to.
    page_number: u32,
}

/// A parsed table from DocTags output.
#[derive(Debug, Clone)]
struct ParsedTable {
    /// Rows of cells. Each cell has (text, is_header).
    rows: Vec<Vec<(String, bool)>>,
    /// Sequential order index within the page.
    order: usize,
    /// The page number (1-based) this table belongs to.
    page_number: u32,
}

// ---------------------------------------------------------------------------
// DocTagsParser
// ---------------------------------------------------------------------------

/// Parser for DocTags format output from VLMs.
///
/// DocTags is a structured text format with XML-like tags. The parser
/// extracts elements and inserts them into the document graph as RDF triples.
pub struct DocTagsParser;

impl DocTagsParser {
    /// Create a new DocTags parser.
    pub fn new() -> Self {
        Self
    }

    /// Parse DocTags text and insert elements into the document graph.
    ///
    /// Returns the number of elements created in the graph.
    pub fn parse_into_graph(
        &self,
        text: &str,
        store: &dyn DocumentStore,
        doc_graph: &str,
        page_number: u32,
    ) -> ruddydoc_core::Result<usize> {
        let (elements, tables) = Self::parse_text(text, page_number);
        let mut count = 0usize;

        let rdf_type = ont::rdf_iri("type");

        for elem in &elements {
            let el_iri = format!("{doc_graph}/el-{}-{}", elem.page_number, elem.order);

            // rdf:type
            store.insert_triple_into(&el_iri, &rdf_type, &ont::iri(elem.class), doc_graph)?;

            // rdoc:textContent
            if !elem.text.is_empty() {
                store.insert_literal(
                    &el_iri,
                    &ont::iri(ont::PROP_TEXT_CONTENT),
                    &elem.text,
                    "string",
                    doc_graph,
                )?;
            }

            // rdoc:readingOrder
            store.insert_literal(
                &el_iri,
                &ont::iri(ont::PROP_READING_ORDER),
                &elem.order.to_string(),
                "integer",
                doc_graph,
            )?;

            // rdoc:onPage — link to the page node
            let page_iri = format!("{doc_graph}/page-{}", elem.page_number);
            store.insert_triple_into(
                &el_iri,
                &ont::iri(ont::PROP_ON_PAGE),
                &page_iri,
                doc_graph,
            )?;

            count += 1;
        }

        // Insert tables.
        for table in &tables {
            let table_iri = format!("{doc_graph}/el-{}-{}", table.page_number, table.order);

            // rdf:type TableElement
            store.insert_triple_into(
                &table_iri,
                &rdf_type,
                &ont::iri(ont::CLASS_TABLE_ELEMENT),
                doc_graph,
            )?;

            // rdoc:readingOrder
            store.insert_literal(
                &table_iri,
                &ont::iri(ont::PROP_READING_ORDER),
                &table.order.to_string(),
                "integer",
                doc_graph,
            )?;

            // rdoc:onPage
            let page_iri = format!("{doc_graph}/page-{}", table.page_number);
            store.insert_triple_into(
                &table_iri,
                &ont::iri(ont::PROP_ON_PAGE),
                &page_iri,
                doc_graph,
            )?;

            // Row/column counts.
            let row_count = table.rows.len();
            let col_count = table.rows.iter().map(|r| r.len()).max().unwrap_or(0);
            store.insert_literal(
                &table_iri,
                &ont::iri(ont::PROP_ROW_COUNT),
                &row_count.to_string(),
                "integer",
                doc_graph,
            )?;
            store.insert_literal(
                &table_iri,
                &ont::iri(ont::PROP_COLUMN_COUNT),
                &col_count.to_string(),
                "integer",
                doc_graph,
            )?;

            // Insert cells.
            for (row_idx, row) in table.rows.iter().enumerate() {
                for (col_idx, (cell_text, is_header)) in row.iter().enumerate() {
                    let cell_iri = format!("{table_iri}/cell-{row_idx}-{col_idx}");

                    store.insert_triple_into(
                        &cell_iri,
                        &rdf_type,
                        &ont::iri(ont::CLASS_TABLE_CELL),
                        doc_graph,
                    )?;

                    store.insert_triple_into(
                        &table_iri,
                        &ont::iri(ont::PROP_HAS_CELL),
                        &cell_iri,
                        doc_graph,
                    )?;

                    store.insert_literal(
                        &cell_iri,
                        &ont::iri(ont::PROP_CELL_ROW),
                        &row_idx.to_string(),
                        "integer",
                        doc_graph,
                    )?;

                    store.insert_literal(
                        &cell_iri,
                        &ont::iri(ont::PROP_CELL_COLUMN),
                        &col_idx.to_string(),
                        "integer",
                        doc_graph,
                    )?;

                    if !cell_text.is_empty() {
                        store.insert_literal(
                            &cell_iri,
                            &ont::iri(ont::PROP_CELL_TEXT),
                            cell_text,
                            "string",
                            doc_graph,
                        )?;
                    }

                    if *is_header {
                        store.insert_literal(
                            &cell_iri,
                            &ont::iri(ont::PROP_IS_HEADER),
                            "true",
                            "boolean",
                            doc_graph,
                        )?;
                    }

                    count += 1;
                }
            }

            count += 1; // for the table element itself
        }

        Ok(count)
    }

    /// Parse DocTags text into structured elements and tables.
    ///
    /// This is a simple scanner that looks for `<tag>...</tag>` patterns.
    /// It handles nested structures for tables (`loc_table > loc_row > loc_cell/loc_col_header`)
    /// and multi-page output (`<page>` boundaries).
    fn parse_text(text: &str, base_page: u32) -> (Vec<ParsedElement>, Vec<ParsedTable>) {
        let mut elements = Vec::new();
        let mut tables = Vec::new();
        let mut order = 0usize;
        let mut current_page = base_page;

        let mut pos = 0;
        let bytes = text.as_bytes();
        let len = bytes.len();

        while pos < len {
            // Find the next '<'.
            let Some(tag_start) = text[pos..].find('<') else {
                break;
            };
            let tag_start = pos + tag_start;

            // Find the closing '>'.
            let Some(tag_end) = text[tag_start..].find('>') else {
                break;
            };
            let tag_end = tag_start + tag_end;

            let tag_content = &text[tag_start + 1..tag_end];
            pos = tag_end + 1;

            // Skip closing tags at the top level.
            if tag_content.starts_with('/') {
                continue;
            }

            // Handle structural tags.
            match tag_content {
                "doctag" => continue,
                "page" => {
                    // If this is not the first page tag and we already have
                    // elements, bump the page number.
                    if !elements.is_empty() || !tables.is_empty() {
                        current_page += 1;
                        // Reset order for new page.
                        order = 0;
                    }
                    continue;
                }
                "loc_table" => {
                    // Parse the table contents until </loc_table>.
                    let (table, new_pos) = Self::parse_table(&text[pos..], current_page, order);
                    if let Some(t) = table {
                        tables.push(t);
                        order += 1;
                    }
                    pos += new_pos;
                    continue;
                }
                _ => {}
            }

            // Check if this is a known element tag.
            if let Some(class) = tag_to_ontology_class(tag_content) {
                // Find the closing tag.
                let close_tag = format!("</{tag_content}>");
                let content = if let Some(close_pos) = text[pos..].find(&close_tag) {
                    let content = &text[pos..pos + close_pos];
                    pos += close_pos + close_tag.len();
                    content.trim().to_string()
                } else {
                    // No closing tag found; take remaining text until next '<' or end.
                    let end = text[pos..].find('<').map(|p| pos + p).unwrap_or(len);
                    let content = &text[pos..end];
                    pos = end;
                    content.trim().to_string()
                };

                elements.push(ParsedElement {
                    class,
                    text: content,
                    order,
                    page_number: current_page,
                });
                order += 1;
            }
        }

        (elements, tables)
    }

    /// Parse a table from the current position (after `<loc_table>`).
    ///
    /// Returns the parsed table and the number of bytes consumed from `text`.
    fn parse_table(text: &str, page_number: u32, order: usize) -> (Option<ParsedTable>, usize) {
        let mut rows: Vec<Vec<(String, bool)>> = Vec::new();
        let mut pos = 0;
        let len = text.len();

        while pos < len {
            let Some(tag_start) = text[pos..].find('<') else {
                break;
            };
            let tag_start = pos + tag_start;

            let Some(tag_end) = text[tag_start..].find('>') else {
                break;
            };
            let tag_end = tag_start + tag_end;

            let tag_content = &text[tag_start + 1..tag_end];
            pos = tag_end + 1;

            match tag_content {
                "/loc_table" => {
                    // End of table.
                    let table = if rows.is_empty() {
                        None
                    } else {
                        Some(ParsedTable {
                            rows,
                            order,
                            page_number,
                        })
                    };
                    return (table, pos);
                }
                "loc_row" => {
                    // Parse cells within this row.
                    let (row, new_pos) = Self::parse_table_row(&text[pos..]);
                    rows.push(row);
                    pos += new_pos;
                }
                _ => {
                    // Skip other tags/content within the table.
                    if tag_content.starts_with('/') {
                        continue;
                    }
                }
            }
        }

        // Unterminated table — return what we have.
        let table = if rows.is_empty() {
            None
        } else {
            Some(ParsedTable {
                rows,
                order,
                page_number,
            })
        };
        (table, pos)
    }

    /// Parse a single table row (after `<loc_row>`).
    ///
    /// Returns the cells and bytes consumed.
    fn parse_table_row(text: &str) -> (Vec<(String, bool)>, usize) {
        let mut cells: Vec<(String, bool)> = Vec::new();
        let mut pos = 0;
        let len = text.len();

        while pos < len {
            let Some(tag_start) = text[pos..].find('<') else {
                break;
            };
            let tag_start = pos + tag_start;

            let Some(tag_end) = text[tag_start..].find('>') else {
                break;
            };
            let tag_end = tag_start + tag_end;

            let tag_content = &text[tag_start + 1..tag_end];
            pos = tag_end + 1;

            match tag_content {
                "/loc_row" => {
                    return (cells, pos);
                }
                "loc_cell" => {
                    let close_tag = "</loc_cell>";
                    let content = if let Some(close_pos) = text[pos..].find(close_tag) {
                        let c = text[pos..pos + close_pos].trim().to_string();
                        pos += close_pos + close_tag.len();
                        c
                    } else {
                        let end = text[pos..].find('<').map(|p| pos + p).unwrap_or(len);
                        let c = text[pos..end].trim().to_string();
                        pos = end;
                        c
                    };
                    cells.push((content, false));
                }
                "loc_col_header" => {
                    let close_tag = "</loc_col_header>";
                    let content = if let Some(close_pos) = text[pos..].find(close_tag) {
                        let c = text[pos..pos + close_pos].trim().to_string();
                        pos += close_pos + close_tag.len();
                        c
                    } else {
                        let end = text[pos..].find('<').map(|p| pos + p).unwrap_or(len);
                        let c = text[pos..end].trim().to_string();
                        pos = end;
                        c
                    };
                    cells.push((content, true));
                }
                _ => {
                    if tag_content.starts_with('/') {
                        continue;
                    }
                }
            }
        }

        // Unterminated row.
        (cells, pos)
    }
}

impl Default for DocTagsParser {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;
    use std::sync::Arc;

    fn test_store() -> Arc<OxigraphStore> {
        Arc::new(OxigraphStore::new().expect("failed to create test store"))
    }

    // -- Basic element parsing --

    #[test]
    fn parse_simple_title_and_paragraphs() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test1";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag><page>\
<loc_title>Document Title</loc_title>\
<loc_body>First paragraph.</loc_body>\
<loc_body>Second paragraph.</loc_body>\
</page></doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 3);

        // Verify the title was created with the correct class.
        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el a <{}> }} }}",
            ont::iri(ont::CLASS_TITLE),
        );
        let result = store.query_to_json(&sparql).unwrap();
        let rows = result.as_array().unwrap();
        assert_eq!(rows.len(), 1);

        // Verify two paragraphs.
        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el a <{}> }} }}",
            ont::iri(ont::CLASS_PARAGRAPH),
        );
        let result = store.query_to_json(&sparql).unwrap();
        let rows = result.as_array().unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn parse_text_content_stored() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test2";
        let parser = DocTagsParser::new();

        let doctags = "<doctag><page><loc_title>My Title</loc_title></page></doctag>";
        parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();

        let sparql = format!(
            "SELECT ?text WHERE {{ GRAPH <{g}> {{ ?el a <{cls}> . ?el <{prop}> ?text }} }}",
            cls = ont::iri(ont::CLASS_TITLE),
            prop = ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql).unwrap();
        let rows = result.as_array().unwrap();
        assert_eq!(rows.len(), 1);
    }

    // -- Section headers --

    #[test]
    fn parse_section_headers() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test3";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag><page>\
<loc_section_header>Section 1</loc_section_header>\
<loc_body>Body text.</loc_body>\
<loc_section_header>Section 2</loc_section_header>\
</page></doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 3);

        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el a <{}> }} }}",
            ont::iri(ont::CLASS_SECTION_HEADER),
        );
        let result = store.query_to_json(&sparql).unwrap();
        let rows = result.as_array().unwrap();
        assert_eq!(rows.len(), 2);
    }

    // -- List items --

    #[test]
    fn parse_list_items() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test4";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag><page>\
<loc_list_item>Item 1</loc_list_item>\
<loc_list_item>Item 2</loc_list_item>\
<loc_list_item>Item 3</loc_list_item>\
</page></doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 3);

        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el a <{}> }} }}",
            ont::iri(ont::CLASS_LIST_ITEM),
        );
        let result = store.query_to_json(&sparql).unwrap();
        let rows = result.as_array().unwrap();
        assert_eq!(rows.len(), 3);
    }

    // -- Formula, code, caption, footnote --

    #[test]
    fn parse_formula_code_caption_footnote() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test5";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag><page>\
<loc_formula>E = mc^2</loc_formula>\
<loc_code>fn main() {}</loc_code>\
<loc_caption>Figure 1: A caption</loc_caption>\
<loc_footnote>A footnote.</loc_footnote>\
</page></doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 4);

        // Verify each type.
        for (class, expected) in [
            (ont::CLASS_FORMULA, 1),
            (ont::CLASS_CODE, 1),
            (ont::CLASS_CAPTION, 1),
            (ont::CLASS_FOOTNOTE, 1),
        ] {
            let sparql = format!(
                "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el a <{}> }} }}",
                ont::iri(class),
            );
            let result = store.query_to_json(&sparql).unwrap();
            let rows = result.as_array().unwrap();
            assert_eq!(
                rows.len(),
                expected,
                "expected {expected} elements of type {class}"
            );
        }
    }

    // -- Page header and footer --

    #[test]
    fn parse_page_header_and_footer() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test6";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag><page>\
<loc_header>Page Header</loc_header>\
<loc_body>Body text.</loc_body>\
<loc_footer>Page Footer</loc_footer>\
</page></doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 3);

        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el a <{}> }} }}",
            ont::iri(ont::CLASS_PAGE_HEADER),
        );
        let result = store.query_to_json(&sparql).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1);

        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el a <{}> }} }}",
            ont::iri(ont::CLASS_PAGE_FOOTER),
        );
        let result = store.query_to_json(&sparql).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1);
    }

    // -- Picture element --

    #[test]
    fn parse_picture_element() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test-pic";
        let parser = DocTagsParser::new();

        let doctags = "<doctag><page><loc_picture>A photo</loc_picture></page></doctag>";
        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 1);

        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el a <{}> }} }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
        );
        let result = store.query_to_json(&sparql).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1);
    }

    // -- Table parsing --

    #[test]
    fn parse_table_with_headers_and_cells() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test7";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag><page>\
<loc_table>\
<loc_row><loc_col_header>Col A</loc_col_header><loc_col_header>Col B</loc_col_header></loc_row>\
<loc_row><loc_cell>1</loc_cell><loc_cell>2</loc_cell></loc_row>\
<loc_row><loc_cell>3</loc_cell><loc_cell>4</loc_cell></loc_row>\
</loc_table>\
</page></doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        // 1 table + 6 cells = 7
        assert_eq!(count, 7);

        // Verify the table element.
        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1);

        // Verify cell count.
        let sparql = format!(
            "SELECT ?cell WHERE {{ GRAPH <{g}> {{ ?cell a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_CELL),
        );
        let result = store.query_to_json(&sparql).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 6);

        // Verify header cells have isHeader set.
        let sparql = format!(
            "SELECT ?cell WHERE {{ GRAPH <{g}> {{ ?cell <{}> ?h }} }}",
            ont::iri(ont::PROP_IS_HEADER),
        );
        let result = store.query_to_json(&sparql).unwrap();
        // Header cells should be 2 (only headers get the isHeader property).
        assert_eq!(result.as_array().unwrap().len(), 2);

        // Verify row count and column count.
        let sparql = format!(
            "SELECT ?rc WHERE {{ GRAPH <{g}> {{ ?t a <{cls}> . ?t <{prop}> ?rc }} }}",
            cls = ont::iri(ont::CLASS_TABLE_ELEMENT),
            prop = ont::iri(ont::PROP_ROW_COUNT),
        );
        let result = store.query_to_json(&sparql).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1);
    }

    // -- Multi-page --

    #[test]
    fn parse_multi_page_output() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test8";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag>\
<page><loc_title>Page 1 Title</loc_title><loc_body>Page 1 body.</loc_body></page>\
<page><loc_title>Page 2 Title</loc_title></page>\
</doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 3);

        // Verify that page 1 elements link to page-1 and page 2 elements link to page-2.
        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el <{prop}> <{g}/page-1> }} }}",
            prop = ont::iri(ont::PROP_ON_PAGE),
        );
        let result = store.query_to_json(&sparql).unwrap();
        assert_eq!(
            result.as_array().unwrap().len(),
            2,
            "page 1 should have 2 elements"
        );

        let sparql = format!(
            "SELECT ?el WHERE {{ GRAPH <{g}> {{ ?el <{prop}> <{g}/page-2> }} }}",
            prop = ont::iri(ont::PROP_ON_PAGE),
        );
        let result = store.query_to_json(&sparql).unwrap();
        assert_eq!(
            result.as_array().unwrap().len(),
            1,
            "page 2 should have 1 element"
        );
    }

    // -- Reading order is sequential --

    #[test]
    fn reading_order_is_sequential() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test9";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag><page>\
<loc_title>Title</loc_title>\
<loc_body>Para 1</loc_body>\
<loc_body>Para 2</loc_body>\
</page></doctag>";

        parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();

        // Query reading order values.
        let sparql = format!(
            "SELECT ?el ?order WHERE {{ GRAPH <{g}> {{ ?el <{}> ?order }} }} ORDER BY ?order",
            ont::iri(ont::PROP_READING_ORDER),
        );
        let result = store.query_to_json(&sparql).unwrap();
        let rows = result.as_array().unwrap();
        assert_eq!(rows.len(), 3);
    }

    // -- Malformed input handling --

    #[test]
    fn handle_missing_closing_tags() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test10";
        let parser = DocTagsParser::new();

        // Missing </loc_body> closing tag.
        let doctags = "<doctag><page><loc_body>Unclosed paragraph<loc_title>Title</loc_title></page></doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        // Should still parse what it can.
        assert!(count >= 1);
    }

    #[test]
    fn handle_empty_input() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test11";
        let parser = DocTagsParser::new();

        let count = parser.parse_into_graph("", store.as_ref(), g, 1).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn handle_no_tags() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test12";
        let parser = DocTagsParser::new();

        let count = parser
            .parse_into_graph("just plain text without any tags", store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn handle_extra_whitespace() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test13";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag>  <page>
  <loc_title>  Spaced Title  </loc_title>
  <loc_body>  Body with spaces  </loc_body>
</page>  </doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 2);

        // Verify that whitespace is trimmed from the text content.
        let sparql = format!(
            "SELECT ?text WHERE {{ GRAPH <{g}> {{ ?el a <{cls}> . ?el <{prop}> ?text }} }}",
            cls = ont::iri(ont::CLASS_TITLE),
            prop = ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql).unwrap();
        let rows = result.as_array().unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn handle_unknown_tags() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test14";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag><page>\
<loc_title>Title</loc_title>\
<loc_unknown>Unknown content</loc_unknown>\
<loc_body>Body</loc_body>\
</page></doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        // Unknown tags are skipped, so only title + body = 2.
        assert_eq!(count, 2);
    }

    // -- Ontology class correctness --

    #[test]
    fn correct_ontology_classes() {
        // Verify the tag-to-class mapping.
        assert_eq!(tag_to_ontology_class("loc_title"), Some(ont::CLASS_TITLE));
        assert_eq!(
            tag_to_ontology_class("loc_section_header"),
            Some(ont::CLASS_SECTION_HEADER)
        );
        assert_eq!(
            tag_to_ontology_class("loc_body"),
            Some(ont::CLASS_PARAGRAPH)
        );
        assert_eq!(
            tag_to_ontology_class("loc_list_item"),
            Some(ont::CLASS_LIST_ITEM)
        );
        assert_eq!(
            tag_to_ontology_class("loc_caption"),
            Some(ont::CLASS_CAPTION)
        );
        assert_eq!(
            tag_to_ontology_class("loc_formula"),
            Some(ont::CLASS_FORMULA)
        );
        assert_eq!(tag_to_ontology_class("loc_code"), Some(ont::CLASS_CODE));
        assert_eq!(
            tag_to_ontology_class("loc_footnote"),
            Some(ont::CLASS_FOOTNOTE)
        );
        assert_eq!(
            tag_to_ontology_class("loc_header"),
            Some(ont::CLASS_PAGE_HEADER)
        );
        assert_eq!(
            tag_to_ontology_class("loc_footer"),
            Some(ont::CLASS_PAGE_FOOTER)
        );
        assert_eq!(
            tag_to_ontology_class("loc_picture"),
            Some(ont::CLASS_PICTURE_ELEMENT)
        );
        // Structural tags return None.
        assert_eq!(tag_to_ontology_class("doctag"), None);
        assert_eq!(tag_to_ontology_class("page"), None);
        assert_eq!(tag_to_ontology_class("loc_table"), None);
        assert_eq!(tag_to_ontology_class("loc_row"), None);
        assert_eq!(tag_to_ontology_class("loc_cell"), None);
        assert_eq!(tag_to_ontology_class("loc_col_header"), None);
    }

    // -- Default trait --

    #[test]
    fn doctags_parser_default() {
        let parser = DocTagsParser::default();
        let store = test_store();
        let g = "urn:ruddydoc:doc:test-default";
        let count = parser
            .parse_into_graph("<doctag><page></page></doctag>", store.as_ref(), g, 1)
            .unwrap();
        assert_eq!(count, 0);
    }

    // -- Comprehensive document --

    #[test]
    fn parse_comprehensive_document() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:test-full";
        let parser = DocTagsParser::new();

        let doctags = "\
<doctag><page>\
<loc_header>Page Header Text</loc_header>\
<loc_title>Document Title</loc_title>\
<loc_body>This is a paragraph of body text.</loc_body>\
<loc_section_header>Section 1</loc_section_header>\
<loc_body>More body text here.</loc_body>\
<loc_list_item>First item</loc_list_item>\
<loc_list_item>Second item</loc_list_item>\
<loc_table>\
<loc_row><loc_col_header>Col A</loc_col_header><loc_col_header>Col B</loc_col_header></loc_row>\
<loc_row><loc_cell>1</loc_cell><loc_cell>2</loc_cell></loc_row>\
</loc_table>\
<loc_caption>Figure 1: A caption</loc_caption>\
<loc_formula>E = mc^2</loc_formula>\
<loc_code>fn main() {}</loc_code>\
<loc_footnote>A footnote.</loc_footnote>\
</page></doctag>";

        let count = parser
            .parse_into_graph(doctags, store.as_ref(), g, 1)
            .unwrap();
        // 1 header + 1 title + 2 body + 1 section_header + 2 list_items +
        // 1 table + 4 cells + 1 caption + 1 formula + 1 code + 1 footnote = 16
        assert_eq!(count, 16);

        // Verify total triple count is substantial.
        let triple_count = store.triple_count_in(g).unwrap();
        assert!(
            triple_count > 30,
            "expected >30 triples, got {triple_count}"
        );
    }
}
