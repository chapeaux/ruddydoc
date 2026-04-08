# RuddyDoc: Initial Implementation Plan

RuddyDoc is a Rust rewrite of [docling](https://github.com/docling-project/docling) (v2.85.0), with an embedded Oxigraph datastore for RDF-compatible export. The goal is to beat Python docling in every performance metric while adding a semantic knowledge graph that makes parsed documents maximally useful to AI agents.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Crate Structure](#crate-structure)
3. [Document Ontology](#document-ontology)
4. [Phase 1: Foundation](#phase-1-foundation)
5. [Phase 2: Text-Based Backends](#phase-2-text-based-backends)
6. [Phase 3: Binary Format Backends](#phase-3-binary-format-backends)
7. [Phase 4: ML Model Integration](#phase-4-ml-model-integration)
8. [Phase 5: PDF Positional Parsing, VLM Pipeline, Server, and CLI](#phase-5-pdf-positional-parsing-vlm-pipeline-server-and-cli)
9. [Phase 6: Performance and Parity](#phase-6-performance-and-parity)
10. [Team Assignments](#team-assignments)
11. [Decision Log](#decision-log)

---

## Architecture Overview

### Python docling architecture (what we are replacing)

```
Input File
  |
  v
DocumentConverter  -- selects backend by format/mime
  |
  v
Backend (format-specific parser: PDF, DOCX, HTML, CSV, ...)
  |
  v
Pipeline (standard_pdf, vlm, asr, simple)
  |    |-- ML Models (layout, OCR, table structure, VLM)
  |    |-- Page assembly, reading order
  |
  v
DoclingDocument (Pydantic model: texts, tables, pictures, body tree)
  |
  v
Export (Markdown, HTML, DocTags, JSON, WebVTT)
```

### RuddyDoc architecture (what we are building)

```
Input File
  |
  v
ruddydoc-converter  -- selects backend by format/mime
  |
  v
ruddydoc-backend-{format}  (format-specific parser)
  |
  v
ruddydoc-pipeline  (orchestrates parsing + optional ML enrichment)
  |    |-- ruddydoc-models (ONNX Runtime for layout/OCR/table)
  |    |-- page assembly, reading order
  |
  v
ruddydoc-graph (Oxigraph store)  -- RDF triples representing the document
  |    |-- ruddydoc-ontology (document ontology + SHACL shapes)
  |    |-- SPARQL queryable
  |
  v
ruddydoc-export  (Markdown, HTML, DocTags, JSON, RDF/Turtle, JSON-LD, N-Triples)
  |
  v
ruddydoc-cli  (thin binary: parse args, wire crates, run)
```

### Key architectural differences from Python docling

1. **Graph-first representation**: Instead of building a Pydantic model and serializing to JSON, we build an RDF knowledge graph in Oxigraph. The graph IS the document representation. Export formats (including the docling-compatible JSON) are projections of the graph.

2. **Crate-per-concern**: Instead of a monolithic Python package with subdirectories, each concern is a separate Rust crate with a thin public API. This enables independent compilation, testing, and eventually independent versioning.

3. **No Python ML dependencies**: We replace PyTorch/torchvision with ONNX Runtime for inference. Models are loaded as ONNX files. This eliminates the ~2GB Python ML dependency chain.

4. **Embedded, not external**: The Oxigraph store is in-process (like SQLite), not a separate service. Zero deployment overhead.

---

## Crate Structure

### Workspace layout

```
ruddydoc/
  Cargo.toml              (workspace root)
  crates/
    ruddydoc-core/        (shared types, error handling, config)
    ruddydoc-graph/       (Oxigraph store wrapper, SPARQL)
    ruddydoc-ontology/    (document ontology, SHACL shapes, vocabulary)
    ruddydoc-converter/   (format detection, backend dispatch)
    ruddydoc-pipeline/    (orchestration, page assembly, reading order)
    ruddydoc-models/      (ONNX Runtime ML model integration)
    ruddydoc-export/      (Markdown, HTML, DocTags, JSON, RDF format exporters)
    ruddydoc-cli/         (CLI binary)
    ruddydoc-backend-md/      (Markdown parser backend)
    ruddydoc-backend-html/    (HTML parser backend)
    ruddydoc-backend-csv/     (CSV parser backend)
    ruddydoc-backend-docx/    (DOCX parser backend)
    ruddydoc-backend-pdf/     (PDF parser backend)
    ruddydoc-backend-latex/   (LaTeX parser backend)
    ruddydoc-backend-pptx/    (PPTX parser backend)
    ruddydoc-backend-xlsx/    (XLSX parser backend)
    ruddydoc-backend-image/   (Image backend for OCR-only flow)
    ruddydoc-backend-xml/     (USPTO, JATS, XBRL XML parser backends)
    ruddydoc-backend-webvtt/  (WebVTT subtitle parser backend)
    ruddydoc-backend-asciidoc/ (AsciiDoc parser backend)
    ruddydoc-server/       (Combined HTTP REST + MCP server)
  ontology/
    ruddydoc.ttl           (RuddyDoc document ontology)
    shapes.ttl             (SHACL shapes for validation)
  tests/
    integration/           (cross-crate integration tests)
    fixtures/              (test documents in each format)
    benchmarks/            (criterion benchmarks vs Python docling)
```

### Crate dependency graph

```
ruddydoc-cli
  +-- ruddydoc-converter  -> ruddydoc-core, ruddydoc-graph, ruddydoc-pipeline
  +-- ruddydoc-export     -> ruddydoc-core, ruddydoc-graph
  +-- ruddydoc-server     (optional, feature-gated)
  +-- ruddydoc-core

ruddydoc-server
  +-- ruddydoc-core
  +-- ruddydoc-graph
  +-- ruddydoc-converter
  +-- ruddydoc-export
  +-- rust-mcp-sdk         (MCP protocol)
  +-- axum                 (HTTP server)
  +-- tokio                (async runtime)

ruddydoc-converter
  +-- ruddydoc-core
  +-- ruddydoc-graph
  +-- ruddydoc-pipeline
  +-- ruddydoc-backend-*  (each backend)

ruddydoc-pipeline
  +-- ruddydoc-core
  +-- ruddydoc-graph
  +-- ruddydoc-models     (optional, feature-gated)

ruddydoc-graph
  +-- ruddydoc-core
  +-- oxigraph

ruddydoc-ontology
  +-- ruddydoc-core
  +-- ruddydoc-graph

ruddydoc-models
  +-- ruddydoc-core
  +-- ort (ONNX Runtime bindings)
  +-- reqwest (optional, for VLM HTTP API calls, feature-gated)

ruddydoc-export
  +-- ruddydoc-core
  +-- ruddydoc-graph

ruddydoc-backend-*
  +-- ruddydoc-core
  +-- ruddydoc-graph
  +-- (format-specific parsing crate)
```

No circular dependencies. `ruddydoc-core` is the leaf.

### Key dependencies (following beret conventions)

| Crate | Dependency | Purpose |
|-------|-----------|---------|
| ruddydoc-graph | `oxigraph = "0.5.6"` | Embedded RDF store and SPARQL engine |
| ruddydoc-core | `serde = { version = "1", features = ["derive"] }` | Serialization |
| ruddydoc-core | `serde_json = "1"` | JSON support |
| ruddydoc-cli | `clap = { version = "4", features = ["derive"] }` | CLI argument parsing |
| ruddydoc-backend-md | `pulldown-cmark = "0.12"` | Markdown parsing |
| ruddydoc-backend-html | `scraper = "0.22"` or `lol_html = "2"` | HTML parsing |
| ruddydoc-backend-csv | `csv = "1"` | CSV parsing |
| ruddydoc-backend-docx | `docx-rs` or custom ZIP+XML | DOCX parsing |
| ruddydoc-backend-pdf | `pdfium-render = "0.9"` + `pdfium-auto = "0.3"` | PDF text extraction with positions, page rendering |
| ruddydoc-backend-pdf | `lopdf = "0.34"` (retained) | PDF metadata extraction, fallback |
| ruddydoc-backend-xlsx | `calamine = "0.26"` | Excel parsing |
| ruddydoc-backend-latex | custom parser | LaTeX parsing |
| ruddydoc-backend-xml | `quick-xml = "0.37"` | XML parsing |
| ruddydoc-models | `ort = "2"` | ONNX Runtime inference |
| ruddydoc-models | `reqwest = { version = "0.12", features = ["json"], optional = true }` | VLM HTTP API calls |
| ruddydoc-export | `oxigraph = "0.5.6"` (via ruddydoc-graph) | RDF serialization |
| ruddydoc-pipeline | `rayon = "1"` | Parallel page processing |
| ruddydoc-server | `axum = "0.8"` | HTTP REST server |
| ruddydoc-server | `rust-mcp-sdk = "0.9"` | MCP protocol (stdio + HTTP/SSE) |
| ruddydoc-server | `tower = "0.5"` | HTTP middleware (CORS, tracing) |
| all | `tokio = { version = "1", features = ["full"] }` | Async runtime (CLI, server, benchmarks) |

Edition: **2024** (following beret).

---

## Document Ontology

The document ontology defines how parsed documents are represented as RDF triples in the Oxigraph store. This is the core innovation over Python docling: instead of a flat data model, documents become queryable knowledge graphs.

### Design principles

1. **AI-agent comprehension first**: The ontology should make it trivial for an AI agent to answer questions like "What tables are in this document?", "What is the reading order of section 3?", "Which figures have captions?".

2. **Reuse established vocabularies**: Use schema.org for general metadata, Dublin Core for bibliographic metadata, and a RuddyDoc-specific vocabulary only for document structure concepts not covered elsewhere.

3. **Named graphs for document isolation**: Each parsed document gets its own named graph (`urn:ruddydoc:doc:{hash}`). The ontology lives in `urn:ruddydoc:ontology`. This allows querying across documents or within a single document.

4. **Users never see RDF**: The ontology is an internal implementation detail. Users interact with JSON output, Markdown export, and CLI commands. SPARQL is available for power users and AI agents, but is never required.

### Namespace

```
Prefix: rdoc:
IRI: https://ruddydoc.chapeaux.io/ontology#

Document namespace: urn:ruddydoc:doc:{document_hash}
Element namespace: urn:ruddydoc:doc:{document_hash}/{element_id}
```

### Core classes

```turtle
rdoc:Document          a rdfs:Class ;
    rdfs:label         "Document" ;
    rdfs:comment       "A parsed document, the top-level container." .

rdoc:Page              a rdfs:Class ;
    rdfs:label         "Page" ;
    rdfs:comment       "A page within a paginated document (PDF, PPTX)." .

rdoc:DocumentElement   a rdfs:Class ;
    rdfs:label         "Document Element" ;
    rdfs:comment       "Any structural element within a document." .

rdoc:TextElement       rdfs:subClassOf rdoc:DocumentElement ;
    rdfs:label         "Text Element" ;
    rdfs:comment       "A text-bearing element (paragraph, heading, list item, etc.)." .

rdoc:Title             rdfs:subClassOf rdoc:TextElement ;
    rdfs:label         "Title" .

rdoc:SectionHeader     rdfs:subClassOf rdoc:TextElement ;
    rdfs:label         "Section Header" .

rdoc:Paragraph         rdfs:subClassOf rdoc:TextElement ;
    rdfs:label         "Paragraph" .

rdoc:ListItem          rdfs:subClassOf rdoc:TextElement ;
    rdfs:label         "List Item" .

rdoc:Footnote          rdfs:subClassOf rdoc:TextElement ;
    rdfs:label         "Footnote" .

rdoc:Caption           rdfs:subClassOf rdoc:TextElement ;
    rdfs:label         "Caption" .

rdoc:Code              rdfs:subClassOf rdoc:TextElement ;
    rdfs:label         "Code Block" .

rdoc:Formula           rdfs:subClassOf rdoc:TextElement ;
    rdfs:label         "Formula" .

rdoc:TableElement      rdfs:subClassOf rdoc:DocumentElement ;
    rdfs:label         "Table" ;
    rdfs:comment       "A table with rows, columns, and cells." .

rdoc:TableCell         a rdfs:Class ;
    rdfs:label         "Table Cell" .

rdoc:PictureElement    rdfs:subClassOf rdoc:DocumentElement ;
    rdfs:label         "Picture" ;
    rdfs:comment       "An image or figure in the document." .

rdoc:KeyValueItem      rdfs:subClassOf rdoc:DocumentElement ;
    rdfs:label         "Key-Value Item" ;
    rdfs:comment       "A form field or key-value pair." .

rdoc:Group             a rdfs:Class ;
    rdfs:label         "Group" ;
    rdfs:comment       "A logical grouping of elements (e.g., a list, a section)." .

rdoc:Furniture         a rdfs:Class ;
    rdfs:label         "Furniture" ;
    rdfs:comment       "Page furniture: headers, footers, page numbers." .

rdoc:PageHeader        rdfs:subClassOf rdoc:Furniture ;
    rdfs:label         "Page Header" .

rdoc:PageFooter        rdfs:subClassOf rdoc:Furniture ;
    rdfs:label         "Page Footer" .
```

### Core properties

```turtle
# Document-level
rdoc:hasElement        a rdf:Property ;
    rdfs:domain        rdoc:Document ;
    rdfs:range         rdoc:DocumentElement ;
    rdfs:label         "has element" .

rdoc:hasPage           a rdf:Property ;
    rdfs:domain        rdoc:Document ;
    rdfs:range         rdoc:Page ;
    rdfs:label         "has page" .

rdoc:pageNumber        a rdf:Property ;
    rdfs:domain        rdoc:Page ;
    rdfs:range         xsd:integer ;
    rdfs:label         "page number" .

rdoc:sourceFormat      a rdf:Property ;
    rdfs:domain        rdoc:Document ;
    rdfs:range         xsd:string ;
    rdfs:label         "source format" ;
    rdfs:comment       "The input format of the document (pdf, docx, html, etc.)." .

rdoc:documentHash      a rdf:Property ;
    rdfs:domain        rdoc:Document ;
    rdfs:range         xsd:string ;
    rdfs:label         "document hash" .

# Element-level
rdoc:textContent       a rdf:Property ;
    rdfs:domain        rdoc:TextElement ;
    rdfs:range         xsd:string ;
    rdfs:label         "text content" .

rdoc:readingOrder      a rdf:Property ;
    rdfs:domain        rdoc:DocumentElement ;
    rdfs:range         xsd:integer ;
    rdfs:label         "reading order" ;
    rdfs:comment       "Position in the document's reading order (0-indexed)." .

rdoc:onPage            a rdf:Property ;
    rdfs:domain        rdoc:DocumentElement ;
    rdfs:range         rdoc:Page ;
    rdfs:label         "on page" .

rdoc:headingLevel      a rdf:Property ;
    rdfs:domain        rdoc:SectionHeader ;
    rdfs:range         xsd:integer ;
    rdfs:label         "heading level" .

rdoc:parentElement     a rdf:Property ;
    rdfs:domain        rdoc:DocumentElement ;
    rdfs:range         rdoc:DocumentElement ;
    rdfs:label         "parent element" ;
    rdfs:comment       "The parent in the document tree structure." .

rdoc:childElement      a rdf:Property ;
    rdfs:domain        rdoc:DocumentElement ;
    rdfs:range         rdoc:DocumentElement ;
    rdfs:label         "child element" ;
    rdfs:comment       "A child in the document tree structure." .

rdoc:nextElement       a rdf:Property ;
    rdfs:domain        rdoc:DocumentElement ;
    rdfs:range         rdoc:DocumentElement ;
    rdfs:label         "next element" ;
    rdfs:comment       "The next element in reading order." .

# Bounding box (for paginated formats)
rdoc:boundingBox       a rdf:Property ;
    rdfs:domain        rdoc:DocumentElement ;
    rdfs:label         "bounding box" .

rdoc:bboxLeft          a rdf:Property ; rdfs:range xsd:float ; rdfs:label "left" .
rdoc:bboxTop           a rdf:Property ; rdfs:range xsd:float ; rdfs:label "top" .
rdoc:bboxRight         a rdf:Property ; rdfs:range xsd:float ; rdfs:label "right" .
rdoc:bboxBottom        a rdf:Property ; rdfs:range xsd:float ; rdfs:label "bottom" .

# Table-specific
rdoc:hasCell           a rdf:Property ;
    rdfs:domain        rdoc:TableElement ;
    rdfs:range         rdoc:TableCell ;
    rdfs:label         "has cell" .

rdoc:cellRow           a rdf:Property ; rdfs:domain rdoc:TableCell ; rdfs:range xsd:integer ; rdfs:label "row" .
rdoc:cellColumn        a rdf:Property ; rdfs:domain rdoc:TableCell ; rdfs:range xsd:integer ; rdfs:label "column" .
rdoc:cellRowSpan       a rdf:Property ; rdfs:domain rdoc:TableCell ; rdfs:range xsd:integer ; rdfs:label "row span" .
rdoc:cellColSpan       a rdf:Property ; rdfs:domain rdoc:TableCell ; rdfs:range xsd:integer ; rdfs:label "column span" .
rdoc:cellText          a rdf:Property ; rdfs:domain rdoc:TableCell ; rdfs:range xsd:string ; rdfs:label "cell text" .
rdoc:isHeader          a rdf:Property ; rdfs:domain rdoc:TableCell ; rdfs:range xsd:boolean ; rdfs:label "is header cell" .

# Picture-specific
rdoc:pictureData       a rdf:Property ;
    rdfs:domain        rdoc:PictureElement ;
    rdfs:range         xsd:base64Binary ;
    rdfs:label         "picture data" .

rdoc:pictureFormat     a rdf:Property ;
    rdfs:domain        rdoc:PictureElement ;
    rdfs:range         xsd:string ;
    rdfs:label         "picture format" .

rdoc:hasCaption        a rdf:Property ;
    rdfs:domain        rdoc:PictureElement ;
    rdfs:range         rdoc:Caption ;
    rdfs:label         "has caption" .

# Confidence / provenance
rdoc:confidence        a rdf:Property ;
    rdfs:domain        rdoc:DocumentElement ;
    rdfs:range         xsd:float ;
    rdfs:label         "confidence" ;
    rdfs:comment       "ML model confidence score (0.0-1.0) for this element." .

rdoc:detectedBy        a rdf:Property ;
    rdfs:domain        rdoc:DocumentElement ;
    rdfs:range         xsd:string ;
    rdfs:label         "detected by" ;
    rdfs:comment       "Name of the model or backend that produced this element." .
```

### Bridging to schema.org (for metadata)

```turtle
# Document metadata maps to schema.org
# rdoc:Document  rdfs:subClassOf schema:CreativeWork (when exporting JSON-LD)
# dc:title       -> schema:name
# dc:creator     -> schema:author
# dc:date        -> schema:datePublished
# dc:language    -> schema:inLanguage
```

The ontologist will finalize these mappings. The point is that document-level metadata uses established vocabularies, while document structure uses the `rdoc:` vocabulary.

---

## Phase 1: Foundation

**Goal**: Workspace compiles. Core types exist. Oxigraph store wrapper works. One backend (Markdown) parses documents into the graph. One exporter (JSON) serializes the graph back out.

**Duration**: 2-3 weeks.

### Deliverables

#### 1.1 Workspace setup (ruddydoc-core)

- `Cargo.toml` workspace with all crate stubs
- `ruddydoc-core` with:
  - `InputFormat` enum (matching Python's 17 formats)
  - `OutputFormat` enum (Markdown, HTML, JSON, Text, DocTags, VTT, Turtle, NTriples, JsonLd)
  - `ConversionStatus` enum (Pending, Started, Success, PartialSuccess, Failure, Skipped)
  - `Error` type following beret pattern (`Box<dyn std::error::Error>`)
  - `DocumentHash` newtype
  - `BoundingBox` struct
  - `DocumentMeta` struct (file path, hash, format, file size, page count)
  - Format detection: `detect_format(path: &Path) -> Option<InputFormat>` using magic bytes and extension
  - MIME type mapping tables (ported from Python's `FormatToMimeType`, `FormatToExtensions`, `MimeTypeToFormat`)

#### 1.2 Graph store (ruddydoc-graph)

- `DocumentStore` struct wrapping `oxigraph::store::Store` (adapted from beret's `CodebaseStore`)
- Methods:
  - `new() -> Result<Self>`
  - `insert_triple(subject, predicate, object) -> Result<()>`
  - `insert_triple_into(subject, predicate, object, graph) -> Result<()>` (named graph support)
  - `insert_literal(subject, predicate, value, datatype) -> Result<()>` (for typed literals: strings, integers, floats, booleans)
  - `query_to_json(sparql) -> Result<Value>`
  - `clear() -> Result<()>`
  - `clear_graph(graph) -> Result<()>` (clear a single document's graph)
  - `serialize_graph(graph, format) -> Result<String>` (Turtle, N-Triples, JSON-LD)
  - `triple_count() -> Result<usize>`
  - `triple_count_in(graph) -> Result<usize>`
- IRI construction helpers:
  - `doc_iri(hash: &str) -> String` (-> `urn:ruddydoc:doc:{hash}`)
  - `element_iri(hash: &str, id: &str) -> String` (-> `urn:ruddydoc:doc:{hash}/{id}`)
  - `ontology_iri(term: &str) -> String` (-> `https://ruddydoc.chapeaux.io/ontology#{term}`)
- IRI escaping adapted from beret's `iri_escape()`
- Unit tests covering insert, query, clear, named graphs, literal types

#### 1.3 Document ontology (ruddydoc-ontology)

- `ontology/ruddydoc.ttl` containing all classes and properties defined above
- `ontology/shapes.ttl` with basic SHACL shapes validating:
  - Every `rdoc:Document` has at least one `rdoc:hasElement`
  - Every `rdoc:TextElement` has `rdoc:textContent`
  - Every `rdoc:SectionHeader` has `rdoc:headingLevel`
  - Every `rdoc:TableCell` has `rdoc:cellRow` and `rdoc:cellColumn`
- `ruddydoc-ontology` crate:
  - `load_ontology(store: &DocumentStore) -> Result<()>` loads the bundled .ttl into the ontology named graph
  - Ontology terms as Rust constants: `pub const CLASS_DOCUMENT: &str = "Document";` etc.

#### 1.4 Backend trait and Markdown backend (ruddydoc-backend-md)

- Backend trait in `ruddydoc-core`:
  ```rust
  pub trait DocumentBackend: Send + Sync {
      fn supported_formats(&self) -> &[InputFormat];
      fn supports_pagination(&self) -> bool;
      fn is_valid(&self, source: &DocumentSource) -> bool;
      fn parse(&self, source: &DocumentSource, store: &DocumentStore) -> Result<DocumentMeta>;
  }
  ```
- `DocumentSource` enum: `File(PathBuf)` or `Stream { name: String, data: Vec<u8> }`
- `ruddydoc-backend-md` implementing `DocumentBackend`:
  - Uses `pulldown-cmark` with GFM extensions
  - Parses Markdown into the graph:
    - Creates `rdoc:Document` node
    - Creates `rdoc:SectionHeader` nodes for headings with `rdoc:headingLevel`
    - Creates `rdoc:Paragraph` nodes for text
    - Creates `rdoc:ListItem` nodes for list items
    - Creates `rdoc:Code` nodes for code blocks
    - Creates `rdoc:TableElement` and `rdoc:TableCell` nodes for tables
    - Sets `rdoc:readingOrder` on all elements
    - Sets `rdoc:textContent` on text elements
    - Establishes `rdoc:parentElement`/`rdoc:childElement` tree structure
    - All triples go into the document's named graph

#### 1.5 JSON exporter (ruddydoc-export)

- Exporter trait in `ruddydoc-core`:
  ```rust
  pub trait DocumentExporter: Send + Sync {
      fn format(&self) -> OutputFormat;
      fn export(&self, store: &DocumentStore, doc_graph: &str) -> Result<String>;
  }
  ```
- `JsonExporter` in `ruddydoc-export`:
  - Queries the document graph via SPARQL
  - Produces JSON matching Python docling's `DoclingDocument` schema (for compatibility)
  - Structure: `{ "name": ..., "texts": [...], "tables": [...], "pictures": [...], "body": {...} }`
- `TurtleExporter`: serializes the document's named graph as Turtle
- `NTriplesExporter`: serializes as N-Triples

#### 1.6 Minimal CLI (ruddydoc-cli)

- `ruddydoc convert <input> [--output <path>] [--format json|turtle|ntriples]`
- `ruddydoc --version`
- `ruddydoc --help`
- Uses clap derive
- Only Markdown input works in Phase 1
- Defaults to JSON output to stdout

### Phase 1 acceptance criteria

- [x] `cargo build` succeeds for entire workspace
- [x] `cargo test` passes with >90% line coverage on ruddydoc-core, ruddydoc-graph, ruddydoc-backend-md
- [x] `cargo clippy -- -D warnings` clean
- [x] `ruddydoc convert test.md` produces valid JSON output matching docling schema structure
- [x] `ruddydoc convert test.md --format turtle` produces valid Turtle
- [x] SPARQL query `SELECT ?e WHERE { ?e a <rdoc:Paragraph> }` returns correct elements
- [x] Benchmark: Markdown parsing is at least 10x faster than Python docling for a 1000-line Markdown file

---

## Phase 2: Text-Based Backends

**Goal**: All text-based format backends work (HTML, CSV, LaTeX, AsciiDoc, WebVTT, XML variants). The converter auto-detects formats.

**Duration**: 3-4 weeks.

### Deliverables

#### 2.1 Format detection (ruddydoc-converter)

- `DocumentConverter` struct:
  - `new(options: ConvertOptions) -> Self`
  - `convert(source: DocumentSource) -> Result<ConversionResult>`
  - `convert_batch(sources: Vec<DocumentSource>) -> Vec<Result<ConversionResult>>`
- Format detection logic ported from Python's `_guess_format()`:
  - Magic byte detection (using `infer` crate or custom)
  - Extension-based fallback
  - Content sniffing for XML, HTML, CSV disambiguation
  - ZIP inspection for OOXML formats (docx vs xlsx vs pptx)
- `ConvertOptions` struct:
  - `format_options: HashMap<InputFormat, FormatOption>` (per-format backend selection and options)
  - `max_file_size: u64`
  - `max_pages: u32`
  - `page_range: Range<u32>`
- `ConversionResult` struct:
  - `input: DocumentMeta`
  - `status: ConversionStatus`
  - `errors: Vec<ErrorItem>`
  - `doc_graph: String` (named graph IRI)
  - `store: Arc<DocumentStore>` (shared reference to the graph)

#### 2.2 HTML backend (ruddydoc-backend-html)

- Port Python's `html_backend.py` (170K lines -- the largest backend)
- Use `scraper` for DOM parsing or `lol_html` for streaming
- Handle:
  - Semantic HTML elements (article, section, aside, nav, header, footer, main)
  - Tables (including nested tables, colspan, rowspan)
  - Lists (ordered, unordered, definition lists)
  - Headings (h1-h6)
  - Images with alt text
  - Links and anchors
  - Code blocks (pre, code)
  - Forms (for key-value extraction)
  - Metadata from head (title, meta tags, Open Graph, JSON-LD)
- Map HTML semantics to ontology classes

#### 2.3 CSV backend (ruddydoc-backend-csv)

- Port Python's `csv_backend.py`
- Auto-detect delimiter (comma, tab, semicolon, pipe) using the `csv` crate's flexibility
- Represent as a single `rdoc:TableElement` with cells
- Handle headers (first row as header cells with `rdoc:isHeader true`)

#### 2.4 LaTeX backend (ruddydoc-backend-latex)

- Port Python's `latex/backend.py` and associated files
- Custom LaTeX parser (no good Rust crate exists for full LaTeX)
- Handle:
  - `\section`, `\subsection`, etc. -> `rdoc:SectionHeader`
  - `\begin{enumerate}`, `\begin{itemize}` -> `rdoc:ListItem`
  - `\begin{table}`, `\begin{tabular}` -> `rdoc:TableElement`
  - `\begin{figure}` -> `rdoc:PictureElement`
  - `\begin{equation}`, `$$...$$`, `\[...\]` -> `rdoc:Formula`
  - `\includegraphics` -> `rdoc:PictureElement`
  - Custom macro expansion (basic support)
  - `\cite`, `\ref`, `\label` -> cross-reference relationships in the graph
- This is complex; start with core structural elements and iterate

#### 2.5 WebVTT backend (ruddydoc-backend-webvtt)

- Port Python's `webvtt_backend.py`
- Parse WebVTT cues into `rdoc:TextElement` with timestamps as properties
- Add `rdoc:startTime` and `rdoc:endTime` properties to the ontology

#### 2.6 AsciiDoc backend (ruddydoc-backend-asciidoc)

- Port Python's `asciidoc_backend.py`
- Parse AsciiDoc structural elements into ontology classes
- Handle sections, blocks, tables, lists, admonitions

#### 2.7 XML backends (ruddydoc-backend-xml)

- **JATS** (Journal Article Tag Suite): scientific journal articles
- **USPTO** (US Patent Office): patent documents
- **XBRL**: financial reporting (feature-gated, complex)
- All use `quick-xml` for parsing
- Each has format-specific semantics mapped to the general document ontology

### Phase 2 acceptance criteria

- [ ] All text-based backends parse their format correctly into the graph
- [ ] Format auto-detection correctly identifies each format from files and streams
- [ ] `ruddydoc convert` works for: .md, .html, .csv, .tex, .vtt, .adoc, .xml (JATS), .xml (USPTO)
- [ ] Round-trip test: parse -> export to JSON -> compare structure against Python docling output
- [ ] Each backend has >80% line coverage in tests

---

## Phase 3: Binary Format Backends

**Goal**: DOCX, XLSX, PPTX, PDF (text extraction only -- no ML), and image backends work.

**Duration**: 4-5 weeks.

### Deliverables

#### 3.1 DOCX backend (ruddydoc-backend-docx)

- Parse OOXML (ZIP containing XML) using `zip` + `quick-xml` or a dedicated crate
- Port Python's `msword_backend.py` (79K lines)
- Handle:
  - Paragraphs with styles (Normal, Heading1-9, ListParagraph, etc.)
  - Tables (including nested tables, merged cells)
  - Images (embedded and linked)
  - Lists (numbered, bulleted -- tricky in OOXML, requires numbering.xml interpretation)
  - Headers and footers
  - Footnotes and endnotes
  - DrawingML objects (basic support)
  - Math equations (OMML -> rdoc:Formula)

#### 3.2 XLSX backend (ruddydoc-backend-xlsx)

- Use `calamine` for reading Excel files
- Port Python's `msexcel_backend.py`
- Each worksheet becomes a `rdoc:TableElement`
- Handle:
  - Multiple sheets (each as a separate table with sheet name)
  - Merged cells (colspan/rowspan)
  - Cell types (string, number, date, boolean, formula result)
  - Header row detection

#### 3.3 PPTX backend (ruddydoc-backend-pptx)

- Parse OOXML (ZIP + XML)
- Port Python's `mspowerpoint_backend.py`
- Each slide becomes a `rdoc:Page`
- Handle:
  - Text frames -> `rdoc:TextElement`
  - Tables -> `rdoc:TableElement`
  - Images -> `rdoc:PictureElement`
  - Slide titles, subtitles
  - Speaker notes (as `rdoc:Footnote` or a new class)
  - Slide reading order (shape z-order)

#### 3.4 PDF backend (ruddydoc-backend-pdf) -- text extraction only

- Use `pdf-extract`, `lopdf`, or `pdfium` bindings for text extraction
- This phase does NOT include ML-based layout analysis (that is Phase 4)
- Handle:
  - Text extraction with position information (bounding boxes)
  - Page segmentation (one `rdoc:Page` per PDF page)
  - Basic reading order from text position
  - Embedded images (extract as binary data)
  - Document metadata from PDF info dictionary
  - Table detection (rule-based, from text positions -- no ML)
- The PDF backend is designed to be enhanced by the ML pipeline in Phase 4

#### 3.5 Image backend (ruddydoc-backend-image)

- Accept image files (PNG, JPEG, TIFF, BMP, WebP)
- In Phase 3: create a `rdoc:Document` with a single `rdoc:PictureElement`
- In Phase 4: pipe through OCR model to extract text
- Image format detection and basic metadata (dimensions, format)

#### 3.6 JSON-Docling backend

- Parse docling-format JSON back into the graph
- This enables round-tripping: docling JSON -> RuddyDoc graph -> export
- Validates the JSON against expected docling schema

### Phase 3 acceptance criteria

- [ ] All binary format backends parse correctly
- [ ] `ruddydoc convert` works for: .docx, .xlsx, .pptx, .pdf, .png/.jpg/.tiff
- [ ] PDF text extraction produces correct reading order for standard documents
- [ ] DOCX list handling correctly identifies numbered vs bulleted lists
- [ ] XLSX merged cell handling produces correct rowspan/colspan
- [ ] Performance: DOCX, XLSX, PPTX parsing faster than Python docling

---

## Phase 4: ML Model Integration

**Goal**: ONNX Runtime integration for layout analysis, table structure recognition, OCR, and picture classification. This is what makes RuddyDoc accurate for complex PDFs.

**Duration**: 4-6 weeks.

### Deliverables

#### 4.1 ONNX Runtime integration (ruddydoc-models)

- Use the `ort` crate (Rust ONNX Runtime bindings)
- `ModelRegistry` struct:
  - `load_model(model_name: &str, model_path: &Path) -> Result<LoadedModel>`
  - `infer(model: &LoadedModel, input: &Tensor) -> Result<Tensor>`
- Model download from HuggingFace Hub (use `hf-hub` crate or HTTP download)
- Model caching in `~/.cache/ruddydoc/models/`
- Feature-gated: `ruddydoc-models` is optional. Without it, the pipeline uses rule-based extraction only.

#### 4.2 Layout analysis model

- Port `docling-ibm-models` layout model (ONNX version)
- Input: page image (rendered from PDF or raw image)
- Output: bounding boxes with labels (Title, Paragraph, Table, Picture, List, Formula, etc.)
- Map model labels to `rdoc:` ontology classes
- Image preprocessing: resize, normalize (port from Python's torchvision transforms)

#### 4.3 Table structure recognition model

- Port the table structure model
- Input: cropped table image (from layout analysis)
- Output: cell bounding boxes with row/column assignments
- Creates `rdoc:TableCell` nodes with correct `rdoc:cellRow`, `rdoc:cellColumn`, spans

#### 4.4 OCR integration

- Support multiple OCR backends (feature-gated):
  - **RapidOCR via ONNX**: default, embedded, cross-platform
  - **Tesseract** via `tesseract-rs`: optional, for users who prefer it
  - **macOS Vision** (on macOS): optional
- OCR pipeline:
  - Detect text regions (from layout analysis or full-page)
  - Run OCR model
  - Create `rdoc:TextElement` nodes with extracted text and bounding boxes
  - Set `rdoc:confidence` and `rdoc:detectedBy`

#### 4.5 Enhanced PDF pipeline

- `StandardPdfPipeline` combining:
  1. PDF page rendering to image
  2. Layout analysis model
  3. Table structure model (on detected tables)
  4. OCR (on text regions, or full page for scanned PDFs)
  5. Reading order determination
  6. Page assembly (merge OCR text with layout regions)
- Pipeline stages run in parallel across pages (using `rayon`)
- The pipeline REPLACES the rule-based extraction from Phase 3 when ML models are available

#### 4.6 Picture classification (optional)

- Classify detected pictures (chart, diagram, photo, logo, etc.)
- Add `rdoc:pictureCategory` property

### Phase 4 acceptance criteria

- [ ] Layout model correctly segments a multi-column PDF into elements
- [ ] Table structure model correctly identifies rows, columns, and spans
- [ ] OCR produces accurate text from scanned documents
- [ ] Standard PDF pipeline produces output comparable to Python docling
- [ ] Pipeline runs in parallel across pages
- [ ] Benchmark: PDF processing within 2x of Python docling speed (GPU not available in Rust ONNX, so CPU parity is acceptable initially)
- [ ] Feature-gated: `cargo build --no-default-features` works without ML dependencies

---

## Phase 5: PDF Positional Parsing, VLM Pipeline, Server, and CLI

**Goal**: Replace the page-level PDF text extraction with word-level positional parsing and page rendering. Add VLM (Visual Language Model) pipeline support. Build a combined HTTP REST + MCP server. Complete the CLI with all commands. Finish remaining export formats.

**Duration**: 6-8 weeks.

**Status note**: Phases 1-4 are complete (503 tests, 28K lines of Rust, 12 backends, pipeline infrastructure, ONNX model framework, export pipeline with JSON/Markdown/HTML/Text/Turtle/N-Triples export, and hierarchical chunking). Phase 5 builds on this foundation.

### What is already done (from earlier phases, now in scope for Phase 5 completion)

The following export infrastructure was delivered during Phases 1-4 and is complete:

- **JSON exporter** (docling-compatible schema)
- **Markdown exporter** (headings, lists, tables, code blocks, images)
- **HTML exporter** (semantic HTML5, accessible tables, code highlighting)
- **Text exporter** (plain text in reading order)
- **Turtle exporter** (RDF Turtle serialization)
- **N-Triples exporter** (RDF N-Triples serialization)
- **HierarchicalChunker** (structure-aware chunking for RAG)
- **Basic CLI** (`ruddydoc convert`, `ruddydoc info`, `ruddydoc formats`)

### Deliverables

#### 5.1 PDF positional parsing (ruddydoc-backend-pdf rewrite)

**Problem**: The current PDF backend uses `lopdf` for page-level text extraction, producing a single text blob per page with no positional information. This is insufficient for ML-based layout analysis, which requires word-level bounding boxes, and for page rendering to images, which is required by all ML models (layout, OCR, table structure, VLM).

**Decision**: Use `pdfium-render` (Rust bindings to Google's PDFium, the PDF engine used by Chrome) as the primary PDF library. PDFium provides the highest-quality text extraction with positions and the most reliable page rendering. The `pdfium-auto` companion crate handles automatic download and caching of the native PDFium library, eliminating manual setup.

**Rationale for PDFium over alternatives**:

| Option | Text positions | Page rendering | Cross-platform | Native dep | Status |
|--------|---------------|----------------|----------------|-----------|--------|
| `pdfium-render` | Character-level bounding boxes | High-quality RGB render | Linux, macOS, Windows | PDFium .so/.dylib/.dll (auto-downloaded) | Active, v0.9 |
| `mupdf` | Word-level | Good render | Linux, macOS, Windows | MuPDF C library (must be installed) | v0.6, less active |
| `pdf-extract` | Word-level (approximate) | No rendering | Pure Rust | None | v0.10, limited maintenance |
| `lopdf` + custom | Requires building a content stream parser | No rendering | Pure Rust | None | Active, but no positional API |

PDFium wins on text extraction quality (character-level with font metrics), rendering quality (Chrome's engine), cross-platform support (auto-download via `pdfium-auto`), and active maintenance. The native library dependency is mitigated by `pdfium-auto` which downloads the correct binary for the platform at build time or first run.

**Implementation**: Rewrite `ruddydoc-backend-pdf` to use PDFium as the primary engine while retaining `lopdf` for PDF metadata extraction (Info dictionary) which it handles well.

**New `PdfBackend` capabilities**:

```rust
/// A word extracted from a PDF page with its bounding box.
pub struct PdfWord {
    pub text: String,
    pub bbox: BoundingBox,
    pub font_name: String,
    pub font_size: f32,
    pub is_bold: bool,
    pub is_italic: bool,
}

/// A rendered page image from a PDF.
pub struct RenderedPage {
    pub page_number: u32,
    pub width: u32,
    pub height: u32,
    pub rgb_data: Vec<u8>,       // HWC layout, 3 channels
    pub scale: f32,              // render scale (e.g., 2.0 for 144 DPI)
}

/// An image embedded in a PDF page.
pub struct EmbeddedImage {
    pub page_number: u32,
    pub bbox: BoundingBox,
    pub format: String,          // "png", "jpeg", etc.
    pub data: Vec<u8>,
}

/// A hyperlink in a PDF page.
pub struct PdfLink {
    pub page_number: u32,
    pub bbox: BoundingBox,
    pub uri: String,
}
```

**PDF backend features**:

1. **Word-level text extraction with bounding boxes**: Extract every word with its precise position (left, top, right, bottom) in page coordinates. Group words into lines using vertical proximity.
2. **Page rendering to RGB images**: Render each page as an RGB image at configurable DPI (default 144 DPI, suitable for ML models). Output as `PageImage` (already defined in `ruddydoc-pipeline`). This is the critical bridge between the PDF backend and all ML model stages.
3. **Embedded image extraction**: Extract raster images embedded in the PDF with their bounding boxes and format.
4. **Font information**: Extract font name, size, bold/italic flags per word. Use font metrics as heuristic signals for heading detection, emphasis, and structure without ML.
5. **Hyperlink extraction**: Extract link annotations with their bounding boxes and target URIs. Create `rdoc:Link` elements in the graph.
6. **PDF metadata** (retained from current backend): title, author, date, subject from the Info dictionary.
7. **Rule-based heading detection**: Use font size relative to body text to heuristically classify large/bold text as headings. This provides reasonable structure even without the layout analysis model.

**Graph output changes**: Instead of creating flat `rdoc:Paragraph` nodes from page-level text blobs, the new backend creates:
- `rdoc:TextElement` nodes per paragraph (grouped from word positions using vertical gap heuristics)
- `rdoc:PictureElement` nodes for embedded images
- `rdoc:Link` nodes for hyperlinks
- Bounding boxes on all elements (`rdoc:boundingBox`)
- Font metadata as properties for downstream heading detection

**Backward compatibility**: The new backend produces the same ontology classes and properties as before. Downstream code (exporters, chunkers, pipeline stages) continues to work unchanged. The additional positional data enriches the graph without breaking existing queries.

**Acceptance criteria for 5.1**:

- [ ] Word-level text extraction with bounding boxes for standard PDFs (comparable quality to Python docling's `pypdfium2` backend)
- [ ] Page rendering produces correct RGB images at configurable DPI
- [ ] Embedded image extraction works for JPEG and PNG images in PDFs
- [ ] Font size-based heading detection classifies at least 80% of headings correctly on test documents
- [ ] All existing PDF backend tests continue to pass
- [ ] PDFium library auto-downloads on first use via `pdfium-auto` (no manual setup)
- [ ] Benchmark: text extraction + rendering for a 100-page PDF completes in <5 seconds

#### 5.2 VLM pipeline (ruddydoc-models + ruddydoc-pipeline)

**Background**: Python docling supports Visual Language Models (VLMs) as an alternative to the layout+OCR+table pipeline. A VLM takes a page image and directly produces structured document output, handling layout, OCR, and table extraction in a single model call. The primary model is SmolDocling/GraniteDocling (258M parameters, purpose-built for documents), which outputs DocTags -- a structured text format encoding document elements with bounding boxes.

**Design**: VLM support in RuddyDoc is a two-part addition:

1. A `VlmModel` trait in `ruddydoc-models` (analogous to `LayoutModel`, `OcrModel`, etc.)
2. A `VlmPipelineStage` in `ruddydoc-pipeline` that replaces the standard layout+OCR+table chain

**New trait in `ruddydoc-models/src/types.rs`**:

```rust
/// Response format from a VLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VlmResponseFormat {
    /// DocTags format (SmolDocling/GraniteDocling output).
    DocTags,
    /// Markdown (general-purpose VLMs).
    Markdown,
    /// HTML (general-purpose VLMs).
    Html,
}

/// A VLM prediction for a single page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlmPrediction {
    /// Raw text output from the model.
    pub text: String,
    /// Response format of the output.
    pub format: VlmResponseFormat,
    /// Number of tokens generated.
    pub num_tokens: u32,
    /// Model confidence (if available).
    pub confidence: Option<f32>,
}

/// Visual Language Model trait.
///
/// VLMs take a page image and produce structured document output in a
/// single call, combining layout analysis, OCR, and table extraction.
pub trait VlmModel: DocumentModel {
    /// Process a page image and produce structured text output.
    fn predict(&self, image: &ImageData, prompt: &str) -> ruddydoc_core::Result<VlmPrediction>;

    /// The response format this model produces.
    fn response_format(&self) -> VlmResponseFormat;
}
```

**New `ModelTask` variant**:

```rust
pub enum ModelTask {
    LayoutAnalysis,
    TableStructure,
    Ocr,
    PictureClassification,
    Vlm,  // new
}
```

**VLM invocation backends** (feature-gated):

| Backend | Feature flag | Use case |
|---------|-------------|----------|
| Local ONNX | `onnx` (existing) | Small models like GraniteDocling-258M via ONNX Runtime |
| HTTP API | `vlm-api` | Remote model servers (vLLM, TGI, KServe) using OpenAI-compatible chat/completions API |

**HTTP API VLM implementation** (in `ruddydoc-models`, behind `vlm-api` feature):

```rust
/// Options for calling a VLM via HTTP API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiVlmOptions {
    /// API endpoint URL (e.g., "http://localhost:8000/v1/chat/completions").
    pub url: String,
    /// API key (optional, for cloud-hosted models).
    pub api_key: Option<String>,
    /// Model name (sent in the API request body).
    pub model_name: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Temperature for generation.
    pub temperature: f32,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
    /// The expected response format.
    pub response_format: VlmResponseFormat,
}

/// VLM that calls an OpenAI-compatible API.
pub struct ApiVlmModel {
    options: ApiVlmOptions,
    client: reqwest::Client,
}
```

The HTTP API VLM sends the page image as a base64-encoded data URL in an OpenAI-compatible chat/completions request. This is compatible with:
- vLLM serving framework
- Hugging Face TGI (Text Generation Inference)
- OpenAI API (GPT-4V, etc.)
- Anthropic API (Claude with vision)
- Any OpenAI-compatible endpoint

**VLM pipeline stage** (in `ruddydoc-pipeline`):

```rust
/// Pipeline stage that uses a VLM for full-page document understanding.
///
/// This stage replaces the standard layout+OCR+table pipeline chain.
/// For each page image, it calls the VLM which returns structured output
/// (DocTags, Markdown, or HTML). The output is then parsed into RDF
/// triples in the document graph.
pub struct VlmPipelineStage {
    model: Box<dyn VlmModel>,
}
```

**DocTags parser**: A new module in `ruddydoc-pipeline` that parses DocTags output (a structured text format with XML-like tags and bounding box annotations) into RDF triples. This is the same format used by SmolDocling and GraniteDocling.

**New pipeline factory method**:

```rust
impl Pipeline {
    /// VLM pipeline: single-stage processing using a visual language model.
    /// Replaces the standard layout+OCR+table chain.
    pub fn vlm(model: Box<dyn VlmModel>) -> Self {
        Pipeline::new()
            .add_stage(Box::new(VlmPipelineStage::new(model)))
            .add_stage(Box::new(ReadingOrderStage))
            .add_stage(Box::new(ProvenanceStage))
    }
}
```

**Acceptance criteria for 5.2**:

- [ ] `VlmModel` trait compiles and is usable from downstream crates
- [ ] `ApiVlmModel` can call an OpenAI-compatible endpoint and return a `VlmPrediction`
- [ ] `VlmPipelineStage` converts VLM output (Markdown format) into RDF triples in the graph
- [ ] DocTags parser handles the core DocTags elements (title, section-header, text, table, figure, caption, list-item, formula, page-header, page-footer)
- [ ] `Pipeline::vlm()` factory produces a working pipeline when connected to a model
- [ ] VLM stage is feature-gated: `cargo build --no-default-features` works without VLM dependencies
- [ ] Integration test: mock HTTP VLM endpoint -> VlmPipelineStage -> correct graph output

#### 5.3 Combined HTTP + MCP server (ruddydoc-server)

**Background**: Python docling provides a separate `docling-mcp` package for AI agent integration. RuddyDoc consolidates this into a single `ruddydoc-server` crate that provides both an MCP server (for AI agent integration via Claude, LM Studio, etc.) and an HTTP REST API (for programmatic access from any language).

**Architecture**: The server crate follows beret's proven MCP server pattern (using `rust-mcp-sdk`) while adding `axum` for HTTP REST endpoints. Both the MCP tools and REST endpoints share the same core logic through a `ServerState` struct that holds the document store and converter.

```rust
/// Shared server state. Holds the document store (in-memory Oxigraph)
/// and the converter. Documents persist for the server's lifetime.
pub struct ServerState {
    /// In-memory document store. All converted documents live here.
    store: Arc<OxigraphStore>,
    /// Document converter for processing uploaded files.
    converter: DocumentConverter,
    /// Map from document ID to its named graph IRI.
    documents: DashMap<String, DocumentRecord>,
}

/// Metadata about a converted document stored in the server.
pub struct DocumentRecord {
    /// The named graph IRI for this document.
    pub graph_iri: String,
    /// Document metadata (format, hash, page count, etc.).
    pub meta: DocumentMeta,
    /// When the document was converted.
    pub converted_at: std::time::Instant,
}
```

**MCP Tools** (following beret's pattern and Python docling-mcp's tool set):

| Tool | Description | Required args | Optional args |
|------|------------|---------------|---------------|
| `convert_document` | Convert a file path or URL, return document ID | `source` (path or URL) | `format` (force input format) |
| `query_document` | Run SPARQL on a converted document | `document_id`, `sparql` | `limit`, `offset` |
| `export_document` | Export in a given format | `document_id` | `format` (default: json) |
| `list_elements` | List document elements by type | `document_id` | `element_type`, `limit`, `offset` |
| `get_element` | Get a specific element's details | `document_id`, `element_id` | |
| `chunk_document` | Chunk for RAG | `document_id` | `max_tokens`, `include_headings` |
| `list_documents` | List all converted documents | | |
| `list_formats` | List supported input/output formats | | |

**REST API endpoints**:

```
POST   /convert            Upload file, get conversion result (document ID + metadata)
GET    /documents           List all converted documents
GET    /documents/{id}      Get document metadata
GET    /documents/{id}/export?format=json   Export document in given format
POST   /documents/{id}/query                Run SPARQL query (body: {"sparql": "..."})
GET    /documents/{id}/elements?type=Paragraph&limit=50   List elements
GET    /documents/{id}/elements/{eid}       Get specific element
GET    /documents/{id}/chunks?max_tokens=512              Get RAG chunks
GET    /formats             List supported formats
GET    /health              Health check (returns 200 + version)
```

**Server modes** (CLI integration):

```
ruddydoc serve --mcp                    MCP server over stdio (for Claude Desktop, LM Studio)
ruddydoc serve --http --port 8080       HTTP REST server only
ruddydoc serve --port 8080              HTTP + MCP/SSE on same port (default mode)
```

Stdio MCP mode uses `rust_mcp_sdk::StdioTransport`. HTTP mode uses `axum` with `rust_mcp_sdk` HTTP/SSE support. The combined mode mounts the MCP SSE endpoint at `/mcp/sse` alongside the REST routes.

**MCP server configuration for Claude Desktop / LM Studio**:

```json
{
  "mcpServers": {
    "ruddydoc": {
      "command": "ruddydoc",
      "args": ["serve", "--mcp"]
    }
  }
}
```

**Implementation structure** (in `crates/ruddydoc-server/src/`):

```
lib.rs          ServerState, shared types, server construction
mcp.rs          MCP handler (implements ServerHandler trait from rust-mcp-sdk)
rest.rs         Axum routes and handlers
tools.rs        MCP tool definitions (following beret's pattern)
state.rs        Document store management, document lifecycle
```

**Acceptance criteria for 5.3**:

- [ ] `ruddydoc serve --mcp` starts an MCP server that handles all 8 tools
- [ ] `ruddydoc serve --http --port 8080` starts an HTTP server with all REST endpoints
- [ ] `ruddydoc serve --port 8080` starts combined HTTP + MCP/SSE server
- [ ] MCP `convert_document` accepts a local file path and returns a document ID
- [ ] MCP `query_document` runs a SPARQL query and returns JSON results
- [ ] MCP `chunk_document` returns chunks suitable for RAG
- [ ] REST `POST /convert` accepts multipart file upload
- [ ] REST `GET /health` returns 200 with version info
- [ ] Server starts in <500ms (cold start, no documents loaded)
- [ ] Integration test: MCP client connects via stdio, converts a file, queries it
- [ ] Integration test: HTTP client uploads a file via REST, exports as JSON
- [ ] CORS headers configured for REST endpoints (allowing browser clients)

#### 5.4 Remaining export formats

Complete the export formats not yet implemented. These are lower priority than 5.1-5.3 and can be partially deferred to Phase 6 if needed.

- **JSON-LD exporter**: schema.org-compatible linked data
  - Document metadata as schema:CreativeWork
  - Structural elements with appropriate schema.org types
  - Proper @context with prefix mapping (`rdoc:`, `schema:`, `dcterms:`)
  - Implementation: query the graph and produce JSON-LD using `serde_json` (no additional dependency)
- **RDF/XML exporter**: standard W3C RDF/XML serialization
  - Implementation: use Oxigraph's built-in RDF/XML serializer via `store.serialize_graph()`
- **DocTags exporter**: reproduce docling's DocTags format for compatibility
  - Needed for VLM output round-tripping and docling interop
- **WebVTT exporter**: for audio/video documents with timestamps (low priority)

**Acceptance criteria for 5.4**:

- [ ] JSON-LD export produces valid JSON-LD with correct @context
- [ ] RDF/XML export produces valid RDF/XML
- [ ] DocTags export matches Python docling's DocTags format for common element types
- [ ] `exporter_for()` returns correct exporter for all `OutputFormat` variants

#### 5.5 Full CLI

Extend the existing CLI (`ruddydoc convert`, `ruddydoc info`, `ruddydoc formats`) with the remaining commands.

**New commands**:

```
ruddydoc convert <input>... [--output <dir>] [--format <fmt>]   (enhance: batch, all formats)
ruddydoc query <sparql> <files...>                               Run SPARQL queries on documents
ruddydoc chunk <files...> [--max-tokens N] [--include-headings]  Chunk documents for RAG
ruddydoc serve [--mcp] [--http] [--port PORT]                    Start server
ruddydoc models list                                             List available/cached ML models
ruddydoc models download <model>                                 Download model from HuggingFace
```

**Enhanced `convert` command**:

- Support all output formats: `--format json|markdown|html|text|turtle|ntriples|jsonld|rdfxml|doctags`
- Batch conversion: `ruddydoc convert *.pdf --output ./converted/ --format markdown`
- Progress bars for batch conversion (using `indicatif`)
- JSON output mode (`--json`) for machine-readable status
- Pipeline selection: `--pipeline standard|vlm|simple` (default: standard when models available, simple otherwise)
- VLM options: `--vlm-url <url>` to use a remote VLM server

**CLI output format argument** (update `OutputFormatArg` enum):

```rust
#[derive(Debug, Clone, ValueEnum)]
enum OutputFormatArg {
    Json,
    Markdown,
    Html,
    Text,
    Turtle,
    Ntriples,
    Jsonld,
    Rdfxml,
    Doctags,
}
```

**Features**:

- Configurable logging: `--verbose` (debug), `--quiet` (errors only), default (info)
- Tab completion generation: `ruddydoc completions bash|zsh|fish`
- `--version` shows version, build info, and available features (models, server)

**Acceptance criteria for 5.5**:

- [ ] `ruddydoc convert` supports all output formats via `--format`
- [ ] `ruddydoc query "SELECT ?e WHERE { ?e a <rdoc:Paragraph> }" test.pdf` returns correct results
- [ ] `ruddydoc chunk test.md --max-tokens 256` produces correctly sized chunks
- [ ] `ruddydoc serve --mcp` starts MCP server (delegates to ruddydoc-server)
- [ ] `ruddydoc models list` shows available models with their status (cached/not cached)
- [ ] Batch conversion with progress bars works for 50+ files
- [ ] `ruddydoc completions bash` produces valid bash completion script

### Phase 5 parallel work tracks

Phase 5 is structured for maximum parallelism:

**Track A (PDF)**: 5.1 (PDF positional parsing) -- standalone, no dependencies on other 5.x items.

**Track B (Server)**: 5.3 (server) -- depends on existing converter and export infrastructure (already done). Can start immediately.

**Track C (VLM)**: 5.2 (VLM pipeline) -- depends on 5.1 (needs page rendering for VLM input). The trait and API VLM model can start immediately; the pipeline integration waits for 5.1.

**Track D (CLI + Export)**: 5.4 (remaining exports) + 5.5 (CLI commands) -- largely independent. CLI `serve` subcommand depends on 5.3.

```
Week 1-2:  5.1 (PDF backend)    | 5.3 (MCP server)     | 5.2 traits + API model | 5.4 exports
Week 3-4:  5.1 (PDF rendering)  | 5.3 (REST API)       | 5.2 DocTags parser     | 5.5 CLI
Week 5-6:  5.1 finalize         | 5.3 finalize         | 5.2 VLM stage + tests  | 5.5 finalize
Week 7-8:  Integration testing across all tracks, bug fixes, polish
```

### Phase 5 acceptance criteria (aggregate)

- [ ] PDF text extraction produces word-level bounding boxes
- [ ] PDF pages render to RGB images suitable for ML model input
- [ ] VLM trait and API model are implemented and feature-gated
- [ ] VLM pipeline stage converts model output to RDF triples
- [ ] MCP server handles all 8 tools and passes integration tests
- [ ] HTTP REST server handles file upload, export, query, and chunking
- [ ] Combined HTTP+MCP server starts and handles both protocols
- [ ] All export formats (JSON, Markdown, HTML, Text, Turtle, N-Triples, JSON-LD, RDF/XML, DocTags) produce correct output
- [ ] CLI supports all commands: convert (batch, all formats), query, chunk, serve, models
- [ ] Server cold start in <500ms
- [ ] PDF positional extraction for 100-page PDF in <5 seconds

---

## Phase 6: Performance and Parity

**Goal**: Beat Python docling in every benchmark. Full compatibility with docling's test suite. Distribution (binary releases, cargo install, npm wrapper). Complete any remaining Phase 5 items that were deferred.

**Duration**: 3-4 weeks.

### Deliverables

#### 6.1 Performance benchmarking

- Criterion benchmarks comparing RuddyDoc vs Python docling:
  - **Startup time**: `time ruddydoc --version` vs `time python -c "import docling"`
  - **Single file**: parse a representative file of each format, measure time and memory
  - **Batch**: parse 100 mixed-format files, measure total time and peak memory
  - **Large PDF**: parse a 100-page PDF with tables and images (now with positional extraction)
  - **PDF rendering**: render 100 pages to images, compare with Python's `pypdfium2`
  - **VLM pipeline**: end-to-end VLM pipeline throughput (pages/sec) vs Python docling
  - **Server latency**: REST API and MCP tool response times under load
  - **SPARQL query**: query a document graph with various complexity queries
- Performance targets:
  - 10x faster startup
  - 5x faster single-file conversion (text formats)
  - 3x faster PDF conversion (with ML models)
  - 10x lower peak memory for batch conversion
  - SPARQL queries < 10ms
  - Server: <50ms response time for `GET /health`, <500ms for single-file conversion via REST

#### 6.2 Compatibility testing

- Port Python docling's test suite:
  - Parse each test fixture
  - Compare output against Python docling's expected output
  - Accept structural equivalence (not byte-for-byte identical)
- Compatibility modes:
  - `--compat docling` flag to produce byte-for-byte identical JSON output
  - Without flag, use RuddyDoc's enhanced JSON schema (with graph metadata)

#### 6.3 Distribution

- GitHub Actions CI:
  - Build for: Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, Windows x86_64
  - Run test suite on each platform
  - Publish binary releases
- `cargo install ruddydoc`
- npm wrapper package (following beret's pattern): `npx @chapeaux/ruddydoc`
- Docker image: `docker run ghcr.io/chapeaux/ruddydoc convert ...`
- Homebrew formula

#### 6.4 Documentation

- README with quick start
- CLI reference (generated from clap)
- Ontology reference (generated from .ttl files)
- SPARQL query cookbook
- Migration guide from Python docling
- API docs (rustdoc)

### Phase 6 acceptance criteria

- [ ] All benchmarks beat Python docling
- [ ] 95%+ of Python docling test fixtures produce equivalent output
- [ ] Binary releases available for all target platforms
- [ ] `cargo install ruddydoc` works
- [ ] npm wrapper works
- [ ] Docker image works

---

## Team Assignments

### Phase 1 assignments

| Work Item | Primary | Validators | Dependencies |
|-----------|---------|------------|-------------|
| 1.1 Workspace + ruddydoc-core types | **Architect** (design) -> **Rust Engineer** (implement) | Architect, QA | None |
| 1.2 DocumentStore (Oxigraph wrapper) | **Rust Engineer** | Ontologist, QA | 1.1 |
| 1.3 Document ontology (ruddydoc.ttl, shapes.ttl) | **Ontologist** | Compliance, QA | None (parallel with 1.1) |
| 1.3b Ontology crate | **Rust Engineer** | Ontologist, QA | 1.2, 1.3 |
| 1.4 Backend trait + Markdown backend | **Rust Engineer** | Architect (trait design), QA | 1.1, 1.2 |
| 1.5 JSON exporter | **Rust Engineer** | QA | 1.2, 1.4 |
| 1.6 Minimal CLI | **Rust Engineer** | Designer (UX review), QA | 1.4, 1.5 |
| CI setup | **DevOps** | QA | 1.1 |
| License audit | **Legal** | Compliance | 1.1 (once dependencies are chosen) |

### Parallel work tracks

Phase 1 has two parallel tracks:

**Track A (Rust)**: 1.1 -> 1.2 -> 1.4 -> 1.5 -> 1.6

**Track B (Ontology)**: 1.3 (runs in parallel with Track A, then 1.3b merges them)

**Track C (Infrastructure)**: CI setup + license audit (runs in parallel)

### Phase 5 assignments

| Work Item | Primary | Validators | Dependencies |
|-----------|---------|------------|-------------|
| 5.1 PDF positional parsing (PDFium integration) | **Rust Engineer** | Architect (API review), QA | Phase 3 PDF backend (done) |
| 5.1 Page rendering to images | **Rust Engineer** | QA (visual comparison) | 5.1 PDFium integration |
| 5.1 Font-based heading detection | **Rust Engineer** | QA | 5.1 word extraction |
| 5.2 VlmModel trait + ModelTask::Vlm | **Architect** (design) -> **Rust Engineer** (implement) | Architect, QA | Phase 4 model infrastructure (done) |
| 5.2 ApiVlmModel (HTTP API) | **Rust Engineer** | Architect, QA | 5.2 trait |
| 5.2 DocTags parser | **Rust Engineer** | QA | None (standalone parser) |
| 5.2 VlmPipelineStage | **Rust Engineer** | Architect, QA | 5.2 trait, 5.1 page rendering |
| 5.3 MCP server (tools, handler) | **Rust Engineer** | Architect (MCP protocol), QA | Converter + export (done) |
| 5.3 REST API (axum routes) | **Rust Engineer** | Designer (API design review), QA | 5.3 ServerState |
| 5.3 Combined HTTP+MCP mode | **Rust Engineer** | QA | 5.3 MCP + REST |
| 5.4 JSON-LD exporter | **Rust Engineer** | Ontologist (schema.org mapping), QA | Export infrastructure (done) |
| 5.4 RDF/XML exporter | **Rust Engineer** | QA | Export infrastructure (done) |
| 5.4 DocTags exporter | **Rust Engineer** | QA | 5.2 DocTags parser |
| 5.5 CLI: query, chunk, serve, models | **Rust Engineer** | Designer (UX), QA | 5.3 server, export (done) |
| 5.5 CLI: batch convert + progress | **Rust Engineer** | Designer (UX), QA | Converter (done) |

### Later phase assignments (high-level)

- **Phase 2**: Rust Engineer (backends), Architect (reviews), QA (testing)
- **Phase 3**: Rust Engineer (backends), Architect (PDF design review), QA
- **Phase 4**: Rust Engineer (ONNX integration), Architect (pipeline design), QA (accuracy testing)
- **Phase 5**: Rust Engineer (PDF, VLM, server, CLI), Architect (API design, MCP protocol), Designer (CLI UX, REST API), Ontologist (JSON-LD), QA
- **Phase 6**: QA (benchmarks, compatibility), DevOps (distribution), Rust Engineer (performance optimization)

---

## Decision Log

| # | Decision | Rationale | Date |
|---|----------|-----------|------|
| 1 | Use Oxigraph as the document representation | Enables SPARQL queries, RDF export, and AI agent integration. Aligned with beret's proven pattern. Non-negotiable per project charter. | 2026-04-07 |
| 2 | Rust edition 2024 | Following beret's convention. | 2026-04-07 |
| 3 | ONNX Runtime instead of PyTorch | Eliminates 2GB Python ML dependency. ONNX models are portable and fast for inference-only use. | 2026-04-07 |
| 4 | Feature-gate ML models | Users who only need text-format parsing should not need ONNX Runtime. `ruddydoc-models` is optional. | 2026-04-07 |
| 5 | One crate per backend format | Keeps compilation fast (only recompile the backend that changed). Allows users to select which formats to include. | 2026-04-07 |
| 6 | Named graphs per document | Enables multi-document queries while keeping document isolation. Standard RDF practice. | 2026-04-07 |
| 7 | JSON export compatible with docling schema | Enables drop-in replacement for existing docling users. Migration path is critical for adoption. | 2026-04-07 |
| 8 | clap for CLI (not typer-like custom) | Following Rust conventions. clap derive gives completion, help, and version for free. | 2026-04-07 |
| 9 | `pulldown-cmark` for Markdown | Most mature Rust Markdown parser. GFM support. Used by mdBook and other Rust tools. | 2026-04-07 |
| 10 | `calamine` for Excel | Best Rust crate for reading XLSX/XLS. Used widely. | 2026-04-07 |
| 11 | Users never see RDF | Core UX principle. Applies to error messages, CLI output, and documentation. SPARQL is opt-in for power users. | 2026-04-07 |
| 12 | `pdfium-render` for PDF text extraction and rendering | PDFium (Chrome's PDF engine) provides the highest-quality character-level text extraction with bounding boxes and page rendering. `pdfium-auto` auto-downloads the native library. Retain `lopdf` for metadata only. Alternatives considered: `mupdf` (less active bindings, manual native lib install), `pdf-extract` (no rendering), custom `lopdf` content stream parser (enormous effort). | 2026-04-07 |
| 13 | `axum` for HTTP REST server | Lightweight, tokio-native, composable. Consistent with Rust ecosystem conventions. tower middleware for CORS, tracing. | 2026-04-07 |
| 14 | `rust-mcp-sdk` for MCP server | Following beret's proven pattern. Supports both stdio and HTTP/SSE transport. Same version (0.9) as beret. | 2026-04-07 |
| 15 | Combined HTTP+MCP in single `ruddydoc-server` crate | Avoids the Python docling pattern of separate `docling-mcp` package. Single crate reduces maintenance and ensures tools and REST endpoints share the same state. Server crate is feature-gated in CLI to keep minimal builds fast. | 2026-04-07 |
| 16 | VLM support via trait + HTTP API (not local-first) | ONNX Runtime VLM inference for a 258M parameter model is feasible but complex (requires tokenizer, beam search, autoregressive generation). HTTP API support is simpler, more flexible (works with any model server), and more immediately useful. Local ONNX VLM is a future enhancement, not a Phase 5 requirement. | 2026-04-07 |
| 17 | Feature-gate VLM and server | `vlm-api` feature enables HTTP VLM support (adds `reqwest` dependency). `server` feature enables `ruddydoc-server` (adds `axum`, `rust-mcp-sdk`, `tower`). Default features include neither. This keeps the base binary small for users who only need CLI conversion. | 2026-04-07 |

---

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| ONNX model accuracy differs from PyTorch | Medium | High | Compare outputs systematically. Accept minor differences if within tolerance. Fall back to Python docling for accuracy-critical workflows initially. |
| PDF text extraction quality without layout model | High | Medium | Phase 3 PDF backend is explicitly "text extraction only". Phase 4 adds ML. Set expectations. |
| Oxigraph performance with large documents (>10K elements) | Low | Medium | Benchmark early. Named graphs keep individual document queries fast. Use indexed SPARQL patterns. |
| DOCX list detection complexity | High | Low | OOXML numbering is notoriously complex. Accept 90% accuracy initially, iterate. |
| LaTeX macro expansion completeness | High | Medium | Support common macros only. Document limitations. Accept that some LaTeX documents will not parse perfectly. |
| Binary size with all backends + ONNX | Medium | Low | Feature gates. Minimal binary excludes ML and rarely-used backends. |
| PDFium native library distribution | Medium | Medium | `pdfium-auto` handles download and caching, but adds ~20MB native library. CI must cache it. Docker images include it. Fallback: retain lopdf-only mode behind feature flag for environments that cannot download native libs. |
| PDFium rendering quality differs from pypdfium2 (Python) | Low | Low | Both use the same underlying PDFium library. Rust bindings may have minor differences in default render settings (DPI, antialiasing). Validate with visual comparison tests. |
| VLM HTTP API latency dominates pipeline time | High | Low | Expected: VLM inference is inherently slower than rule-based extraction. This is a user choice (accuracy vs speed). Document the tradeoff. Pipeline selection (`--pipeline standard|vlm`) gives users control. |
| VLM DocTags format changes between model versions | Medium | Medium | Pin to a specific DocTags schema version. Add version negotiation in the parser. Test against multiple SmolDocling model versions. |
| MCP protocol compatibility across client versions | Medium | Medium | Use `rust-mcp-sdk` which tracks the spec. Pin `ProtocolVersion::V2025_11_25` (matching beret). Test with Claude Desktop and LM Studio. |
| Server memory growth with many converted documents | Medium | Medium | Documents persist in-memory Oxigraph for the server's lifetime. Add `DELETE /documents/{id}` endpoint and `forget_document` MCP tool. Monitor memory in benchmarks. Consider LRU eviction for long-running servers. |
| axum + rust-mcp-sdk integration complexity | Low | Medium | Both are tokio-native. beret demonstrates rust-mcp-sdk HTTP/SSE mode works. The REST API (axum) and MCP/SSE (rust-mcp-sdk) run on the same tokio runtime but separate listeners. |
