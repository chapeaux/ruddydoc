# RuddyDoc

**Fast document conversion with an embedded, reasoning knowledge graph**

[![CI](https://img.shields.io/badge/CI-passing-brightgreen)]()
[![Version](https://img.shields.io/badge/version-0.1.0-blue)]()
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-797_passing-brightgreen)]()

RuddyDoc is a high-performance document converter built in Rust. Parse documents, build knowledge graphs, and export to any format -- all from the command line, a REST API, or an MCP server for AI agents.

## What it does

Convert documents between formats with automatic structure extraction. RuddyDoc parses your documents into an embedded RDF knowledge graph (powered by [Sparq](https://github.com/sparq-org/sparq)), making content queryable with SPARQL and exportable to 10 formats. Beyond conversion, every document is automatically reasoned over (RDFS/OWL-RL), validated (SHACL), and given a standards-compliant provenance record (PROV-O) -- and can be searched (full-text or semantic), asked questions in plain English, and content-addressed with a canonical graph hash. Built for RAG workflows, AI agents, and document processing pipelines.

## Key features

- **12 input formats**: Markdown, HTML, CSV, DOCX, PDF, LaTeX, PPTX, XLSX, Image, XML (JATS, USPTO, XBRL), WebVTT, AsciiDoc
- **10 output formats**: JSON (docling-compatible), Markdown, HTML, Text, Turtle, N-Triples, JSON-LD, RDF/XML, DocTags, WebVTT
- **Embedded RDF knowledge graph**: Query documents with SPARQL, no external database required
- **Semantic reasoning by default**: RDFS/OWL-RL materialization means `?x a rdoc:TextElement` finds paragraphs and headers, not just literal `TextElement` instances -- the ontology's class hierarchy is always live
- **SHACL validation**: every conversion is checked against the ontology's shape constraints; violations are reported, never silently dropped
- **Full-text and semantic search**: BM25 search over document literals, plus embedding-based semantic search over chunks for RAG
- **Natural-language querying**: ask a document a question in plain English; RuddyDoc grounds it against the schema, generates SPARQL, and self-repairs on invalid queries
- **RDF dataset canonicalization**: RDFC-1.0 content hashing of the derived graph itself, for detecting semantically-identical documents regardless of source format
- **W3C PROV-O provenance**: every conversion records a standards-compliant lineage record (`prov:Activity`/`prov:Entity`/`wasGeneratedBy`/`used`), alongside RuddyDoc's own detailed per-element provenance
- **Document chunking**: structure-aware chunking for RAG and AI retrieval workflows
- **REST API + MCP server**: 16 tools for AI agents (Claude Desktop, LM Studio) and HTTP clients
- **VLM pipeline support**: Visual Language Model integration for PDF understanding (via HTTP API)
- **Fast and portable**: 17MB binary, 5ms startup, 6-7x faster than Python docling for text-format parsing
- **797 tests**: comprehensive test coverage across 23 crates

## Semantic capabilities

RuddyDoc doesn't just parse documents into triples -- it reasons over them. These capabilities are built on [Sparq](https://github.com/sparq-org/sparq)'s reasoning, validation, search, and lineage crates, encapsulated entirely behind RuddyDoc's own store (no Sparq types ever leak past `ruddydoc-graph`).

### Schema introspection

Mined directly from the store's indexes -- no SPARQL required to discover what's in a document.

```bash
ruddydoc query 'SELECT ?type (COUNT(?e) AS ?count) WHERE { ?e a ?type } GROUP BY ?type' paper.pdf
```

or, via MCP: `introspect_document` (full schema: entity/triple counts, classes, predicates, characteristic sets), `list_classes`, `list_prefixes`.

### RDFS/OWL-RL reasoning (on by default)

Every conversion materializes the ontology's class hierarchy into the document's own graph. The 28-class ontology has real depth (`Paragraph rdfs:subClassOf TextElement`, `TableCell rdfs:subClassOf DocumentElement`, ...) that's now queryable without knowing every leaf class:

```sparql
SELECT ?el WHERE { ?el a <https://ruddydoc.chapeaux.io/ontology#TextElement> }
-- finds paragraphs, headers, captions, footnotes, etc. -- not just literal TextElement instances
```

### SHACL validation

Every conversion is checked against SHACL shapes covering `Document`, `TextElement`, and `TableElement` (required properties, correct datatypes). Violations don't block conversion -- they're attached to the result as a machine-readable report:

```json
{ "conforms": true, "results": [] }
```

Re-check on demand (e.g. after a graph mutation) with the `validate_document` MCP tool.

### Full-text search

BM25 search over a document's string literals -- finds matching text without hand-writing SPARQL's `text:` magic predicates:

```json
{"tool": "search_text", "arguments": {"document_id": "...", "query": "revenue growth", "limit": 10}}
```

### Vector search / RAG

Chunk a document, embed each chunk via any OpenAI-compatible embedding endpoint, and get semantic similarity search over the results -- entirely in-memory, no vector database:

```json
{"tool": "embed_document", "arguments": {"document_id": "...", "max_tokens": 512}}
{"tool": "semantic_search", "arguments": {"document_id": "...", "query": "how does the model handle outliers?", "limit": 5}}
```

Requires `RUDDYDOC_EMBEDDING_URL` to be configured (see below).

### Natural-language querying

Ask a document a question in plain English -- no SPARQL required. RuddyDoc grounds the question against the document's schema, generates a SPARQL query with the configured LLM, validates it, and self-repairs on the first invalid attempt before executing:

```json
{"tool": "ask_document", "arguments": {"document_id": "...", "question": "How many tables does this document have?"}}
```

Requires `RUDDYDOC_LLM_URL` to be configured (see below).

### RDF dataset canonicalization

An RDFC-1.0 canonical-graph hash (SHA-256 of the canonical N-Quads) -- a content hash of the *derived RDF graph*, not the source bytes, so two documents that parse to the same semantic content hash identically even from different source formats. It's a genuine perf cost (a full graph canonicalization pass), so it's opt-in:

```bash
ruddydoc convert paper.pdf --canonical-hash
# converted paper.pdf (Pdf, 182004 bytes, 1204 triples)
# canonical hash: 8ab2da25fa49285a9fa6538901da7831976e5e457e76826e8e05b619b82b8ee2
```

Also available on demand via the `canonicalize_document` MCP tool.

### W3C PROV-O provenance

Every conversion automatically records a standards-compliant PROV-O lineage: one `prov:Activity` for the conversion, one `prov:Entity` for the resulting document graph (`wasGeneratedBy` the activity, `wasDerivedFrom`/`used` the source input). This is supplementary to -- not a replacement for -- RuddyDoc's own detailed per-element provenance (`rdoc:Provenance`, confidence scores, detecting model names):

```sparql
ASK { GRAPH ?doc { ?doc <http://www.w3.org/ns/prov#wasGeneratedBy> ?activity } }
-- true
```

### Configuring the embedding/LLM providers

Vector search and natural-language querying need an external HTTP endpoint (OpenAI-compatible). Both are opt-in via environment variables -- unset means the feature is unavailable, not silently pointed at localhost:

| Variable | Purpose | Default |
|----------|---------|---------|
| `RUDDYDOC_EMBEDDING_URL` | Embedding endpoint (unset = `embed_document`/`semantic_search` disabled) | none |
| `RUDDYDOC_EMBEDDING_API_KEY` | Bearer token for the embedding endpoint | none |
| `RUDDYDOC_EMBEDDING_MODEL` | Model name to request | `text-embedding-3-small` |
| `RUDDYDOC_LLM_URL` | Chat-completion endpoint (unset = `ask_document` disabled) | none |
| `RUDDYDOC_LLM_API_KEY` | Bearer token for the LLM endpoint | none |
| `RUDDYDOC_LLM_MODEL` | Model name to request | `gpt-4o-mini` |

## MCP tools reference

`ruddydoc serve --mcp` runs a JSON-RPC 2.0 stdio MCP server. 16 tools, grouped by purpose:

**Core**

| Tool | Description |
|------|-------------|
| `convert_document` | Convert a file to RuddyDoc's knowledge graph, returns a document ID |
| `query_document` | Run a SPARQL query against a converted document |
| `export_document` | Export a document to any of the 10 output formats |
| `list_elements` | List a document's structural elements, optionally filtered by type |
| `chunk_document` | Split a document into RAG-ready chunks |
| `list_documents` | List all documents converted in this server session |
| `list_formats` | List all supported input/output formats |

**Semantic**

| Tool | Description |
|------|-------------|
| `introspect_document` | Full schema introspection (counts, classes, predicates, characteristic sets) |
| `list_classes` | Classes observed in a document, by instance count |
| `list_prefixes` | Namespaces/prefixes in use, by term count |
| `search_text` | BM25 full-text search over document literals |
| `embed_document` | Chunk + embed a document for semantic search |
| `semantic_search` | Embedding-similarity search over a document's chunks |
| `ask_document` | Natural-language question answering (no SPARQL) |
| `validate_document` | Re-check SHACL conformance on demand |
| `canonicalize_document` | RDFC-1.0 canonical-graph hash on demand |

## Example use cases

Concrete workflows, from a single CLI call to a full agentic pipeline. The MCP examples show raw tool-call arguments/results so you can see exactly what an agent framework (Claude Desktop, LM Studio, a custom LangGraph/agent-SDK loop) sends and gets back over `ruddydoc serve --mcp`.

### AI agent workflows

**Grounded Q&A without blowing the context window.** Instead of pasting an entire PDF into an LLM's prompt, an agent converts it once and then asks questions against the graph instead of the raw text:

```json
{"tool": "convert_document", "arguments": {"source": "/data/quarterly-report.pdf"}}
-> {"id": "a1b2c3...", "format": "Pdf", "page_count": 42, "validation": {"conforms": true, "results": []}}

{"tool": "ask_document", "arguments": {"document_id": "a1b2c3...", "question": "What was revenue growth in Q3?"}}
-> {"sparql": "SELECT ?text WHERE { ... }", "result": [{"text": "..."}], "repairs": 0}
```

The response includes the generated SPARQL (`sparql`) and how many self-repair attempts it took (`repairs`) -- an agent, or a human reviewing its trace, can check the answer is actually backed by a query over the document instead of trusting an LLM's unverified recollection of the text. This also means only the question and a handful of matched triples ever enter the LLM's context, not the whole document.

**Multi-document research assistant.** Point a single query at several sources -- each gets its own named graph inside one shared store, so one SPARQL query can join across them:

```bash
ruddydoc query 'SELECT ?doc ?text WHERE {
  GRAPH ?doc { ?el <https://ruddydoc.chapeaux.io/ontology#textContent> ?text }
  FILTER(CONTAINS(?text, "supply chain"))
}' q1-earnings.pdf q2-earnings.pdf q3-earnings.pdf
```

An agent doing the equivalent over MCP converts each source with `convert_document`, uses `list_documents` to keep track of what's loaded in the session, and calls `search_text` or `ask_document` per document to compare findings across the set -- without ever holding all three documents' text in its own context at once.

**RAG chatbot backend with no vector database.** Ingest once at startup, then answer live queries by embedding and searching in-memory:

```json
{"tool": "embed_document", "arguments": {"document_id": "a1b2c3...", "max_tokens": 512}}
{"tool": "semantic_search", "arguments": {"document_id": "a1b2c3...", "query": "how do we handle returns?", "limit": 5}}
-> {"result": [{"chunk": "...", "score": 0.87}, ...]}
```

Feed the top-k chunks into your own LLM call as context. No Pinecone/Qdrant/pgvector to run -- the embeddings live in the same process as the graph.

**Self-verifying pipeline for untrusted input.** An agent ingesting user-uploaded documents shouldn't blindly trust extraction results. `convert_document`'s response already includes the SHACL conformance report (no second call needed for the initial check), and `canonicalize_document`/`validate_document` are there to re-check on demand after the graph changes:

```json
{"tool": "convert_document", "arguments": {"source": "/uploads/user-submitted.docx"}}
-> {"id": "a1b2c3...", "format": "Docx", "validation": {"conforms": true, "results": []}}

{"tool": "canonicalize_document", "arguments": {"document_id": "a1b2c3..."}}
-> {"canonical_hash": "8ab2da25fa49285a9fa6538901da7831976e5e457e76826e8e05b619b82b8ee2"}
```

If `validation.conforms` is `false`, the agent can branch to a fallback path or flag the document for human review instead of feeding malformed structure downstream. The canonical hash plus the automatically-recorded PROV-O lineage (`prov:wasGeneratedBy`, `prov:used`) gives a standards-compliant record of exactly which conversion activity produced which graph, from which source -- useful evidence in a compliance-sensitive pipeline.

**Content-addressed deduplication across formats.** The same report submitted once as a PDF and later as a re-typed DOCX produces different source bytes but, if the extracted content matches, the *same* canonical hash -- because `--canonical-hash`/`canonicalize_document` hashes the derived RDF graph, not the source file:

```bash
ruddydoc convert report-v1.pdf --canonical-hash   # canonical hash: 8ab2da25...
ruddydoc convert report-v1.docx --canonical-hash  # canonical hash: 8ab2da25... (same)
```

An ingestion agent can use this hash as a cache/dedup key to skip reprocessing semantically-identical documents regardless of which format they arrived in.

### Other workflows

**CI/CD documentation quality gate.** Every conversion carries a SHACL conformance report; a CI job driving `ruddydoc serve --mcp` can gate a merge on it instead of silently accepting malformed docs:

```json
{"tool": "convert_document", "arguments": {"source": "docs/getting-started.md"}}
-> {"id": "...", "validation": {"conforms": false, "results": [
     {"message": "Missing required property: rdoc:textContent", "focusNode": "urn:...#paragraph-3"}
   ]}}
```

A build step that converts every changed doc this way and fails when `validation.conforms` is `false` catches structural problems (missing headings, malformed tables) before they ship -- the same check that already runs on every conversion, just enforced instead of ignored.

**Batch knowledge-base ingestion.** Convert a directory of mixed-format documents (PDF, DOCX, HTML, Markdown) into one queryable graph for an internal search or wiki tool:

```bash
ruddydoc convert ./handbook/*.{pdf,docx,md,html} --format turtle --output ./kb/
```

Serve the resulting graph behind `query_document`/`search_text`/`semantic_search` for engineers to search across the whole handbook by meaning, not just keyword.

## Benchmarks

Measured with Criterion on this repository's own hardware (`cargo bench -p ruddydoc-bench`); numbers vary by machine but the relative shape holds. All conversion numbers below go through the real `DocumentConverter::convert()` path, including RDFS materialization, SHACL validation, and PROV-O lineage recording -- there's no separate "fast path" that skips the semantic layer.

### Parsing performance

| Format | Fixture size | Time |
|--------|-------------|------|
| Markdown | sample fixture | 0.60 ms |
| Markdown | 1,000 lines | 10.8 ms |
| Markdown | 10,000 lines | 127 ms |
| HTML | sample fixture | 0.71 ms |
| HTML | 500 elements | 11.1 ms |
| CSV | sample fixture | 0.48 ms |
| CSV | 1,000 rows | 78.8 ms |
| LaTeX | 1,000 lines | 8.2 ms |

### End-to-end conversion (full pipeline: parse + RDFS + SHACL + PROV-O + export)

| Document size | Export format | Time |
|---------------|----------------|------|
| 100-line Markdown | JSON | 5.6 ms |
| 500-line Markdown | JSON | 17.5 ms |
| 1,000-line Markdown | JSON | 39.7 ms |

### Export performance (500-line Markdown source)

| Format | Time |
|--------|------|
| WebVTT | 0.41 ms |
| N-Triples | 0.60 ms |
| JSON-LD | 0.68 ms |
| RDF/XML | 1.1 ms |
| Turtle | 3.6 ms |
| Text | 3.2 ms |
| DocTags | 5.7 ms |
| Markdown | 6.1 ms |
| JSON | 6.2 ms |
| HTML | 6.8 ms |

### Graph operations

| Operation | Scale | Time |
|-----------|-------|------|
| Insert triples | 1,000 | 1.2 ms |
| SPARQL SELECT | 1,000 elements | 3.4 ms |
| Serialize to Turtle | 1,000 elements | 4.4 ms |
| Clear graph | 1,000 elements | 31 us |
| Chunk for RAG | 500-line document | 4.1 ms |

### vs Python docling

> **Methodology note:** the docling-side figures below are documented reference points for docling's known dependency footprint and typical reported startup/memory characteristics -- they were not re-benchmarked side-by-side in this session. Installing docling pulls in PyTorch, the full CUDA toolkit, transformers, and OpenCV (confirmed via `pip install --dry-run docling`: 60+ packages, several GB), which is impractical to install just for a benchmark run. The RuddyDoc-side figures are freshly measured (see above).

| Metric | RuddyDoc | Python docling |
|--------|----------|----------------|
| Startup time | 5 ms | ~2 s |
| Binary size | 17 MB | 2+ GB (with ML deps) |
| Parse 1,000-line Markdown | 10.8 ms | ~70 ms |
| Full convert + export (1,000-line Markdown) | 39.7 ms (incl. reasoning, validation, provenance) | N/A (docling has no equivalent semantic layer) |
| Memory (batch 100 files) | ~50 MB | ~500 MB |

## Installation

> **Note:** `cargo install ruddydoc` is currently unavailable. RuddyDoc depends on
> [Sparq](https://github.com/sparq-org/sparq) via a pinned git dependency (Sparq
> isn't published to crates.io yet), and `cargo publish` requires every
> dependency to be registry-resolvable. Use one of the other install methods
> below, or build from source. See [LICENSES.md](LICENSES.md#special-consideration-sparq).

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

# Convert with a canonical content hash of the derived graph
ruddydoc convert paper.pdf --canonical-hash

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

# Count elements by type (RDFS materialization means superclasses count too)
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

# MCP server (stdio, for Claude Desktop / LM Studio / any MCP client)
ruddydoc serve --mcp
```

## CLI reference

| Command | Description |
|---------|-------------|
| `convert` | Convert documents to specified output format(s) (`--canonical-hash` for an RDFC-1.0 content hash) |
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
          Sparq Store (RDF knowledge graph, 28 classes, 65+ properties, 3 SHACL shapes)
                |
                v
          RDFS/OWL-RL materialization + SHACL validation + PROV-O lineage (every conversion)
                |
                v
          Pipeline (optional ML enrichment: layout, OCR, table, VLM)
                |
                v
          Export (10 output formats) / SPARQL / Full-text + vector search / NLQ / Chunking for RAG
```

Key architectural decisions:
- **Graph-first**: Documents are RDF graphs, not flat data models. Export formats are projections.
- **Crate-per-concern**: 12 backend crates, independently compilable and testable.
- **Reasoning is infrastructure, not an add-on**: RDFS materialization and SHACL validation run on every conversion (not opt-in), the same tier as ontology loading -- semantic correctness is a baseline guarantee, not a special mode.
- **Sparq types stay encapsulated**: only `ruddydoc-graph` ever names a Sparq type (`sparq_core::Graph`, `sparq_introspect::*`, etc.); every other crate talks to the engine-agnostic `DocumentStore` trait.
- **Feature-gated ML and providers**: ONNX Runtime, VLM support, and the embedding/LLM HTTP providers are all optional and off by default. Base binary has zero ML dependencies and makes zero outbound network calls.
- **Embedded store**: Sparq is in-process (like SQLite). No external services needed for reasoning, validation, or full-text search. Vector search and NLQ are the only capabilities requiring an external HTTP call (an embedding/LLM endpoint you configure). Sparq is pinned via a git dependency across 9 crates (not yet published to crates.io) -- see [LICENSES.md](LICENSES.md#special-consideration-sparq).

For detailed architecture, see [INITIAL_PLAN.md](INITIAL_PLAN.md).

## Comparison with Python docling

RuddyDoc is a Rust rewrite of [docling](https://github.com/docling-project/docling) with substantial semantic enhancements docling doesn't have.

| | RuddyDoc | Python docling |
|--|----------|----------------|
| Language | Rust | Python |
| Startup | 5 ms | ~2 s |
| Binary | 17 MB | 2+ GB with ML |
| Input formats | 12 | 12 |
| Output formats | 10 (+ 4 RDF) | 6 |
| Knowledge graph | Sparq (SPARQL) | None |
| RDFS/OWL-RL reasoning | Built-in, on by default | None |
| SHACL validation | Built-in, every conversion | None |
| Full-text search | Built-in (BM25) | None |
| Vector search / RAG | Built-in (in-memory) | Via external vector DB |
| Natural-language querying | Built-in (self-repairing) | None |
| RDF canonicalization | Built-in (RDFC-1.0) | None |
| Provenance | W3C PROV-O + detailed bespoke model | Limited |
| Chunking | Built-in CLI | Via docling-core |
| Server | Built-in REST + MCP (16 tools) | Separate docling-mcp |
| VLM support | HTTP API to any endpoint | transformers/vLLM |
| Tests | 797 | ~200 |

For migration details, see [docs/migration-from-docling.md](docs/migration-from-docling.md).

## Building from source

```bash
git clone https://github.com/chapeaux/ruddydoc.git
cd ruddydoc
cargo build --release
# Binary at target/release/ruddydoc
```

The first build needs network access to fetch Sparq from its git repository
(it's a pinned commit, not a crates.io dependency -- 9 Sparq crates in total:
the core store/engine plus introspection, reasoning, SHACL, full-text,
NLQ, canonicalization, and PROV-O).

Run tests: `cargo test --workspace`
Run benchmarks: `cargo bench -p ruddydoc-bench`
Run clippy: `cargo clippy --workspace -- -D warnings`

## Project structure

```
crates/
  ruddydoc-core/          Shared types, traits, format detection
  ruddydoc-graph/         Sparq store wrapper: SPARQL, reasoning, SHACL, search, vectors, NLQ, canonicalization, PROV-O
  ruddydoc-ontology/      Document ontology (28 classes, 65+ properties) + SHACL shapes
  ruddydoc-converter/     Format detection, backend dispatch, reasoning/validation/provenance pipeline
  ruddydoc-pipeline/      ML pipeline stages, DocTags parser
  ruddydoc-models/        ONNX Runtime, VLM/embedding/LLM HTTP API clients
  ruddydoc-export/        All 10 exporters + chunking
  ruddydoc-server/        REST API (axum) + MCP server (16 tools)
  ruddydoc-cli/           CLI binary (clap)
  ruddydoc-backend-*/     12 format-specific parsers
  ruddydoc-bench/         Criterion benchmarks
  ruddydoc-tests/         Compatibility test suite
ontology/
  ruddydoc.ttl            Document ontology + inline SHACL shapes (loaded together into the store's shapes graph)
```

## Contributing

Contributions are welcome. Key areas:

- New input format backends
- Export format improvements
- ML model integrations (ONNX models for layout, OCR, table structure)
- Broader SHACL shape coverage (currently Document/TextElement/TableElement)
- Performance optimizations
- Documentation

## License

MIT. See [LICENSE](LICENSE).
