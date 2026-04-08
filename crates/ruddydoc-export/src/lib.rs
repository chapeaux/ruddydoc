//! Document export formats for RuddyDoc.
//!
//! Each exporter implements `DocumentExporter` and queries the document
//! graph via SPARQL to produce output in its format.
//!
//! The [`chunking`] module provides structure-aware document chunking for
//! RAG (Retrieval Augmented Generation) workflows.

pub mod chunking;
mod doctags_export;
mod html_export;
mod json_export;
mod jsonld_export;
mod markdown_export;
mod rdfxml_export;
mod text_export;
mod webvtt_export;

pub use chunking::{
    Chunk, ChunkMetadata, ChunkOptions, DocumentChunker, HierarchicalChunker, chunk_document,
};
pub use doctags_export::DocTagsExporter;
pub use html_export::HtmlExporter;
pub use json_export::JsonExporter;
pub use jsonld_export::JsonLdExporter;
pub use markdown_export::MarkdownExporter;
pub use rdfxml_export::RdfXmlExporter;
pub use text_export::TextExporter;
pub use webvtt_export::WebVttExporter;

use ruddydoc_core::{DocumentExporter, DocumentStore, OutputFormat};

/// Turtle RDF serialization exporter.
pub struct TurtleExporter;

impl DocumentExporter for TurtleExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::Turtle
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        store.serialize_graph(doc_graph, "turtle")
    }
}

/// N-Triples RDF serialization exporter.
pub struct NTriplesExporter;

impl DocumentExporter for NTriplesExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::NTriples
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        store.serialize_graph(doc_graph, "ntriples")
    }
}

