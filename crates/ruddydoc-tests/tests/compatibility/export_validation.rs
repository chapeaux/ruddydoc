//! Export format validation tests.

use super::helpers::*;
use ruddydoc_core::{DocumentExporter, DocumentStore};

#[test]
fn json_export_is_valid_json() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let exporter = ruddydoc_export::JsonExporter;
    let json_str = exporter.export(&store, &graph).expect("export failed");

    // Should parse as valid JSON
    let json: serde_json::Value =
        serde_json::from_str(&json_str).expect("JSON export is not valid JSON");

    // Should be an object (not array or primitive)
    assert!(json.is_object(), "JSON export should be an object");
}

#[test]
fn json_export_has_required_fields() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);

    // Validate schema
    validate_docling_json(&json).expect("JSON schema validation failed");
}

#[test]
fn markdown_export_is_valid_markdown() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let exporter = ruddydoc_export::MarkdownExporter;
    let md = exporter.export(&store, &graph).expect("export failed");

    // Should be non-empty
    assert!(!md.is_empty(), "Markdown export is empty");

    // Should contain heading markers
    assert!(md.contains('#'), "Markdown should have headings");

    // Should be parseable by the Markdown backend
    let backend2 = ruddydoc_backend_md::MarkdownBackend::new();
    let result = parse_string(&backend2, "test.md", &md);
    assert!(result.1.len() > 0, "exported Markdown should be parseable");
}

#[test]
fn html_export_is_valid_html5() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let exporter = ruddydoc_export::HtmlExporter;
    let html = exporter.export(&store, &graph).expect("export failed");

    // Basic HTML5 structure checks
    assert!(
        html.contains("<!DOCTYPE html>") || html.contains("<!doctype html>"),
        "HTML should have DOCTYPE"
    );
    assert!(html.contains("<html"), "HTML should have <html> tag");
    assert!(html.contains("</html>"), "HTML should close <html> tag");
    assert!(html.contains("<body"), "HTML should have <body> tag");
    assert!(html.contains("</body>"), "HTML should close <body> tag");

    // Should be parseable by HTML backend
    let backend_html = ruddydoc_backend_html::HtmlBackend::new();
    let result = parse_string(&backend_html, "test.html", &html);
    assert!(result.1.len() > 0, "exported HTML should be parseable");
}

#[test]
fn turtle_export_is_valid_turtle() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let exporter = ruddydoc_export::TurtleExporter;
    let turtle = exporter.export(&store, &graph).expect("export failed");

    // Basic Turtle syntax checks
    assert!(!turtle.is_empty(), "Turtle export is empty");

    // Should have statement terminators
    assert!(turtle.contains(" ."), "Turtle should have statement terminators");

    // Should contain ontology terms
    assert!(
        turtle.contains("ontology#"),
        "Turtle should reference ontology"
    );

    // Every non-empty line should end with ' .' or be a prefix/comment
    for line in turtle.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty()
            && !trimmed.starts_with('@')
            && !trimmed.starts_with('#')
            && !trimmed.ends_with(';')
        {
            assert!(
                trimmed.ends_with(" ."),
                "Turtle line should end with ' .': {trimmed}"
            );
        }
    }
}

#[test]
fn ntriples_export_is_valid_ntriples() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let exporter = ruddydoc_export::NTriplesExporter;
    let nt = exporter.export(&store, &graph).expect("export failed");

    assert!(!nt.is_empty(), "N-Triples export is empty");

    // Every non-empty line must end with " ."
    for line in nt.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            assert!(
                trimmed.ends_with(" ."),
                "N-Triples line must end with ' .': {trimmed}"
            );
        }
    }

    // Should have subject-predicate-object triples (very basic check)
    let non_empty_lines: Vec<_> = nt.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(
        !non_empty_lines.is_empty(),
        "N-Triples should have at least one triple"
    );
}

#[test]
fn text_export_is_plain_text() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let exporter = ruddydoc_export::TextExporter;
    let text = exporter.export(&store, &graph).expect("export failed");

    assert!(!text.is_empty(), "Text export is empty");

    // Should not contain HTML tags
    assert!(!text.contains('<'), "Text export should not have HTML tags");

    // Should not contain Markdown markers (basic check)
    assert!(
        !text.contains("##"),
        "Text export should not have Markdown headings"
    );

    // Should contain actual text content from the document
    assert!(
        text.contains("Introduction") || text.to_lowercase().contains("introduction"),
        "Text export should contain document content"
    );
}

#[test]
fn all_export_formats_produce_nonempty_output() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    // Test all implemented export formats
    let formats: Vec<(Box<dyn DocumentExporter>, &str)> = vec![
        (Box::new(ruddydoc_export::JsonExporter), "JSON"),
        (Box::new(ruddydoc_export::MarkdownExporter), "Markdown"),
        (Box::new(ruddydoc_export::HtmlExporter), "HTML"),
        (Box::new(ruddydoc_export::TextExporter), "Text"),
        (Box::new(ruddydoc_export::TurtleExporter), "Turtle"),
        (Box::new(ruddydoc_export::NTriplesExporter), "N-Triples"),
    ];

    for (exporter, name) in formats {
        let output = exporter.export(&store, &graph).expect(&format!("{name} export failed"));
        assert!(!output.is_empty(), "{name} export is empty");
        assert!(output.len() > 10, "{name} export is suspiciously short: {} bytes", output.len());
    }
}

#[test]
fn json_export_texts_have_reading_order() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let texts = json["texts"].as_array().unwrap();

    // All texts should have reading_order
    for (i, text) in texts.iter().enumerate() {
        assert!(
            text.get("reading_order").is_some(),
            "text[{i}] missing reading_order"
        );
    }

    // Reading orders should be sorted
    let orders: Vec<i64> = texts
        .iter()
        .map(|t| t["reading_order"].as_i64().unwrap())
        .collect();

    for i in 1..orders.len() {
        assert!(
            orders[i] >= orders[i - 1],
            "reading orders not sorted: {} -> {}",
            orders[i - 1],
            orders[i]
        );
    }
}

#[test]
fn json_export_table_cells_have_positions() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let tables = json["tables"].as_array().unwrap();

    for (i, table) in tables.iter().enumerate() {
        let cells = table["cells"].as_array().expect(&format!("table[{i}] missing cells"));

        for (j, cell) in cells.iter().enumerate() {
            assert!(
                cell.get("row").is_some(),
                "table[{i}] cell[{j}] missing row"
            );
            assert!(
                cell.get("col").is_some(),
                "table[{i}] cell[{j}] missing col"
            );

            let row = cell["row"].as_i64().unwrap();
            let col = cell["col"].as_i64().unwrap();

            assert!(row >= 0, "table[{i}] cell[{j}] has negative row");
            assert!(col >= 0, "table[{i}] cell[{j}] has negative col");
        }
    }
}

#[test]
fn html_export_has_semantic_structure() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let exporter = ruddydoc_export::HtmlExporter;
    let html = exporter.export(&store, &graph).expect("export failed");

    // Should use semantic HTML5 elements
    assert!(html.contains("<h1") || html.contains("<h2"), "HTML should have headings");
    assert!(html.contains("<p"), "HTML should have paragraphs");

    // Tables should be proper table elements
    if html.contains("<table") {
        assert!(html.contains("<tr"), "HTML tables should have rows");
        assert!(html.contains("<td") || html.contains("<th"), "HTML tables should have cells");
    }

    // Code blocks should be in <pre><code>
    if html.contains("<code") {
        assert!(
            html.contains("<pre"),
            "HTML code blocks should be in <pre> tags"
        );
    }
}
