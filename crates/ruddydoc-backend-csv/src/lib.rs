//! CSV parser backend for RuddyDoc.
//!
//! Uses the `csv` crate to parse CSV/TSV files into a single table
//! element in the document ontology graph. Auto-detects delimiters
//! (comma, tab, semicolon, pipe) by choosing the one that produces
//! the most consistent column counts.

use csv::ReaderBuilder;
use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

/// CSV document backend.
pub struct CsvBackend;

impl CsvBackend {
    /// Create a new CSV backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CsvBackend {
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

/// Candidate delimiters in order of preference.
const DELIMITERS: &[u8] = b",\t;|";

/// Detect the best delimiter for the given content.
///
/// For each candidate, parse the content and measure consistency of column
/// counts across rows. The delimiter producing the lowest variance and
/// more than one column is preferred. If all produce a single column, we
/// fall back to comma.
fn detect_delimiter(content: &[u8]) -> u8 {
    let mut best_delim = b',';
    let mut best_score: Option<(usize, f64)> = None; // (max_cols, variance)

    for &delim in DELIMITERS {
        let mut rdr = ReaderBuilder::new()
            .delimiter(delim)
            .has_headers(false)
            .flexible(true)
            .from_reader(content);

        let mut col_counts: Vec<usize> = Vec::new();
        for record in rdr.records().flatten() {
            col_counts.push(record.len());
        }

        if col_counts.is_empty() {
            continue;
        }

        let max_cols = *col_counts.iter().max().unwrap_or(&0);
        let mean = col_counts.iter().sum::<usize>() as f64 / col_counts.len() as f64;
        let variance = col_counts
            .iter()
            .map(|&c| {
                let diff = c as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / col_counts.len() as f64;

        // Prefer: more columns, then lower variance
        let dominated = best_score.is_some_and(|(prev_max, prev_var)| {
            if max_cols > prev_max {
                false // new is better (more columns)
            } else if max_cols == prev_max {
                variance >= prev_var // new is not better unless lower variance
            } else {
                true // new is worse
            }
        });

        if best_score.is_none() || !dominated {
            best_delim = delim;
            best_score = Some((max_cols, variance));
        }
    }

    best_delim
}

/// Parse the CSV content into a vector of rows, each row a vector of cell strings.
/// Also returns the detected number of columns (the maximum across all rows).
fn parse_csv_records(
    content: &[u8],
    delimiter: u8,
) -> ruddydoc_core::Result<(Vec<Vec<String>>, usize)> {
    let mut rdr = ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(content);

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut max_cols: usize = 0;

    for result in rdr.records() {
        let record = result?;
        let cells: Vec<String> = record.iter().map(|f| f.to_string()).collect();
        if cells.len() > max_cols {
            max_cols = cells.len();
        }
        rows.push(cells);
    }

    // Pad rows with fewer columns to max_cols
    for row in &mut rows {
        while row.len() < max_cols {
            row.push(String::new());
        }
    }

    Ok((rows, max_cols))
}

impl DocumentBackend for CsvBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Csv]
    }