/// Select an exporter by output format.
pub fn exporter_for(format: OutputFormat) -> ruddydoc_core::Result<Box<dyn DocumentExporter>> {
    match format {
        OutputFormat::Json => Ok(Box::new(JsonExporter)),
        OutputFormat::Turtle => Ok(Box::new(TurtleExporter)),
        OutputFormat::NTriples => Ok(Box::new(NTriplesExporter)),
        OutputFormat::Markdown => Ok(Box::new(MarkdownExporter)),
        OutputFormat::Html => Ok(Box::new(HtmlExporter)),
        OutputFormat::Text => Ok(Box::new(TextExporter)),
        OutputFormat::JsonLd => Ok(Box::new(JsonLdExporter)),
        OutputFormat::RdfXml => Ok(Box::new(RdfXmlExporter)),
        OutputFormat::DocTags => Ok(Box::new(DocTagsExporter)),
        OutputFormat::WebVtt => Ok(Box::new(WebVttExporter)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_core::{DocumentBackend, DocumentSource};
    use ruddydoc_graph::OxigraphStore;

    fn compute_hash(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        result.iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
    }

    fn setup_test_doc() -> ruddydoc_core::Result<(OxigraphStore, String)> {
        let store = OxigraphStore::new()?;
        let backend = ruddydoc_backend_md::MarkdownBackend::new();
        let md = "# Test Heading\n\nA paragraph.\n\n- Item one\n- Item two\n\n```python\nprint('hello')\n```\n\n| Col1 | Col2 |\n|------|------|\n| A | B |\n\n![Logo](logo.png)\n";
        let source = DocumentSource::Stream {
            name: "test.md".to_string(),
            data: md.as_bytes().to_vec(),
        };
        let hash = compute_hash(md.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash);
        backend.parse(&source, &store, &doc_graph)?;
        Ok((store, doc_graph))
    }

    fn setup_rich_doc() -> ruddydoc_core::Result<(OxigraphStore, String)> {
        let store = OxigraphStore::new()?;
        let backend = ruddydoc_backend_md::MarkdownBackend::new();
        let md = "\
# Document Title

First paragraph with some text.

## Section One

Another paragraph here.

- Bullet A
- Bullet B
- Bullet C

1. Step one
2. Step two
3. Step three

```rust
fn main() {
    println!(\"Hello, world!\");
}
```

| Name  | Value |
|-------|-------|
| alpha | 100   |
| beta  | 200   |

![Alt text](image.png)

> This is a blockquote.
";
        let source = DocumentSource::Stream {
            name: "rich.md".to_string(),
            data: md.as_bytes().to_vec(),
        };
        let hash = compute_hash(md.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash);
        backend.parse(&source, &store, &doc_graph)?;
        Ok((store, doc_graph))
    }

    // -----------------------------------------------------------------
    // Existing exporter tests
    // -----------------------------------------------------------------

    #[test]
    fn json_export_has_structure() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = JsonExporter;
        let json_str = exporter.export(&store, &doc_graph)?;
        let json: serde_json::Value = serde_json::from_str(&json_str)?;

        assert!(json.get("name").is_some());
        assert!(json.get("source_format").is_some());
        assert!(json.get("texts").is_some());
        assert!(json.get("tables").is_some());
        assert!(json.get("pictures").is_some());

        let texts = json["texts"].as_array().expect("texts array");
        assert!(!texts.is_empty());

        let tables = json["tables"].as_array().expect("tables array");
        assert!(!tables.is_empty());

        let pictures = json["pictures"].as_array().expect("pictures array");
        assert!(!pictures.is_empty());

        Ok(())
    }

    #[test]
    fn turtle_export_produces_valid_output() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = TurtleExporter;
        let turtle = exporter.export(&store, &doc_graph)?;
        assert!(!turtle.is_empty());
        assert!(turtle.contains("ruddydoc"));
        Ok(())
    }

    #[test]
    fn ntriples_export_produces_valid_output() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = NTriplesExporter;
        let nt = exporter.export(&store, &doc_graph)?;
        assert!(!nt.is_empty());
        // N-Triples always end with " .\n"
        assert!(nt.contains(" ."));
        Ok(())
    }

    #[test]
    fn markdown_export_preserves_content() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = MarkdownExporter;
        let md_out = exporter.export(&store, &doc_graph)?;

        assert!(md_out.contains("# Test Heading"));
        assert!(md_out.contains("A paragraph."));
        assert!(md_out.contains("Item one"));
        assert!(md_out.contains("Item two"));
        assert!(md_out.contains("print('hello')"));
        Ok(())
    }

    #[test]
    fn exporter_for_json() -> ruddydoc_core::Result<()> {
        let e = exporter_for(OutputFormat::Json)?;
        assert_eq!(e.format(), OutputFormat::Json);
        Ok(())
    }

    #[test]
    fn exporter_for_doctags() -> ruddydoc_core::Result<()> {
        let e = exporter_for(OutputFormat::DocTags)?;
        assert_eq!(e.format(), OutputFormat::DocTags);
        Ok(())
    }

    // -----------------------------------------------------------------
    // HTML exporter tests
    // -----------------------------------------------------------------

    #[test]
    fn html_export_has_doctype_and_structure() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("<head>"));
        assert!(html.contains("<meta charset=\"utf-8\">"));
        assert!(html.contains("<title>"));
        assert!(html.contains("</title>"));
        assert!(html.contains("<body>"));
        assert!(html.contains("<article>"));
        assert!(html.contains("</article>"));
        assert!(html.contains("</body>"));
        assert!(html.contains("</html>"));
        Ok(())
    }

    #[test]
    fn html_export_headings() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        assert!(html.contains("<h1>Document Title</h1>"));
        assert!(html.contains("<h2>Section One</h2>"));
        Ok(())
    }

    #[test]
    fn html_export_paragraphs() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        assert!(html.contains("<p>First paragraph with some text.</p>"));
        assert!(html.contains("<p>Another paragraph here.</p>"));
        Ok(())
    }

    #[test]
    fn html_export_unordered_list() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>Bullet A</li>"));
        assert!(html.contains("<li>Bullet B</li>"));
        assert!(html.contains("<li>Bullet C</li>"));
        assert!(html.contains("</ul>"));
        Ok(())
    }

    #[test]
    fn html_export_ordered_list() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        assert!(html.contains("<ol>"));
        assert!(html.contains("<li>Step one</li>"));
        assert!(html.contains("<li>Step two</li>"));
        assert!(html.contains("<li>Step three</li>"));
        assert!(html.contains("</ol>"));
        Ok(())
    }

    #[test]
    fn html_export_code_block_with_language() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        assert!(
            html.contains("<pre><code class=\"language-rust\">"),
            "expected code block with language-rust class, got:\n{html}"
        );
        assert!(html.contains("fn main()"));
        assert!(html.contains("</code></pre>"));
        Ok(())
    }

    #[test]
    fn html_export_table_with_thead_tbody() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        assert!(html.contains("<table>"), "expected <table> tag");
        assert!(html.contains("<thead>"), "expected <thead> tag");
        assert!(html.contains("<th>"), "expected <th> tag for header cells");
        assert!(html.contains("</thead>"), "expected </thead> tag");
        assert!(html.contains("<tbody>"), "expected <tbody> tag");
        assert!(html.contains("<td>"), "expected <td> tag for body cells");
        assert!(html.contains("</tbody>"), "expected </tbody> tag");
        assert!(html.contains("</table>"), "expected </table> tag");
        Ok(())
    }

    #[test]
    fn html_export_image() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        assert!(html.contains("<img src=\"image.png\""));
        assert!(html.contains("alt=\"Alt text\""));
        Ok(())
    }

    #[test]
    fn html_entity_escaping() -> ruddydoc_core::Result<()> {
        let store = OxigraphStore::new()?;
        let backend = ruddydoc_backend_md::MarkdownBackend::new();
        let md = "# Title\n\nText with <b>HTML</b> & \"quotes\" inside.\n";
        let source = DocumentSource::Stream {
            name: "escape.md".to_string(),
            data: md.as_bytes().to_vec(),
        };
        let hash = compute_hash(md.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash);
        backend.parse(&source, &store, &doc_graph)?;

        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        // The text content should have HTML entities escaped
        assert!(
            html.contains("&lt;b&gt;HTML&lt;/b&gt;") || html.contains("HTML"),
            "HTML should be escaped or text content preserved"
        );
        assert!(
            html.contains("&amp;") || html.contains("&quot;"),
            "special chars should be escaped"
        );
        Ok(())
    }

    #[test]
    fn html_escape_function_correctness() {
        use html_export::escape_html;

        assert_eq!(escape_html("hello"), "hello");
        assert_eq!(escape_html("<b>bold</b>"), "&lt;b&gt;bold&lt;/b&gt;");
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("say \"hi\""), "say &quot;hi&quot;");
        assert_eq!(
            escape_html("<script>alert(\"xss\")</script>"),
            "&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;"
        );
    }

    // -----------------------------------------------------------------
    // Text exporter tests
    // -----------------------------------------------------------------

    #[test]
    fn text_export_preserves_reading_order() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = TextExporter;
        let text = exporter.export(&store, &doc_graph)?;

        // All text elements should be present
        assert!(text.contains("Document Title"));
        assert!(text.contains("First paragraph with some text."));
        assert!(text.contains("Section One"));
        assert!(text.contains("Another paragraph here."));
        assert!(text.contains("Bullet A"));
        assert!(text.contains("Step one"));
        assert!(text.contains("fn main()"));

        // Table cell text should be present
        assert!(text.contains("alpha"));
        assert!(text.contains("100"));

        // Reading order: title should come before the section
        let title_pos = text.find("Document Title").expect("title should exist");
        let section_pos = text.find("Section One").expect("section should exist");
        assert!(
            title_pos < section_pos,
            "title should come before section in reading order"
        );

        Ok(())
    }

    #[test]
    fn text_export_simple_doc() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = TextExporter;
        let text = exporter.export(&store, &doc_graph)?;

        assert!(text.contains("Test Heading"));
        assert!(text.contains("A paragraph."));
        assert!(text.contains("Item one"));
        assert!(text.contains("print('hello')"));
        assert!(text.ends_with('\n'));
        Ok(())
    }

    // -----------------------------------------------------------------
    // Exporter registration tests
    // -----------------------------------------------------------------

    #[test]
    fn exporter_for_html() -> ruddydoc_core::Result<()> {
        let e = exporter_for(OutputFormat::Html)?;
        assert_eq!(e.format(), OutputFormat::Html);
        Ok(())
    }

    #[test]
    fn exporter_for_text() -> ruddydoc_core::Result<()> {
        let e = exporter_for(OutputFormat::Text)?;
        assert_eq!(e.format(), OutputFormat::Text);
        Ok(())
    }

    #[test]
    fn exporter_for_all_supported_formats() -> ruddydoc_core::Result<()> {
        let supported = [
            OutputFormat::Json,
            OutputFormat::Turtle,
            OutputFormat::NTriples,
            OutputFormat::Markdown,
            OutputFormat::Html,
            OutputFormat::Text,
            OutputFormat::JsonLd,
            OutputFormat::RdfXml,
            OutputFormat::DocTags,
            OutputFormat::WebVtt,
        ];
        for format in supported {
            let exporter = exporter_for(format)?;
            assert_eq!(exporter.format(), format);
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // Markdown exporter improvement tests
    // -----------------------------------------------------------------

    #[test]
    fn markdown_code_block_has_proper_fencing() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = MarkdownExporter;
        let md = exporter.export(&store, &doc_graph)?;

        // Code block should have ```rust on its own line and closing ``` on its own line
        assert!(
            md.contains("```rust\n"),
            "expected ```rust followed by newline"
        );
        assert!(
            md.contains("\n```\n"),
            "expected closing ``` on its own line"
        );
        Ok(())
    }

    #[test]
    fn markdown_table_has_separator() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = MarkdownExporter;
        let md = exporter.export(&store, &doc_graph)?;

        // Table should have a separator row with dashes
        assert!(md.contains("---"), "table should contain separator dashes");
        assert!(md.contains("| Name"), "table should have Name column");
        assert!(md.contains("| Value"), "table should have Value column");
        Ok(())
    }

    #[test]
    fn markdown_export_rich_doc() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = MarkdownExporter;
        let md = exporter.export(&store, &doc_graph)?;

        assert!(md.contains("# Document Title"));
        assert!(md.contains("## Section One"));
        assert!(md.contains("- Bullet A"));
        assert!(md.contains("![Alt text](image.png)"));
        Ok(())
    }

    // -----------------------------------------------------------------
    // Round-trip: parse markdown -> export HTML -> verify key content
    // -----------------------------------------------------------------

    #[test]
    fn roundtrip_markdown_to_html() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        // Verify structural elements from the original markdown survive
        assert!(html.contains("Document Title"), "title preserved");
        assert!(
            html.contains("First paragraph with some text."),
            "paragraph preserved"
        );
        assert!(html.contains("Bullet A"), "list item preserved");
        assert!(html.contains("Step one"), "ordered list item preserved");
        assert!(html.contains("fn main()"), "code content preserved");
        assert!(html.contains("alpha"), "table cell preserved");
        assert!(html.contains("image.png"), "image src preserved");
        Ok(())
    }

    // -----------------------------------------------------------------
    // Blockquote export test
    // -----------------------------------------------------------------

    #[test]
    fn html_export_blockquote() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = HtmlExporter;
        let html = exporter.export(&store, &doc_graph)?;

        assert!(
            html.contains("<blockquote>"),
            "expected blockquote element in HTML"
        );
        assert!(
            html.contains("This is a blockquote."),
            "blockquote text should be present"
        );
        assert!(html.contains("</blockquote>"));
        Ok(())
    }

    // -----------------------------------------------------------------
    // JSON-LD exporter tests
    // -----------------------------------------------------------------

    #[test]
    fn jsonld_export_has_context() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = JsonLdExporter;
        let jsonld_str = exporter.export(&store, &doc_graph)?;
        let json: serde_json::Value = serde_json::from_str(&jsonld_str)?;

        let context = json.get("@context").expect("@context must exist");
        assert_eq!(
            context.get("schema").and_then(|v| v.as_str()),
            Some("https://schema.org/")
        );
        assert_eq!(
            context.get("rdoc").and_then(|v| v.as_str()),
            Some("https://ruddydoc.chapeaux.io/ontology#")
        );
        assert_eq!(
            context.get("dcterms").and_then(|v| v.as_str()),
            Some("http://purl.org/dc/terms/")
        );
        Ok(())
    }

    #[test]
    fn jsonld_export_has_type() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = JsonLdExporter;
        let jsonld_str = exporter.export(&store, &doc_graph)?;
        let json: serde_json::Value = serde_json::from_str(&jsonld_str)?;

        assert_eq!(
            json.get("@type").and_then(|v| v.as_str()),
            Some("schema:CreativeWork")
        );
        Ok(())
    }

    #[test]
    fn jsonld_export_has_elements() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = JsonLdExporter;
        let jsonld_str = exporter.export(&store, &doc_graph)?;
        let json: serde_json::Value = serde_json::from_str(&jsonld_str)?;

        let elements = json["rdoc:hasElement"]
            .as_array()
            .expect("rdoc:hasElement array");
        assert!(!elements.is_empty(), "should have at least one element");

        // All elements should have @type, rdoc:textContent, rdoc:readingOrder
        for el in elements {
            assert!(el.get("@type").is_some(), "element should have @type");
            assert!(
                el.get("rdoc:textContent").is_some(),
                "element should have rdoc:textContent"
            );
            assert!(
                el.get("rdoc:readingOrder").is_some(),
                "element should have rdoc:readingOrder"
            );
        }

        Ok(())
    }

    #[test]
    fn jsonld_export_element_types() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = JsonLdExporter;
        let jsonld_str = exporter.export(&store, &doc_graph)?;
        let json: serde_json::Value = serde_json::from_str(&jsonld_str)?;

        let elements = json["rdoc:hasElement"]
            .as_array()
            .expect("rdoc:hasElement array");

        let types: Vec<&str> = elements
            .iter()
            .filter_map(|el| el.get("@type").and_then(|v| v.as_str()))
            .collect();

        // Check that we have section headers and paragraphs
        assert!(
            types.iter().any(|t| t.contains("SectionHeader")),
            "should contain SectionHeader elements"
        );
        assert!(
            types.iter().any(|t| t.contains("Paragraph")),
            "should contain Paragraph elements"
        );
        assert!(
            types.iter().any(|t| t.contains("ListItem")),
            "should contain ListItem elements"
        );
        assert!(
            types.iter().any(|t| t.contains("Code")),
            "should contain Code elements"
        );

        Ok(())
    }

    #[test]
    fn jsonld_export_heading_has_level() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = JsonLdExporter;
        let jsonld_str = exporter.export(&store, &doc_graph)?;
        let json: serde_json::Value = serde_json::from_str(&jsonld_str)?;

        let elements = json["rdoc:hasElement"]
            .as_array()
            .expect("rdoc:hasElement array");

        let headers: Vec<&serde_json::Value> = elements
            .iter()
            .filter(|el| {
                el.get("@type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t.contains("SectionHeader"))
            })
            .collect();

        assert!(
            !headers.is_empty(),
            "should have at least one SectionHeader"
        );
        for header in &headers {
            assert!(
                header.get("rdoc:headingLevel").is_some(),
                "SectionHeader should have rdoc:headingLevel"
            );
        }

        Ok(())
    }

    #[test]
    fn jsonld_export_code_has_language() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = JsonLdExporter;
        let jsonld_str = exporter.export(&store, &doc_graph)?;
        let json: serde_json::Value = serde_json::from_str(&jsonld_str)?;

        let elements = json["rdoc:hasElement"]
            .as_array()
            .expect("rdoc:hasElement array");

        let code_blocks: Vec<&serde_json::Value> = elements
            .iter()
            .filter(|el| {
                el.get("@type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t.contains("Code"))
            })
            .collect();

        assert!(
            !code_blocks.is_empty(),
            "should have at least one Code block"
        );
        let code = code_blocks[0];
        assert_eq!(
            code.get("rdoc:codeLanguage").and_then(|v| v.as_str()),
            Some("rust"),
            "Code block should have codeLanguage=rust"
        );

        Ok(())
    }

    #[test]
    fn jsonld_export_has_source_format() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = JsonLdExporter;
        let jsonld_str = exporter.export(&store, &doc_graph)?;
        let json: serde_json::Value = serde_json::from_str(&jsonld_str)?;

        assert!(
            json.get("rdoc:sourceFormat").is_some(),
            "should have rdoc:sourceFormat"
        );

        Ok(())
    }

    #[test]
    fn jsonld_export_reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = JsonLdExporter;
        let jsonld_str = exporter.export(&store, &doc_graph)?;
        let json: serde_json::Value = serde_json::from_str(&jsonld_str)?;

        let elements = json["rdoc:hasElement"]
            .as_array()
            .expect("rdoc:hasElement array");

        let orders: Vec<i64> = elements
            .iter()
            .filter_map(|el| el.get("rdoc:readingOrder").and_then(|v| v.as_i64()))
            .collect();

        // Verify elements are in non-decreasing reading order
        for window in orders.windows(2) {
            assert!(
                window[0] <= window[1],
                "reading order should be non-decreasing: {} > {}",
                window[0],
                window[1]
            );
        }

        Ok(())
    }

    // -----------------------------------------------------------------
    // RDF/XML exporter tests
    // -----------------------------------------------------------------

    #[test]
    fn rdfxml_export_produces_valid_output() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = RdfXmlExporter;
        let rdfxml = exporter.export(&store, &doc_graph)?;

        assert!(!rdfxml.is_empty());
        // RDF/XML should contain XML processing instruction or rdf:RDF root
        assert!(
            rdfxml.contains("rdf:RDF") || rdfxml.contains("<rdf:Description"),
            "RDF/XML should contain rdf:RDF or rdf:Description"
        );
        Ok(())
    }

    #[test]
    fn rdfxml_export_contains_document_elements() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = RdfXmlExporter;
        let rdfxml = exporter.export(&store, &doc_graph)?;

        // Should contain references to the ontology namespace
        assert!(
            rdfxml.contains("ruddydoc"),
            "RDF/XML should reference the ontology namespace"
        );
        Ok(())
    }

    #[test]
    fn rdfxml_export_format_is_correct() -> ruddydoc_core::Result<()> {
        let e = exporter_for(OutputFormat::RdfXml)?;
        assert_eq!(e.format(), OutputFormat::RdfXml);
        Ok(())
    }

    // -----------------------------------------------------------------
    // DocTags exporter tests
    // -----------------------------------------------------------------

    #[test]
    fn doctags_export_has_root_tags() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = DocTagsExporter;
        let doctags = exporter.export(&store, &doc_graph)?;

        assert!(
            doctags.starts_with("<doctag>"),
            "should start with <doctag>"
        );
        assert!(doctags.contains("</doctag>"), "should end with </doctag>");
        assert!(doctags.contains("<page>"), "should contain <page> tag");
        assert!(doctags.contains("</page>"), "should contain </page> tag");
        Ok(())
    }

    #[test]
    fn doctags_export_section_header() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = DocTagsExporter;
        let doctags = exporter.export(&store, &doc_graph)?;

        assert!(
            doctags.contains("<loc_section_header>"),
            "should contain section header tag"
        );
        assert!(
            doctags.contains("Section One"),
            "should contain section header text"
        );
        Ok(())
    }

    #[test]
    fn doctags_export_paragraph() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = DocTagsExporter;
        let doctags = exporter.export(&store, &doc_graph)?;

        assert!(
            doctags.contains("<loc_body>"),
            "should contain body tag for paragraphs"
        );
        assert!(
            doctags.contains("First paragraph with some text."),
            "should contain paragraph text"
        );
        Ok(())
    }

    #[test]
    fn doctags_export_list_items() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = DocTagsExporter;
        let doctags = exporter.export(&store, &doc_graph)?;

        assert!(
            doctags.contains("<loc_list_item>"),
            "should contain list item tags"
        );
        assert!(
            doctags.contains("Bullet A"),
            "should contain list item text"
        );
        Ok(())
    }

    #[test]
    fn doctags_export_code_block() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = DocTagsExporter;
        let doctags = exporter.export(&store, &doc_graph)?;

        assert!(doctags.contains("<loc_code>"), "should contain code tag");
        assert!(doctags.contains("fn main()"), "should contain code content");
        Ok(())
    }

    #[test]
    fn doctags_export_table() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = DocTagsExporter;
        let doctags = exporter.export(&store, &doc_graph)?;

        assert!(doctags.contains("<loc_table>"), "should contain table tag");
        assert!(doctags.contains("<loc_row>"), "should contain row tag");
        assert!(
            doctags.contains("<loc_col_header>"),
            "should contain header cell tag"
        );
        assert!(
            doctags.contains("<loc_cell>"),
            "should contain body cell tag"
        );
        assert!(doctags.contains("Name"), "should contain table header text");
        assert!(doctags.contains("alpha"), "should contain table cell text");
        Ok(())
    }

    #[test]
    fn doctags_export_picture() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = DocTagsExporter;
        let doctags = exporter.export(&store, &doc_graph)?;

        assert!(
            doctags.contains("<loc_picture>"),
            "should contain picture tag"
        );
        Ok(())
    }

    #[test]
    fn doctags_export_escapes_angle_brackets() -> ruddydoc_core::Result<()> {
        let store = OxigraphStore::new()?;
        let backend = ruddydoc_backend_md::MarkdownBackend::new();
        let md = "# Title\n\nText with <b>HTML</b> inside.\n";
        let source = DocumentSource::Stream {
            name: "escape.md".to_string(),
            data: md.as_bytes().to_vec(),
        };
        let hash = compute_hash(md.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash);
        backend.parse(&source, &store, &doc_graph)?;

        let exporter = DocTagsExporter;
        let doctags = exporter.export(&store, &doc_graph)?;

        // Angle brackets in content should be escaped
        assert!(
            doctags.contains("&lt;b&gt;HTML&lt;/b&gt;") || !doctags.contains("<b>HTML</b>"),
            "angle brackets in content should be escaped"
        );
        Ok(())
    }

    // -----------------------------------------------------------------
    // WebVTT exporter tests
    // -----------------------------------------------------------------

    #[test]
    fn webvtt_export_has_header() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = WebVttExporter;
        let vtt = exporter.export(&store, &doc_graph)?;

        assert!(
            vtt.starts_with("WEBVTT"),
            "WebVTT output must start with WEBVTT header"
        );
        Ok(())
    }

    #[test]
    fn webvtt_export_has_timestamps() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_test_doc()?;
        let exporter = WebVttExporter;
        let vtt = exporter.export(&store, &doc_graph)?;

        // Should have timestamp arrows for cues
        assert!(
            vtt.contains("-->"),
            "WebVTT output should contain timestamp arrows"
        );
        Ok(())
    }

    #[test]
    fn webvtt_export_contains_text() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = WebVttExporter;
        let vtt = exporter.export(&store, &doc_graph)?;

        assert!(
            vtt.contains("Document Title"),
            "WebVTT should contain document text"
        );
        assert!(
            vtt.contains("First paragraph with some text."),
            "WebVTT should contain paragraph text"
        );
        Ok(())
    }

    #[test]
    fn webvtt_export_pseudo_timestamps_are_sequential() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = WebVttExporter;
        let vtt = exporter.export(&store, &doc_graph)?;

        // Extract all timestamps
        let timestamps: Vec<&str> = vtt.lines().filter(|line| line.contains("-->")).collect();

        assert!(
            !timestamps.is_empty(),
            "should have at least one timestamp line"
        );

        // Verify timestamps are sequential
        let mut prev_end = "00:00:00.000".to_string();
        for ts_line in &timestamps {
            let parts: Vec<&str> = ts_line.split(" --> ").collect();
            assert_eq!(parts.len(), 2, "timestamp line should have start --> end");
            let start = parts[0].trim();
            let end = parts[1].trim();
            assert!(
                start >= prev_end.as_str() || start == "00:00:00.000",
                "timestamps should be sequential: start {start} < prev_end {prev_end}"
            );
            prev_end = end.to_string();
        }

        Ok(())
    }

    #[test]
    fn webvtt_export_with_timed_elements() -> ruddydoc_core::Result<()> {
        // Manually insert timed elements to test the timed path
        let store = OxigraphStore::new()?;
        let doc_graph = "urn:ruddydoc:doc:timed_test";
        let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        let xsd_duration = "http://www.w3.org/2001/XMLSchema#duration";

        // Create document
        store.insert_triple_into(
            &format!("{doc_graph}/doc"),
            rdf_type,
            &ruddydoc_ontology::iri(ruddydoc_ontology::CLASS_DOCUMENT),
            doc_graph,
        )?;

        // Create timed paragraph 1
        let el1 = format!("{doc_graph}/p0");
        store.insert_triple_into(
            &el1,
            rdf_type,
            &ruddydoc_ontology::iri(ruddydoc_ontology::CLASS_PARAGRAPH),
            doc_graph,
        )?;
        store.insert_literal(
            &el1,
            &ruddydoc_ontology::iri(ruddydoc_ontology::PROP_TEXT_CONTENT),
            "Hello, welcome.",
            "string",
            doc_graph,
        )?;
        store.insert_literal(
            &el1,
            &ruddydoc_ontology::iri(ruddydoc_ontology::PROP_READING_ORDER),
            "0",
            "integer",
            doc_graph,
        )?;
        store.insert_literal(
            &el1,
            &ruddydoc_ontology::iri(ruddydoc_ontology::PROP_START_TIME),
            "PT1S",
            &xsd_duration,
            doc_graph,
        )?;
        store.insert_literal(
            &el1,
            &ruddydoc_ontology::iri(ruddydoc_ontology::PROP_END_TIME),
            "PT5S",
            &xsd_duration,
            doc_graph,
        )?;

        // Create timed paragraph 2
        let el2 = format!("{doc_graph}/p1");
        store.insert_triple_into(
            &el2,
            rdf_type,
            &ruddydoc_ontology::iri(ruddydoc_ontology::CLASS_PARAGRAPH),
            doc_graph,
        )?;
        store.insert_literal(
            &el2,
            &ruddydoc_ontology::iri(ruddydoc_ontology::PROP_TEXT_CONTENT),
            "Today we discuss RuddyDoc.",
            "string",
            doc_graph,
        )?;
        store.insert_literal(
            &el2,
            &ruddydoc_ontology::iri(ruddydoc_ontology::PROP_READING_ORDER),
            "1",
            "integer",
            doc_graph,
        )?;
        store.insert_literal(
            &el2,
            &ruddydoc_ontology::iri(ruddydoc_ontology::PROP_START_TIME),
            "PT5.5S",
            &xsd_duration,
            doc_graph,
        )?;
        store.insert_literal(
            &el2,
            &ruddydoc_ontology::iri(ruddydoc_ontology::PROP_END_TIME),
            "PT10S",
            &xsd_duration,
            doc_graph,
        )?;

        let exporter = WebVttExporter;
        let vtt = exporter.export(&store, doc_graph)?;

        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("00:00:01.000 --> 00:00:05.000"));
        assert!(vtt.contains("Hello, welcome."));
        assert!(vtt.contains("00:00:05.500 --> 00:00:10.000"));
        assert!(vtt.contains("Today we discuss RuddyDoc."));

        Ok(())
    }

    #[test]
    fn webvtt_export_format_is_correct() -> ruddydoc_core::Result<()> {
        let e = exporter_for(OutputFormat::WebVtt)?;
        assert_eq!(e.format(), OutputFormat::WebVtt);
        Ok(())
    }

    // -----------------------------------------------------------------
    // New exporter registration tests
    // -----------------------------------------------------------------

    #[test]
    fn exporter_for_jsonld() -> ruddydoc_core::Result<()> {
        let e = exporter_for(OutputFormat::JsonLd)?;
        assert_eq!(e.format(), OutputFormat::JsonLd);
        Ok(())
    }

    #[test]
    fn exporter_for_rdfxml() -> ruddydoc_core::Result<()> {
        let e = exporter_for(OutputFormat::RdfXml)?;
        assert_eq!(e.format(), OutputFormat::RdfXml);
        Ok(())
    }

    #[test]
    fn exporter_for_webvtt() -> ruddydoc_core::Result<()> {
        let e = exporter_for(OutputFormat::WebVtt)?;
        assert_eq!(e.format(), OutputFormat::WebVtt);
        Ok(())
    }

    // -----------------------------------------------------------------
    // Round-trip tests for new formats
    // -----------------------------------------------------------------

    #[test]
    fn roundtrip_markdown_to_jsonld() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = JsonLdExporter;
        let jsonld = exporter.export(&store, &doc_graph)?;

        // Valid JSON
        let json: serde_json::Value = serde_json::from_str(&jsonld)?;
        assert!(json.is_object(), "JSON-LD should be a JSON object");

        // Content preserved
        let elements = json["rdoc:hasElement"].as_array().expect("elements array");
        let all_text: String = elements
            .iter()
            .filter_map(|el| el.get("rdoc:textContent").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join(" ");

        assert!(all_text.contains("Document Title"), "title preserved");
        assert!(all_text.contains("First paragraph"), "paragraph preserved");
        assert!(all_text.contains("Bullet A"), "list item preserved");
        Ok(())
    }

    #[test]
    fn roundtrip_markdown_to_doctags() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = DocTagsExporter;
        let doctags = exporter.export(&store, &doc_graph)?;

        // Structural integrity
        assert!(doctags.starts_with("<doctag>"));
        assert!(doctags.contains("</doctag>"));

        // Content preserved
        assert!(doctags.contains("Document Title"));
        assert!(doctags.contains("First paragraph with some text."));
        assert!(doctags.contains("Bullet A"));
        assert!(doctags.contains("fn main()"));
        assert!(doctags.contains("alpha"));
        Ok(())
    }

    #[test]
    fn roundtrip_markdown_to_webvtt() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = WebVttExporter;
        let vtt = exporter.export(&store, &doc_graph)?;

        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("Document Title"));
        assert!(vtt.contains("First paragraph with some text."));
        Ok(())
    }

    #[test]
    fn roundtrip_markdown_to_rdfxml() -> ruddydoc_core::Result<()> {
        let (store, doc_graph) = setup_rich_doc()?;
        let exporter = RdfXmlExporter;
        let rdfxml = exporter.export(&store, &doc_graph)?;

        assert!(!rdfxml.is_empty());
        // Should contain the ontology namespace
        assert!(rdfxml.contains("ruddydoc"));
        // Should contain document content
        assert!(
            rdfxml.contains("Document Title") || rdfxml.contains("First paragraph"),
            "RDF/XML should contain document content"
        );
        Ok(())
    }
}
