# RuddyDoc

**Fast document conversion with an embedded knowledge graph**

[![CI](https://img.shields.io/badge/CI-passing-brightgreen)]()
[![Version](https://img.shields.io/badge/version-0.1.0-blue)]()
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-666_passing-brightgreen)]()

RuddyDoc is a high-performance document converter built in Rust. Parse documents, build knowledge graphs, and export to any format -- all from the command line or via API.

## What it does

Convert documents between formats with automatic structure extraction. RuddyDoc parses your documents into an embedded RDF knowledge graph (Oxigraph), making content queryable with SPARQL and exportable to 10 formats. Built for RAG workflows, AI agents, and document processing pipelines.

## Key features

- **12 input formats**: Markdown, HTML, CSV, DOCX, PDF, LaTeX, PPTX, XLSX, Image, XML (JATS, USPTO, XBRL), WebVTT, AsciiDoc
- **10 output formats**: JSON (docling-compatible), Markdown, HTML, Text, Turtle, N-Triples, JSON-LD, RDF/XML, DocTags, WebVTT
- **Embedded RDF knowledge graph**: Query documents with SPARQL, no external database required
- **Document chunking**: Structure-aware chunking for RAG and AI retrieval workflows
- **REST API + MCP server**: Integrate with AI agents (Claude Desktop, LM Studio) and HTTP clients
- **VLM pipeline support**: Visual Language Model integration for PDF understanding (via HTTP API)
- **Fast and portable**: 17MB binary, 5ms startup, 10x faster than Python docling for text formats
- **666 tests**: Comprehensive test coverage across 23 crates

## Benchmarks

Measured on real workloads using Criterion (see `cargo bench` for full results):

### Parsing performance

| Format | Fixture size | Time |
|--------|-------------|------|
| Markdown | sample fixture | 733 us |
| Markdown | 1,000 lines | 7.2 ms |
| Markdown | 10,000 lines | 145 ms |
| HTML | sample fixture | 875 us |
| HTML | 500 elements | 13 ms |
| CSV | sample fixture | 577 us |
| CSV | 1,000 rows | 42 ms |
| LaTeX | 1,000 lines | 14 ms |

### Export performance (500-line Markdown source)

| Format | Time |
|--------|------|
| WebVTT | 602 us |
| JSON-LD | 1.0 ms |
| Turtle | 1.2 ms |
| N-Triples | 1.1 ms |
| RDF/XML | 1.9 ms |
| Text | 6.6 ms |
| DocTags | 10 ms |
| JSON | 11 ms |
| HTML | 12 ms |
| Markdown | 12 ms |

### Graph operations

| Operation | Scale | Time |
|-----------|-------|------|
| Insert triples | 1,000 | 1.7 ms |
| SPARQL SELECT | 1,000 elements | 1.7 ms |
| Serialize to Turtle | 1,000 elements | 4.2 ms |
| Clear graph | 1,000 elements | 297 us |

### vs Python docling

| Metric | RuddyDoc | Python docling |
|--------|----------|----------------|
| Startup time | 5 ms | ~2 s |
| Binary size | 17 MB | 2+ GB (with ML deps) |
| Parse 1000-line Markdown | 7 ms | ~70 ms |
| Memory (batch 100 files) | ~50 MB | ~500 MB |

## Installation

### From cargo

```bash
cargo install ruddydoc
```

### Download binary

Download the latest release for your platform from [GitHub Releases](https://github.com/chapeaux/ruddydoc/releases).

### Docker

```bash
docker pull ghcr.io/chapeaux/ruddydoc:latest
docker run --rm -v $(pwd):/data ruddydoc convert /data/document.pdf
```

## Quick start

### Convert a document

```bash
# Convert PDF to Markdown
ruddydoc convert paper.pdf --format markdown

# Convert to JSON (docling-compatible)
ruddydoc convert paper.pdf --format json > output.json

# Batch convert
ruddydoc convert ./docs/*.pdf --format markdown --output ./converted/
```

### Query with SPARQL

```bash
# List all section headings in order
ruddydoc query 'SELECT ?text ?level WHERE {
  ?h a <https://ruddydoc.chapeaux.io/ontology#SectionHeader> ;
     <https://ruddydoc.chapeaux.io/ontology#textContent> ?text ;
     <https://ruddydoc.chapeaux.io/ontology#headingLevel> ?level ;
     <https://ruddydoc.chapeaux.io/ontology#readingOrder> ?order .
} ORDER BY ?order' paper.pdf

# Count elements by type
ruddydoc query 'SELECT ?type (COUNT(?e) AS ?count) WHERE {
  ?e a ?type
} GROUP BY ?type' paper.pdf
```

### Chunk for RAG

```bash
# Create 512-token chunks with heading context
ruddydoc chunk paper.pdf --max-tokens 512 > chunks.json

# Customize chunking
ruddydoc chunk paper.pdf --max-tokens 256 --include-headings false
```

### Start the server

```bash
# REST API
ruddydoc serve --port 8080

# Convert via API
curl -X POST http://localhost:8080/convert -H 'Content-Type: application/json' \
  -d '{"source": "/path/to/document.pdf"}'
```

## CLI reference

| Command | Description |
|---------|-------------|
| `convert` | Convert documents to specified output format(s) |
| `query` | Run a SPARQL query on parsed documents |
| `chunk` | Split documents into chunks for RAG workflows |
| `serve` | Start REST API + MCP server for AI agent integration |
| `info` | Show document metadata without full conversion |
| `formats` | List all supported input and output formats |
| `models` | Manage ML models (list, download) |

Run `ruddydoc <command> --help` for detailed options.

## Supported formats

### Input formats

| Format | Extensions | Description |
|--------|-----------|-------------|
| Markdown | .md, .markdown | CommonMark with GFM extensions |
| HTML | .html, .htm, .xhtml | HTML5 with semantic element support |
| CSV | .csv, .tsv | Comma/tab/semicolon/pipe-separated values (auto-detected) |
| DOCX | .docx | Microsoft Word (OOXML) with styles, lists, tables, images |
| PDF | .pdf | Text extraction with font-based heading detection |
| LaTeX | .tex, .latex | Custom recursive-descent parser |
| PPTX | .pptx | Microsoft PowerPoint with slide ordering |
| XLSX | .xlsx, .xls | Microsoft Excel with multi-sheet support |
| Image | .png, .jpg, .tiff, .bmp, .webp | Dimensions and format (OCR with ML models) |
| XML | .xml | JATS scientific articles, USPTO patents, generic XML |
| WebVTT | .vtt | Subtitle cues with timestamps |
| AsciiDoc | .adoc, .asciidoc, .asc | Headings, lists, tables, code blocks, admonitions |

### Output formats

| Format | Description | Use case |
|--------|-------------|----------|
| JSON | docling-compatible schema | Drop-in replacement for Python docling |
| Markdown | GitHub Flavored Markdown | Human-readable documents |
| HTML | Semantic HTML5 with thead/tbody | Web publishing, accessibility |
| Text | Plain text in reading order | Simple text extraction |
| Turtle | RDF Turtle serialization | Semantic web, knowledge graphs |
| N-Triples | RDF N-Triples serialization | RDF streaming, large datasets |
| JSON-LD | Schema.org-compatible linked data | Google Structured Data, SEO |
| RDF/XML | W3C RDF/XML serialization | Legacy RDF tools |
| DocTags | SmolDocling/GraniteDocling format | VLM training and evaluation |
| WebVTT | Subtitle format | Video subtitles, transcripts |

## Architecture

RuddyDoc is a 23-crate Rust workspace:

```
Input File --> Backend (format-specific parser)
                |
                v
          Oxigraph Store (RDF knowledge graph, 24 classes, 50+ properties)
                |
                v
          Pipeline (optional ML enrichment: layout, OCR, table, VLM)
                |
                v
          Export (10 output formats) / SPARQL queries / Chunking for RAG
```

Key architectural decisions:
- **Graph-first**: Documents are RDF graphs, not flat data models. Export formats are projections.
- **Crate-per-concern**: 12 backend crates, independently compilable and testable.
- **Feature-gated ML**: ONNX Runtime and VLM support are optional. Base binary has zero ML dependencies.
- **Embedded store**: Oxigraph is in-process (like SQLite). No external services needed.

For detailed architecture, see [INITIAL_PLAN.md](INITIAL_PLAN.md).

## Comparison with Python docling

RuddyDoc is a Rust rewrite of [docling](https://github.com/docling-project/docling) with semantic enhancements.

| | RuddyDoc | Python docling |
|--|----------|----------------|
| Language | Rust | Python |
| Startup | 5 ms | ~2 s |
| Binary | 17 MB | 2+ GB with ML |
| Input formats | 12 | 12 |
| Output formats | 10 (+ 4 RDF) | 6 |
| Knowledge graph | Oxigraph (SPARQL) | None |
| Chunking | Built-in CLI | Via docling-core |
| Server | Built-in REST + MCP | Separate docling-mcp |
| VLM support | HTTP API to any endpoint | transformers/vLLM |
| Tests | 666 | ~200 |

For migration details, see [docs/migration-from-docling.md](docs/migration-from-docling.md).

## Building from source

```bash
git clone https://github.com/chapeaux/ruddydoc.git
cd ruddydoc
cargo build --release
# Binary at target/release/ruddydoc
```

Run tests: `cargo test`
Run benchmarks: `cargo bench -p ruddydoc-bench`
Run clippy: `cargo clippy --workspace -- -D warnings`

## Project structure

```
crates/
  ruddydoc-core/          Shared types, traits, format detection
  ruddydoc-graph/         Oxigraph store wrapper, SPARQL
  ruddydoc-ontology/      Document ontology, SHACL shapes
  ruddydoc-converter/     Format detection, backend dispatch
  ruddydoc-pipeline/      ML pipeline stages, DocTags parser
  ruddydoc-models/        ONNX Runtime, VLM API client
  ruddydoc-export/        All 10 exporters + chunking
  ruddydoc-server/        REST API (axum) + MCP server
  ruddydoc-cli/           CLI binary (clap)
  ruddydoc-backend-*/     12 format-specific parsers
  ruddydoc-bench/         Criterion benchmarks
  ruddydoc-tests/         Compatibility test suite
ontology/
  ruddydoc.ttl            Document ontology (24 classes, 50+ properties)
  shapes.ttl              SHACL validation shapes
```

## Contributing

Contributions are welcome. Key areas:

- New input format backends
- Export format improvements
- ML model integrations (ONNX models for layout, OCR, table structure)
- Performance optimizations
- Documentation

## License

MIT. See [LICENSE](LICENSE).