    fn supports_pagination(&self) -> bool {
        false
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("csv" | "tsv")
                )
            }
            DocumentSource::Stream { name, .. } => name.ends_with(".csv") || name.ends_with(".tsv"),
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
            "csv",
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

        // Detect delimiter and parse records
        let delimiter = detect_delimiter(content.as_bytes());
        let (rows, max_cols) = parse_csv_records(content.as_bytes(), delimiter)?;

        // Create the single TableElement
        let table_iri = ruddydoc_core::element_iri(&hash_str, "table-0");

        store.insert_triple_into(
            &table_iri,
            &rdf_type,
            &ont::iri(ont::CLASS_TABLE_ELEMENT),
            g,
        )?;

        // rdoc:readingOrder = 0 on the table element
        store.insert_literal(
            &table_iri,
            &ont::iri(ont::PROP_READING_ORDER),
            "0",
            "integer",
            g,
        )?;

        // rdoc:hasElement (document -> table)
        store.insert_triple_into(&doc_iri, &ont::iri(ont::PROP_HAS_ELEMENT), &table_iri, g)?;

        // Set row/column counts
        let row_count = rows.len();
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

        // Insert cells
        for (row_idx, row) in rows.iter().enumerate() {
            let is_header = row_idx == 0;
            for (col_idx, cell_text) in row.iter().enumerate() {
                let cell_iri =
                    ruddydoc_core::element_iri(&hash_str, &format!("cell-{row_idx}-{col_idx}"));

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
                    if is_header { "true" } else { "false" },
                    "boolean",
                    g,
                )?;
            }
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Csv,
            file_size,
            page_count: None,
            language: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;

    fn parse_csv(
        csv_data: &str,
        file_name: &str,
    ) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = CsvBackend::new();
        let source = DocumentSource::Stream {
            name: file_name.to_string(),
            data: csv_data.as_bytes().to_vec(),
        };

        let hash_str = compute_hash(csv_data.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    #[test]
    fn basic_comma_delimited() -> ruddydoc_core::Result<()> {
        let csv_data = "Name,Age,City\nAlice,30,NYC\nBob,25,LA\n";
        let (store, meta, graph) = parse_csv(csv_data, "sample.csv")?;

        assert_eq!(meta.format, InputFormat::Csv);
        assert!(meta.page_count.is_none());

        // Check table exists
        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?t a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check row count = 3 (header + 2 data rows)
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
    fn header_row_is_marked() -> ruddydoc_core::Result<()> {
        let csv_data = "Name,Age\nAlice,30\n";
        let (store, _meta, graph) = parse_csv(csv_data, "test.csv")?;

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
    fn tab_delimited() -> ruddydoc_core::Result<()> {
        let csv_data = "Name\tAge\tCity\nAlice\t30\tNYC\nBob\t25\tLA\n";
        let (store, _meta, graph) = parse_csv(csv_data, "tabs.tsv")?;

        // Check column count = 3
        let sparql = format!(
            "SELECT ?cc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?cc \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_COLUMN_COUNT),
        );
        let result = store.query_to_json(&sparql)?;
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
    fn semicolon_delimited() -> ruddydoc_core::Result<()> {
        let csv_data = "Name;Age;City\nAlice;30;NYC\nBob;25;LA\n";
        let (store, _meta, graph) = parse_csv(csv_data, "semicolons.csv")?;

        // Check column count = 3
        let sparql = format!(
            "SELECT ?cc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?cc \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_COLUMN_COUNT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let cc = rows[0]["cc"].as_str().expect("cc");
        assert!(cc.contains('3'));

        Ok(())
    }

    #[test]
    fn pipe_delimited() -> ruddydoc_core::Result<()> {
        let csv_data = "Name|Age|City\nAlice|30|NYC\n";
        let (store, _meta, graph) = parse_csv(csv_data, "pipes.csv")?;

        // Check column count = 3
        let sparql = format!(
            "SELECT ?cc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?cc \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_COLUMN_COUNT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let cc = rows[0]["cc"].as_str().expect("cc");
        assert!(cc.contains('3'));

        Ok(())
    }

    #[test]
    fn quoted_fields() -> ruddydoc_core::Result<()> {
        let csv_data = "Name,Description\n\"Alice, Bob\",\"Has a comma, inside\"\n";
        let (store, _meta, graph) = parse_csv(csv_data, "quoted.csv")?;

        // Should be 2 columns, not split on the commas inside quotes
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
        assert!(cc.contains('2'));

        // Verify the quoted cell text contains the comma
        let sparql_text = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> \"1\"^^<http://www.w3.org/2001/XMLSchema#integer>. \
                 ?c <{}> \"0\"^^<http://www.w3.org/2001/XMLSchema#integer> \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW),
            ont::iri(ont::PROP_CELL_COLUMN),
        );
        let result = store.query_to_json(&sparql_text)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Alice, Bob"));

        Ok(())
    }

    #[test]
    fn empty_csv() -> ruddydoc_core::Result<()> {
        let csv_data = "";
        let (store, meta, graph) = parse_csv(csv_data, "empty.csv")?;

        assert_eq!(meta.format, InputFormat::Csv);

        // Table should exist
        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?t a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Row count = 0
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
        assert!(rc.contains('0'));

        // Column count = 0
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
        assert!(cc.contains('0'));

        // No cells
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
        assert!(rows.is_empty());

        Ok(())
    }

    #[test]
    fn single_column() -> ruddydoc_core::Result<()> {
        let csv_data = "Name\nAlice\nBob\nCharlie\n";
        let (store, _meta, graph) = parse_csv(csv_data, "single.csv")?;

        // Column count = 1
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
        assert!(cc.contains('1'));

        // 4 cells total (1 header + 3 data)
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
        assert_eq!(rows.len(), 4);

        Ok(())
    }

    #[test]
    fn inconsistent_columns_are_padded() -> ruddydoc_core::Result<()> {
        let csv_data = "A,B,C\n1,2\n3,4,5,6\n";
        let (store, _meta, graph) = parse_csv(csv_data, "inconsistent.csv")?;

        // max column count = 4 (from the row with 4 fields)
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

        // Total cells: 3 rows * 4 cols = 12
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
        assert_eq!(rows.len(), 12);

        Ok(())
    }

    #[test]
    fn document_metadata() -> ruddydoc_core::Result<()> {
        let csv_data = "X,Y\n1,2\n";
        let (store, meta, graph) = parse_csv(csv_data, "meta.csv")?;

        assert_eq!(meta.format, InputFormat::Csv);
        assert!(meta.page_count.is_none());
        assert!(meta.file_path.is_none());

        // Check sourceFormat = "csv"
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
        assert!(fmt.contains("csv"));

        // Check fileName = "meta.csv"
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
        assert!(name.contains("meta.csv"));

        // Check documentHash
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
        let csv_data = "A,B\n1,2\n";
        let (store, _meta, graph) = parse_csv(csv_data, "order.csv")?;

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
    fn cell_row_and_column_indices() -> ruddydoc_core::Result<()> {
        let csv_data = "A,B\nC,D\n";
        let (store, _meta, graph) = parse_csv(csv_data, "indices.csv")?;

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
    fn is_valid_accepts_csv_and_tsv() {
        let backend = CsvBackend::new();

        let csv_file = DocumentSource::File(std::path::PathBuf::from("data.csv"));
        assert!(backend.is_valid(&csv_file));

        let tsv_file = DocumentSource::File(std::path::PathBuf::from("data.tsv"));
        assert!(backend.is_valid(&tsv_file));

        let md_file = DocumentSource::File(std::path::PathBuf::from("data.md"));
        assert!(!backend.is_valid(&md_file));

        let csv_stream = DocumentSource::Stream {
            name: "file.csv".to_string(),
            data: vec![],
        };
        assert!(backend.is_valid(&csv_stream));

        let tsv_stream = DocumentSource::Stream {
            name: "file.tsv".to_string(),
            data: vec![],
        };
        assert!(backend.is_valid(&tsv_stream));

        let other_stream = DocumentSource::Stream {
            name: "file.txt".to_string(),
            data: vec![],
        };
        assert!(!backend.is_valid(&other_stream));
    }

    #[test]
    fn default_trait() {
        let _backend: CsvBackend = CsvBackend::default();
    }

    #[test]
    fn detect_delimiter_comma() {
        let data = b"a,b,c\n1,2,3\n";
        assert_eq!(detect_delimiter(data), b',');
    }

    #[test]
    fn detect_delimiter_tab() {
        let data = b"a\tb\tc\n1\t2\t3\n";
        assert_eq!(detect_delimiter(data), b'\t');
    }

    #[test]
    fn detect_delimiter_semicolon() {
        let data = b"a;b;c\n1;2;3\n";
        assert_eq!(detect_delimiter(data), b';');
    }

    #[test]
    fn detect_delimiter_pipe() {
        let data = b"a|b|c\n1|2|3\n";
        assert_eq!(detect_delimiter(data), b'|');
    }

    #[test]
    fn detect_delimiter_empty() {
        // Empty content should default to comma
        let data = b"";
        assert_eq!(detect_delimiter(data), b',');
    }

    #[test]
    fn has_element_links_table() -> ruddydoc_core::Result<()> {
        let csv_data = "X\n1\n";
        let (store, meta, graph) = parse_csv(csv_data, "link.csv")?;

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
}
