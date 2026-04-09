//! JSON schema validation tests (docling compatibility).

use super::helpers::*;

#[test]
fn json_schema_validation_passes_for_markdown() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);

    // Validate against docling schema
    validate_docling_json(&json).expect("schema validation failed");
}

#[test]
fn json_schema_validation_passes_for_html() {
    let backend = ruddydoc_backend_html::HtmlBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.html");

    let json = export_json(&store, &graph);
    validate_docling_json(&json).expect("schema validation failed");
}

#[test]
fn json_schema_validation_passes_for_csv() {
    let backend = ruddydoc_backend_csv::CsvBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.csv");

    let json = export_json(&store, &graph);
    validate_docling_json(&json).expect("schema validation failed");
}

#[test]
fn json_schema_all_text_types_are_valid() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let texts = json["texts"].as_array().unwrap();

    // Valid text types per docling schema
    let valid_types = [
        "paragraph",
        "section_header",
        "list_item",
        "code",
        "title",
        "caption",
        "footnote",
        "formula",
    ];

    for (i, text) in texts.iter().enumerate() {
        let text_type = text["type"]
            .as_str()
            .expect(&format!("text[{i}] missing type"));
        assert!(
            valid_types.contains(&text_type),
            "text[{i}] has invalid type: {text_type}"
        );
    }
}

#[test]
fn json_schema_section_headers_have_valid_levels() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let texts = json["texts"].as_array().unwrap();

    let headers: Vec<_> = texts
        .iter()
        .filter(|t| t["type"] == "section_header")
        .collect();

    for (i, header) in headers.iter().enumerate() {
        let level = header["heading_level"]
            .as_i64()
            .expect(&format!("header[{i}] missing heading_level"));

        assert!(
            (1..=6).contains(&level),
            "header[{i}] has invalid level: {level} (should be 1-6)"
        );
    }
}

#[test]
fn json_schema_table_cells_have_valid_booleans() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let tables = json["tables"].as_array().unwrap();

    for (i, table) in tables.iter().enumerate() {
        if let Some(cells) = table.get("cells") {
            for (j, cell) in cells.as_array().unwrap().iter().enumerate() {
                // is_header must be a boolean
                assert!(
                    cell["is_header"].is_boolean(),
                    "table[{i}] cell[{j}] is_header is not boolean"
                );
            }
        }
    }
}

#[test]
fn json_schema_source_format_is_valid() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let source_format = json["source_format"]
        .as_str()
        .expect("missing source_format");

    // Valid source formats (matching InputFormat enum)
    let valid_formats = [
        "markdown", "html", "csv", "docx", "pdf", "latex", "pptx", "xlsx", "image", "xml",
        "webvtt", "asciidoc", "json", "text", "xbrl", "epub", "rtf",
    ];

    assert!(
        valid_formats.contains(&source_format),
        "invalid source_format: {source_format}"
    );
}

#[test]
fn json_schema_name_is_nonempty_string() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let name = json["name"].as_str().expect("missing name");

    assert!(!name.is_empty(), "name should not be empty");
    assert!(name.len() > 0, "name should be non-empty string");
}

#[test]
fn json_schema_pictures_have_optional_fields() {
    // Pictures should have optional format, alt_text, link_target
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let pictures = json["pictures"].as_array().unwrap();

    if !pictures.is_empty() {
        // Just verify the structure is valid (fields are optional)
        for (i, pic) in pictures.iter().enumerate() {
            // If present, format should be string
            if let Some(fmt) = pic.get("format") {
                if !fmt.is_null() {
                    assert!(
                        fmt.is_string(),
                        "picture[{i}] format should be string or null"
                    );
                }
            }

            // If present, alt_text should be string
            if let Some(alt) = pic.get("alt_text") {
                if !alt.is_null() {
                    assert!(
                        alt.is_string(),
                        "picture[{i}] alt_text should be string or null"
                    );
                }
            }
        }
    }
}

#[test]
fn json_schema_tables_have_dimensions() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let tables = json["tables"].as_array().unwrap();

    for (i, table) in tables.iter().enumerate() {
        // Tables should have row_count and col_count
        let row_count = table["row_count"]
            .as_i64()
            .expect(&format!("table[{i}] missing row_count"));
        let col_count = table["col_count"]
            .as_i64()
            .expect(&format!("table[{i}] missing col_count"));

        assert!(row_count > 0, "table[{i}] row_count should be positive");
        assert!(col_count > 0, "table[{i}] col_count should be positive");

        // If cells are present, verify count
        if let Some(cells) = table.get("cells") {
            let cell_count = cells.as_array().unwrap().len() as i64;
            assert!(
                cell_count <= row_count * col_count,
                "table[{i}] has more cells than row_count * col_count"
            );
        }
    }
}

#[test]
fn json_schema_code_blocks_have_optional_language() {
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let (store, graph) = parse_file(&backend, "tests/fixtures/sample.md");

    let json = export_json(&store, &graph);
    let texts = json["texts"].as_array().unwrap();

    let code_blocks: Vec<_> = texts.iter().filter(|t| t["type"] == "code").collect();

    for (i, code) in code_blocks.iter().enumerate() {
        // code_language is optional
        if let Some(lang) = code.get("code_language") {
            if !lang.is_null() {
                assert!(
                    lang.is_string(),
                    "code[{i}] code_language should be string or null"
                );
            }
        }

        // text field is required
        assert!(code.get("text").is_some(), "code[{i}] missing text field");
    }
}
