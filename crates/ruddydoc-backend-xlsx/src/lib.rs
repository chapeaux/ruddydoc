//! XLSX parser backend for RuddyDoc.
//!
//! Uses the `calamine` crate to parse Excel spreadsheets (.xlsx, .xls)
//! into table elements in the document ontology graph. Each worksheet
//! becomes a separate `rdoc:TableElement`, and each cell becomes a
//! `rdoc:TableCell` with row/column indices and text content.

use calamine::{Data, Dimensions, Reader, Sheets, open_workbook_auto, open_workbook_auto_from_rs};
use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

/// XLSX document backend.
pub struct XlsxBackend;

impl XlsxBackend {
    /// Create a new XLSX backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for XlsxBackend {
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

/// Hex-encode bytes without pulling in an external hex crate.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Convert a calamine `Data` cell value to a string representation.
fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        other => other.to_string(),
    }
}

/// Fetch the merged-cell regions for a worksheet, if the concrete workbook
/// format exposes them. `open_workbook_auto`'s generic `Reader` trait has no
/// merge-cell method (that API only exists on the concrete `Xlsx`/`Xls`
/// structs), so this matches on the `Sheets` variant to reach it. Xlsb/Ods
/// have no merge-cell support in calamine and fall back to an empty list --
/// `is_valid` only accepts `.xlsx`/`.xls` anyway.
fn merge_cells_for_sheet<RS>(workbook: &mut Sheets<RS>, name: &str) -> Vec<Dimensions>
where
    RS: std::io::Read + std::io::Seek,
{
    match workbook {
        Sheets::Xlsx(wb) => wb
            .worksheet_merge_cells(name)
            .and_then(|r| r.ok())
            .unwrap_or_default(),
        Sheets::Xls(wb) => wb.worksheet_merge_cells(name).unwrap_or_default(),
        _ => Vec::new(),
    }
}

