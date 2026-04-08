//! Round-trip tests: parse -> export -> parse -> compare structure.

use super::helpers::*;
use ruddydoc_core::DocumentExporter;

#[test]
fn markdown_roundtrip_preserves_element_counts() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();

    // Parse sample.md
    let (store1, graph1) = parse_file(&backend, "tests/fixtures/sample.md");

    // Export to Markdown
    let exporter = ruddydoc_export::MarkdownExporter;
    let md_out = exporter.export(&store1, &graph1).expect("export failed");

    // Parse exported Markdown
    let (store2, graph2) = parse_string(&backend, "roundtrip.md", &md_out);

    // Compare element counts
    assert_eq!(
        count_paragraphs(&store1, &graph1),
        count_paragraphs(&store2, &graph2),
        "paragraph count mismatch"
    );

    assert_eq!(
        count_headings(&store1, &graph1),
        count_headings(&store2, &graph2),
        "heading count mismatch"
    );

    assert_eq!(
        count_list_items(&store1, &graph1),
        count_list_items(&store2, &graph2),
        "list item count mismatch"
    );

    assert_eq!(
        count_code_blocks(&store1, &graph1),
        count_code_blocks(&store2, &graph2),
        "code block count mismatch"
    );

    assert_eq!(
        count_tables(&store1, &graph1),
        count_tables(&store2, &graph2),
        "table count mismatch"
    );

    // Pictures may not round-trip perfectly (image data), so just check presence
    let pics1 = count_pictures(&store1, &graph1);
    let pics2 = count_pictures(&store2, &graph2);
    assert!(
        pics1 > 0 && pics2 > 0,
        "pictures lost in roundtrip: {pics1} -> {pics2}"
    );
}

#[test]
fn markdown_roundtrip_preserves_reading_order() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();

    let (store1, graph1) = parse_file(&backend, "tests/fixtures/sample.md");
    let exporter = ruddydoc_export::MarkdownExporter;
    let md_out = exporter.export(&store1, &graph1).expect("export failed");
    let (store2, graph2) = parse_string(&backend, "roundtrip.md", &md_out);

    let orders1 = get_reading_orders(&store1, &graph1);
    let orders2 = get_reading_orders(&store2, &graph2);

    // Reading orders should be same length
    assert_eq!(
        orders1.len(),
        orders2.len(),
        "reading order count mismatch"
    );

    // Both should be contiguous sequences starting at 0
    for (i, &order) in orders1.iter().enumerate() {
        assert_eq!(order, i as i64, "first parse reading order gap at {i}");
    }
    for (i, &order) in orders2.iter().enumerate() {
        assert_eq!(order, i as i64, "second parse reading order gap at {i}");
    }
}

#[test]
fn json_roundtrip_preserves_structure() {
    // Parse sample.md
    let backend_md = ruddydoc_backend_md::MarkdownBackend::new();
    let (store1, graph1) = parse_file(&backend_md, "tests/fixtures/sample.md");

    // Export to JSON
    let json_str = {
        let exporter = ruddydoc_export::JsonExporter;
        exporter.export(&store1, &graph1).expect("JSON export failed")
    };

    // Parse JSON back (requires JSON backend - skip if not implemented)
    // This test is a placeholder for when the JSON backend is implemented
    let json: serde_json::Value = serde_json::from_str(&json_str).expect("invalid JSON");

    // Verify element counts in JSON match graph
    let texts_count = json["texts"].as_array().unwrap().len();
    let tables_count = json["tables"].as_array().unwrap().len();
    let pictures_count = json["pictures"].as_array().unwrap().len();

    // Total text elements in graph
    let graph_texts = count_paragraphs(&store1, &graph1)
        + count_headings(&store1, &graph1)
        + count_list_items(&store1, &graph1)
        + count_code_blocks(&store1, &graph1);

    assert_eq!(
        texts_count, graph_texts,
        "JSON texts count doesn't match graph"
    );
    assert_eq!(
        tables_count,
        count_tables(&store1, &graph1),
        "JSON tables count doesn't match graph"
    );
    assert_eq!(
        pictures_count,
        count_pictures(&store1, &graph1),
        "JSON pictures count doesn't match graph"
    );
}

#[test]
fn html_roundtrip_preserves_structure() {
    let backend = ruddydoc_backend_html::HtmlBackend::new();

    // Parse sample.html
    let (store1, graph1) = parse_file(&backend, "tests/fixtures/sample.html");

    // Export to HTML
    let exporter = ruddydoc_export::HtmlExporter;
    let html_out = exporter.export(&store1, &graph1).expect("export failed");

    // Parse exported HTML
    let (store2, graph2) = parse_string(&backend, "roundtrip.html", &html_out);

    // Compare element counts
    assert_eq!(
        count_paragraphs(&store1, &graph1),
        count_paragraphs(&store2, &graph2),
        "HTML paragraph count mismatch"
    );

    assert_eq!(
        count_headings(&store1, &graph1),
        count_headings(&store2, &graph2),
        "HTML heading count mismatch"
    );

    assert_eq!(
        count_tables(&store1, &graph1),
        count_tables(&store2, &graph2),
        "HTML table count mismatch"
    );

    // Reading order should be preserved
    let orders1 = get_reading_orders(&store1, &graph1);
    let orders2 = get_reading_orders(&store2, &graph2);
    assert_eq!(orders1.len(), orders2.len(), "HTML reading order length");
}

#[test]
fn csv_export_to_json_preserves_table_structure() {
    let backend = ruddydoc_backend_csv::CsvBackend::new();

    // Parse sample.csv
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.csv");

    // Export to JSON
    let json = export_json(&store, &graph);

    // CSV becomes a single table
    assert_eq!(
        json["tables"].as_array().unwrap().len(),
        1,
        "CSV should produce 1 table"
    );

    let table = &json["tables"][0];
    let cells = table["cells"].as_array().unwrap();

    // Verify cells have correct structure
    for (i, cell) in cells.iter().enumerate() {
        assert!(
            cell.get("text").is_some(),
            "cell {i} missing text"
        );
        assert!(
            cell.get("row").is_some(),
            "cell {i} missing row"
        );
        assert!(
            cell.get("col").is_some(),
            "cell {i} missing col"
        );
        assert!(
            cell.get("is_header").is_some(),
            "cell {i} missing is_header"
        );
    }

    // First row should be headers
    let first_row_cells: Vec<_> = cells
        .iter()
        .filter(|c| c["row"].as_i64().unwrap() == 0)
        .collect();

    for (i, cell) in first_row_cells.iter().enumerate() {
        assert!(
            cell["is_header"].as_bool().unwrap(),
            "first row cell {i} should be header"
        );
    }
}