impl DocumentBackend for XlsxBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Xlsx]
    }

    fn supports_pagination(&self) -> bool {
        false
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("xlsx" | "xls")
                )
            }
            DocumentSource::Stream { name, .. } => {
                name.ends_with(".xlsx") || name.ends_with(".xls")
            }
        }
    }

    fn parse(
        &self,
        source: &DocumentSource,
        store: &dyn DocumentStore,
        doc_graph: &str,
    ) -> ruddydoc_core::Result<DocumentMeta> {
        // Read source content and open workbook
        let (raw_bytes, file_path, file_name) = match source {
            DocumentSource::File(path) => {
                let bytes = std::fs::read(path)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".to_string());
                (bytes, Some(path.clone()), name)
            }
            DocumentSource::Stream { name, data } => (data.clone(), None, name.clone()),
        };

        let file_size = raw_bytes.len() as u64;
        let hash_str = compute_hash(&raw_bytes);
        let doc_hash = DocumentHash(hash_str.clone());

        // Open workbook: use path-based opener for files (better extension detection),
        // fall back to stream-based opener for in-memory data.
        let sheets: Vec<(String, Vec<Vec<Data>>, Vec<Dimensions>)> = match source {
            DocumentSource::File(path) => {
                let mut workbook = open_workbook_auto(path)?;
                let sheet_names = workbook.sheet_names().to_vec();
                let mut result = Vec::new();
                for name in sheet_names {
                    let range = workbook.worksheet_range(&name)?;
                    let rows: Vec<Vec<Data>> = range.rows().map(|r| r.to_vec()).collect();
                    let merges = merge_cells_for_sheet(&mut workbook, &name);
                    result.push((name, rows, merges));
                }
                result
            }
            DocumentSource::Stream { data, .. } => {
                let cursor = std::io::Cursor::new(data.clone());
                let mut workbook = open_workbook_auto_from_rs(cursor)?;
                let sheet_names = workbook.sheet_names().to_vec();
                let mut result = Vec::new();
                for name in sheet_names {
                    let range = workbook.worksheet_range(&name)?;
                    let rows: Vec<Vec<Data>> = range.rows().map(|r| r.to_vec()).collect();
                    let merges = merge_cells_for_sheet(&mut workbook, &name);
                    result.push((name, rows, merges));
                }
                result
            }
        };

        // Create the document node
        let doc_iri = ruddydoc_core::doc_iri(&hash_str);
        let rdf_type = ont::rdf_iri("type");
        let g = doc_graph;

        store.insert_triple_into(&doc_iri, &rdf_type, &ont::iri(ont::CLASS_DOCUMENT), g)?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_SOURCE_FORMAT),
            "xlsx",
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

        // Process each worksheet as a separate TableElement
        for (sheet_idx, (sheet_name, rows, merges)) in sheets.iter().enumerate() {
            let table_iri = ruddydoc_core::element_iri(&hash_str, &format!("table-{sheet_idx}"));

            store.insert_triple_into(
                &table_iri,
                &rdf_type,
                &ont::iri(ont::CLASS_TABLE_ELEMENT),
                g,
            )?;

            // Reading order = sheet index
            store.insert_literal(
                &table_iri,
                &ont::iri(ont::PROP_READING_ORDER),
                &sheet_idx.to_string(),
                "integer",
                g,
            )?;

            // Store sheet name as textContent on the table element
            store.insert_literal(
                &table_iri,
                &ont::iri(ont::PROP_TEXT_CONTENT),
                sheet_name,
                "string",
                g,
            )?;

            // Link document to table
            store.insert_triple_into(&doc_iri, &ont::iri(ont::PROP_HAS_ELEMENT), &table_iri, g)?;

            // Compute row and column counts
            let row_count = rows.len();
            let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);

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
                &max_cols.to_string(),
                "integer",
                g,
            )?;

            // Insert cells. A merged region only gets one TableCell, anchored
            // at its top-left position and carrying the region's row/col
            // span; every other position it covers is skipped rather than
            // emitted as its own blank cell.
            for (row_idx, row) in rows.iter().enumerate() {
                let is_header = row_idx == 0;
                for col_idx in 0..max_cols {
                    let pos = (row_idx as u32, col_idx as u32);
                    let covering_merge = merges.iter().find(|d| d.contains(pos.0, pos.1));
                    if let Some(merge) = covering_merge
                        && merge.start != pos
                    {
                        continue;
                    }
                    let (row_span, col_span) = covering_merge
                        .map(|d| (d.end.0 - d.start.0 + 1, d.end.1 - d.start.1 + 1))
                        .unwrap_or((1, 1));

                    let cell = row.get(col_idx).unwrap_or(&Data::Empty);
                    let cell_text = cell_to_string(cell);

                    let cell_iri = ruddydoc_core::element_iri(
                        &hash_str,
                        &format!("table-{sheet_idx}-cell-{row_idx}-{col_idx}"),
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
                        &cell_text,
                        "string",
                        g,
                    )?;
                    store.insert_literal(
                        &cell_iri,
                        &ont::iri(ont::PROP_IS_HEADER),
                        if is_header { "true" } else { "false" },
                        "boolean",
                        g,
                    )?;
                    store.insert_literal(
                        &cell_iri,
                        &ont::iri(ont::PROP_CELL_ROW_SPAN),
                        &row_span.to_string(),
                        "integer",
                        g,
                    )?;
                    store.insert_literal(
                        &cell_iri,
                        &ont::iri(ont::PROP_CELL_COL_SPAN),
                        &col_span.to_string(),
                        "integer",
                        g,
                    )?;
                }
            }
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Xlsx,
            file_size,
            page_count: None,
            language: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::SparqStore;
    use std::io::Write;

    // -----------------------------------------------------------------------
    // Minimal XLSX builder for test fixtures
    // -----------------------------------------------------------------------

    /// Build a minimal XLSX file in memory with the given sheets.
    /// Each sheet is a tuple of (name, rows) where rows is a Vec<Vec<String>>.
    fn build_xlsx(sheets: &[(&str, &[&[&str]])]) -> Vec<u8> {
        let with_merges: Vec<(&str, &[&[&str]], &[&str])> = sheets
            .iter()
            .map(|(name, rows)| (*name, *rows, &[][..]))
            .collect();
        build_xlsx_with_merges(&with_merges)
    }

    /// Like `build_xlsx`, but each sheet also carries a list of merged-cell
    /// ranges (e.g. `"A1:B1"`) to write into a `<mergeCells>` block.
    fn build_xlsx_with_merges(sheets: &[(&str, &[&[&str]], &[&str])]) -> Vec<u8> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        // [Content_Types].xml
        let mut content_types = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        );
        for (i, _) in sheets.iter().enumerate() {
            content_types.push_str(&format!(
                r#"
  <Override PartName="/xl/worksheets/sheet{}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
                i + 1
            ));
        }
        // Shared strings
        content_types.push_str(
            r#"
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>"#,
        );
        content_types.push_str("\n</Types>");

        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(content_types.as_bytes()).unwrap();

        // _rels/.rels
        zip.start_file("_rels/.rels", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
        )
        .unwrap();

        // Collect all unique strings for shared strings table
        let mut all_strings: Vec<String> = Vec::new();
        for (_, rows, _) in sheets {
            for row in *rows {
                for cell in *row {
                    if !cell.is_empty() {
                        let s = cell.to_string();
                        if !all_strings.contains(&s) {
                            all_strings.push(s);
                        }
                    }
                }
            }
        }

        // xl/sharedStrings.xml
        let mut ss_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{}" uniqueCount="{}">"#,
            all_strings.len(),
            all_strings.len()
        );
        for s in &all_strings {
            ss_xml.push_str(&format!("<si><t>{}</t></si>", xml_escape(s)));
        }
        ss_xml.push_str("</sst>");

        zip.start_file("xl/sharedStrings.xml", options).unwrap();
        zip.write_all(ss_xml.as_bytes()).unwrap();

        // xl/workbook.xml
        let mut wb = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>"#,
        );
        for (i, (name, _, _)) in sheets.iter().enumerate() {
            wb.push_str(&format!(
                r#"
    <sheet name="{}" sheetId="{}" r:id="rId{}"/>"#,
                xml_escape(name),
                i + 1,
                i + 1
            ));
        }
        wb.push_str(
            r#"
  </sheets>
</workbook>"#,
        );

        zip.start_file("xl/workbook.xml", options).unwrap();
        zip.write_all(wb.as_bytes()).unwrap();

        // xl/_rels/workbook.xml.rels
        let mut wb_rels = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        );
        for (i, _) in sheets.iter().enumerate() {
            wb_rels.push_str(&format!(
                r#"
  <Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{}.xml"/>"#,
                i + 1,
                i + 1
            ));
        }
        // shared strings relationship
        wb_rels.push_str(&format!(
            r#"
  <Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#,
            sheets.len() + 1
        ));
        wb_rels.push_str("\n</Relationships>");

        zip.start_file("xl/_rels/workbook.xml.rels", options)
            .unwrap();
        zip.write_all(wb_rels.as_bytes()).unwrap();

        // Worksheet files
        for (i, (_, rows, merges)) in sheets.iter().enumerate() {
            let mut ws = String::from(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>"#,
            );

            for (row_idx, row) in rows.iter().enumerate() {
                ws.push_str(&format!("\n    <row r=\"{}\">", row_idx + 1));
                for (col_idx, cell) in row.iter().enumerate() {
                    let col_letter = col_index_to_letter(col_idx);
                    let cell_ref = format!("{}{}", col_letter, row_idx + 1);

                    if cell.is_empty() {
                        // Skip empty cells
                        continue;
                    }

                    // Try to parse as number
                    if let Ok(n) = cell.parse::<f64>() {
                        ws.push_str(&format!("\n      <c r=\"{cell_ref}\"><v>{n}</v></c>"));
                    } else if *cell == "true" || *cell == "false" {
                        let v = if *cell == "true" { "1" } else { "0" };
                        ws.push_str(&format!(
                            "\n      <c r=\"{cell_ref}\" t=\"b\"><v>{v}</v></c>"
                        ));
                    } else {
                        // Use shared string index
                        let idx = all_strings.iter().position(|s| s == *cell).unwrap();
                        ws.push_str(&format!(
                            "\n      <c r=\"{cell_ref}\" t=\"s\"><v>{idx}</v></c>"
                        ));
                    }
                }
                ws.push_str("\n    </row>");
            }

            ws.push_str("\n  </sheetData>");

            if !merges.is_empty() {
                ws.push_str(&format!("\n  <mergeCells count=\"{}\">", merges.len()));
                for m in *merges {
                    ws.push_str(&format!("\n    <mergeCell ref=\"{m}\"/>"));
                }
                ws.push_str("\n  </mergeCells>");
            }

            ws.push_str("\n</worksheet>");

            zip.start_file(format!("xl/worksheets/sheet{}.xml", i + 1), options)
                .unwrap();
            zip.write_all(ws.as_bytes()).unwrap();
        }

        let cursor = zip.finish().unwrap();
        cursor.into_inner()
    }

    /// Convert a 0-based column index to an Excel column letter (A, B, ..., Z, AA, AB, ...).
    fn col_index_to_letter(idx: usize) -> String {
        let mut result = String::new();
        let mut n = idx;
        loop {
            result.insert(0, (b'A' + (n % 26) as u8) as char);
            if n < 26 {
                break;
            }
            n = n / 26 - 1;
        }
        result
    }

    /// Escape XML special characters.
    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    // -----------------------------------------------------------------------
    // Test helper
    // -----------------------------------------------------------------------

    fn parse_xlsx(
        xlsx_data: &[u8],
        file_name: &str,
    ) -> ruddydoc_core::Result<(SparqStore, DocumentMeta, String)> {
        let store = SparqStore::new()?;
        let backend = XlsxBackend::new();
        let source = DocumentSource::Stream {
            name: file_name.to_string(),
            data: xlsx_data.to_vec(),
        };

        let hash_str = compute_hash(xlsx_data);
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn single_sheet_with_headers_and_data() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[(
            "Sheet1",
            &[
                &["Name", "Age", "City"],
                &["Alice", "30", "NYC"],
                &["Bob", "25", "LA"],
            ],
        )]);
        let (store, meta, graph) = parse_xlsx(&xlsx, "sample.xlsx")?;

        assert_eq!(meta.format, InputFormat::Xlsx);
        assert!(meta.page_count.is_none());

        // Check table exists
        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?t a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check row count = 3
        let sparql_rows = format!(
            "SELECT ?rc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?rc \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_ROW_COUNT),
        );
        let result = store.query_to_json(&sparql_rows)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let rc = rows[0]["rc"].as_str().expect("rc");
        assert!(rc.contains('3'));

        // Check column count = 3
        let sparql_cols = format!(
            "SELECT ?cc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?cc \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_COLUMN_COUNT),
        );
        let result = store.query_to_json(&sparql_cols)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let cc = rows[0]["cc"].as_str().expect("cc");
        assert!(cc.contains('3'));

        // Check total cells: 3 rows * 3 cols = 9
        let sparql_cells = format!(
            "SELECT ?c WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
        );
        let result = store.query_to_json(&sparql_cells)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 9);

        Ok(())
    }

    #[test]
    fn multi_sheet_workbook() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[
            ("Sales", &[&["Product", "Revenue"], &["Widget", "1000"]]),
            (
                "Inventory",
                &[&["Item", "Count"], &["Bolt", "500"], &["Nut", "300"]],
            ),
        ]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "multi.xlsx")?;

        // Should have 2 table elements
        let sparql_tables = format!(
            "SELECT ?t WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql_tables)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);

        // Check reading orders (0 and 1)
        let sparql_orders = format!(
            "SELECT ?order WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?order \
               }} \
             }} ORDER BY ?order",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_READING_ORDER),
        );
        let result = store.query_to_json(&sparql_orders)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);
        assert!(rows[0]["order"].as_str().expect("order").contains('0'));
        assert!(rows[1]["order"].as_str().expect("order").contains('1'));

        // Check sheet names are stored
        let sparql_names = format!(
            "SELECT ?name WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?name \
               }} \
             }} ORDER BY ?name",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql_names)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);
        let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
        // SPARQL string literals include type info, just check the values contain expected names
        assert!(names.iter().any(|n| n.contains("Inventory")));
        assert!(names.iter().any(|n| n.contains("Sales")));

        // First sheet: 2 rows * 2 cols = 4 cells, Second sheet: 3 rows * 2 cols = 6 cells
        // Total = 10 cells
        let sparql_cells = format!(
            "SELECT ?c WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
        );
        let result = store.query_to_json(&sparql_cells)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 10);

        Ok(())
    }

    #[test]
    fn header_row_is_marked() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[("Sheet1", &[&["Name", "Age"], &["Alice", "30"]])]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "headers.xlsx")?;

        // Header cells (row 0) should have isHeader = true
        let sparql_headers = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> \"true\"^^<http://www.w3.org/2001/XMLSchema#boolean> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_IS_HEADER),
        );
        let result = store.query_to_json(&sparql_headers)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2); // "Name" and "Age"

        // Non-header cells should have isHeader = false
        let sparql_non_headers = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> \"false\"^^<http://www.w3.org/2001/XMLSchema#boolean> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_IS_HEADER),
        );
        let result = store.query_to_json(&sparql_non_headers)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2); // "Alice" and "30"

        Ok(())
    }

    #[test]
    fn different_cell_types() -> ruddydoc_core::Result<()> {
        // String, number, boolean, empty
        let xlsx = build_xlsx(&[(
            "Types",
            &[
                &["Header"],
                &["text"],
                &["42"],
                &["true"],
                &[""], // empty cell (will be skipped in builder)
            ],
        )]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "types.xlsx")?;

        // Check that the numeric cell "42" exists
        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> \"2\"^^<http://www.w3.org/2001/XMLSchema#integer> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("42"));

        // Check that the boolean cell exists
        let sparql_bool = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> \"3\"^^<http://www.w3.org/2001/XMLSchema#integer> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW),
        );
        let result = store.query_to_json(&sparql_bool)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("true") || text.contains("TRUE"));

        Ok(())
    }

    #[test]
    fn row_and_column_counts() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[(
            "Data",
            &[
                &["A", "B", "C", "D"],
                &["1", "2", "3", "4"],
                &["5", "6", "7", "8"],
            ],
        )]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "counts.xlsx")?;

        // Row count = 3
        let sparql_rows = format!(
            "SELECT ?rc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?rc \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_ROW_COUNT),
        );
        let result = store.query_to_json(&sparql_rows)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let rc = rows[0]["rc"].as_str().expect("rc");
        assert!(rc.contains('3'));

        // Column count = 4
        let sparql_cols = format!(
            "SELECT ?cc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?cc \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_COLUMN_COUNT),
        );
        let result = store.query_to_json(&sparql_cols)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let cc = rows[0]["cc"].as_str().expect("cc");
        assert!(cc.contains('4'));

        Ok(())
    }

    #[test]
    fn cell_row_and_column_indices() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[("Sheet1", &[&["A", "B"], &["C", "D"]])]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "indices.xlsx")?;

        // Query cell at row 1, column 1 (should be "D")
        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> \"1\"^^<http://www.w3.org/2001/XMLSchema#integer>. \
                 ?c <{}> \"1\"^^<http://www.w3.org/2001/XMLSchema#integer> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW),
            ont::iri(ont::PROP_CELL_COLUMN),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains('D'));

        Ok(())
    }

    #[test]
    fn merged_header_cell_gets_col_span_and_no_duplicate_blanks() -> ruddydoc_core::Result<()> {
        // A1:B1 merged ("Full Width Header" spans 2 columns), then a normal
        // 2-column data row.
        let xlsx = build_xlsx_with_merges(&[(
            "Sheet1",
            &[&["Full Width Header"], &["Alice", "30"]],
            &["A1:B1"],
        )]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "merged.xlsx")?;

        // Only 3 cells total: the merged header (1, not 2) + the 2 data
        // cells. Before the fix, the merge's second position (B1) would
        // also get its own blank TableCell, making it 4.
        let sparql_cells = format!(
            "SELECT ?c WHERE {{ GRAPH <{graph}> {{ ?c a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_CELL),
        );
        let result = store.query_to_json(&sparql_cells)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 3, "expected no blank cell for the merge's B1 span");

        // The anchor cell (row 0, col 0) must carry colSpan = 2.
        let sparql = format!(
            "SELECT ?text ?cs WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> \"0\"^^<http://www.w3.org/2001/XMLSchema#integer>. \
                 ?c <{}> \"0\"^^<http://www.w3.org/2001/XMLSchema#integer>. \
                 ?c <{}> ?cs \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW),
            ont::iri(ont::PROP_CELL_COLUMN),
            ont::iri(ont::PROP_CELL_COL_SPAN),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Full Width Header"));
        let cs = rows[0]["cs"].as_str().expect("cs");
        assert!(cs.contains('2'), "expected colSpan 2, got {cs:?}");

        // A non-merged cell must default to rowSpan/colSpan = 1.
        let sparql_normal = format!(
            "SELECT ?rs ?cs WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> \"1\"^^<http://www.w3.org/2001/XMLSchema#integer>. \
                 ?c <{}> \"0\"^^<http://www.w3.org/2001/XMLSchema#integer>. \
                 ?c <{}> ?rs. \
                 ?c <{}> ?cs \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_ROW),
            ont::iri(ont::PROP_CELL_COLUMN),
            ont::iri(ont::PROP_CELL_ROW_SPAN),
            ont::iri(ont::PROP_CELL_COL_SPAN),
        );
        let result_normal = store.query_to_json(&sparql_normal)?;
        let normal_rows = result_normal.as_array().expect("expected array");
        assert_eq!(normal_rows.len(), 1);
        assert!(normal_rows[0]["rs"].as_str().expect("rs").contains('1'));
        assert!(normal_rows[0]["cs"].as_str().expect("cs").contains('1'));

        Ok(())
    }

    #[test]
    fn merged_row_span_cell() -> ruddydoc_core::Result<()> {
        // A1:A2 merged vertically (rowspan 2), plus a distinct value in each
        // row's second column.
        let xlsx = build_xlsx_with_merges(&[(
            "Sheet1",
            &[&["Spans 2 rows", "Top"], &["", "Bottom"]],
            &["A1:A2"],
        )]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "rowspan.xlsx")?;

        let sparql = format!(
            "SELECT ?text ?rs WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> \"0\"^^<http://www.w3.org/2001/XMLSchema#integer>. \
                 ?c <{}> \"0\"^^<http://www.w3.org/2001/XMLSchema#integer>. \
                 ?c <{}> ?rs \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW),
            ont::iri(ont::PROP_CELL_COLUMN),
            ont::iri(ont::PROP_CELL_ROW_SPAN),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Spans 2 rows"));
        let rs = rows[0]["rs"].as_str().expect("rs");
        assert!(rs.contains('2'), "expected rowSpan 2, got {rs:?}");

        // No TableCell should exist at (row=1, col=0) -- it's covered by the
        // merge anchored at (0, 0).
        let sparql_covered = format!(
            "ASK {{ GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> \"1\"^^<http://www.w3.org/2001/XMLSchema#integer>. \
                 ?c <{}> \"0\"^^<http://www.w3.org/2001/XMLSchema#integer> \
               }} }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_ROW),
            ont::iri(ont::PROP_CELL_COLUMN),
        );
        let result_covered = store.query_to_json(&sparql_covered)?;
        assert_eq!(result_covered, serde_json::Value::Bool(false));

        Ok(())
    }

    #[test]
    fn is_valid_accepts_xlsx_and_xls() {
        let backend = XlsxBackend::new();

        let xlsx_file = DocumentSource::File(std::path::PathBuf::from("data.xlsx"));
        assert!(backend.is_valid(&xlsx_file));

        let xls_file = DocumentSource::File(std::path::PathBuf::from("data.xls"));
        assert!(backend.is_valid(&xls_file));

        let csv_file = DocumentSource::File(std::path::PathBuf::from("data.csv"));
        assert!(!backend.is_valid(&csv_file));

        let xlsx_stream = DocumentSource::Stream {
            name: "file.xlsx".to_string(),
            data: vec![],
        };
        assert!(backend.is_valid(&xlsx_stream));

        let xls_stream = DocumentSource::Stream {
            name: "file.xls".to_string(),
            data: vec![],
        };
        assert!(backend.is_valid(&xls_stream));

        let other_stream = DocumentSource::Stream {
            name: "file.txt".to_string(),
            data: vec![],
        };
        assert!(!backend.is_valid(&other_stream));
    }

    #[test]
    fn document_metadata() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[("Sheet1", &[&["X", "Y"], &["1", "2"]])]);
        let (store, meta, graph) = parse_xlsx(&xlsx, "meta.xlsx")?;

        assert_eq!(meta.format, InputFormat::Xlsx);
        assert!(meta.page_count.is_none());
        assert!(meta.file_path.is_none());

        // Check sourceFormat = "xlsx"
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
        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("xlsx"));

        // Check fileName = "meta.xlsx"
        let sparql_name = format!(
            "SELECT ?name WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?name \
               }} \
             }}",
            ont::iri(ont::PROP_FILE_NAME),
        );
        let result = store.query_to_json(&sparql_name)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let name = rows[0]["name"].as_str().expect("name");
        assert!(name.contains("meta.xlsx"));

        // Check documentHash is present
        let sparql_hash = format!(
            "SELECT ?hash WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?hash \
               }} \
             }}",
            ont::iri(ont::PROP_DOCUMENT_HASH),
        );
        let result = store.query_to_json(&sparql_hash)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        Ok(())
    }

    #[test]
    fn reading_order_on_table() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[("Sheet1", &[&["A", "B"], &["1", "2"]])]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "order.xlsx")?;

        // Table should have readingOrder = 0
        let sparql = format!(
            "SELECT ?order WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?order \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_READING_ORDER),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let order = rows[0]["order"].as_str().expect("order");
        assert!(order.contains('0'));

        Ok(())
    }

    #[test]
    fn has_element_links_table() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[("Sheet1", &[&["X"], &["1"]])]);
        let (store, meta, graph) = parse_xlsx(&xlsx, "link.xlsx")?;

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?t WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?t. \
                 ?t a <{}> \
               }} \
             }}",
            ont::iri(ont::PROP_HAS_ELEMENT),
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        Ok(())
    }

    #[test]
    fn default_trait() {
        let _backend: XlsxBackend = XlsxBackend::default();
    }

    #[test]
    fn supported_formats() {
        let backend = XlsxBackend::new();
        assert_eq!(backend.supported_formats(), &[InputFormat::Xlsx]);
        assert!(!backend.supports_pagination());
    }

    #[test]
    fn error_on_invalid_data() {
        let backend = XlsxBackend::new();
        let store = SparqStore::new().unwrap();
        let source = DocumentSource::Stream {
            name: "bad.xlsx".to_string(),
            data: b"this is not an xlsx file".to_vec(),
        };
        let result = backend.parse(&source, &store, "urn:test:graph");
        assert!(result.is_err());
    }

    #[test]
    fn sheet_name_preserved() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[("My Custom Sheet", &[&["Data"], &["Value"]])]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "named.xlsx")?;

        // Check sheet name is stored as textContent
        let sparql = format!(
            "SELECT ?name WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?name \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let name = rows[0]["name"].as_str().expect("name");
        assert!(name.contains("My Custom Sheet"));

        Ok(())
    }

    #[test]
    fn file_size_stored() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[("Sheet1", &[&["A"]])]);
        let (store, meta, graph) = parse_xlsx(&xlsx, "size.xlsx")?;

        assert!(meta.file_size > 0);

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?size WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?size \
               }} \
             }}",
            ont::iri(ont::PROP_FILE_SIZE),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        Ok(())
    }

    #[test]
    fn has_cell_links_cells_to_table() -> ruddydoc_core::Result<()> {
        let xlsx = build_xlsx(&[("Sheet1", &[&["A", "B"]])]);
        let (store, _meta, graph) = parse_xlsx(&xlsx, "celllinks.xlsx")?;

        // Check that table has hasCell links to cells
        let sparql = format!(
            "SELECT ?cell WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?cell. \
                 ?cell a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_HAS_CELL),
            ont::iri(ont::CLASS_TABLE_CELL),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2); // 1 row * 2 cols = 2 cells

        Ok(())
    }
}
