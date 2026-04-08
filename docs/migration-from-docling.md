# Migrating from Python docling

This guide helps Python docling users transition to RuddyDoc.

## Why migrate?

- **10x faster** for text-based formats (Markdown, HTML, CSV)
- **3x faster** for PDFs with ML models
- **Zero deployment overhead**: single binary, no Python runtime or virtualenv
- **Lower memory usage**: 10x reduction in peak memory for batch processing
- **Embedded knowledge graph**: query documents with SPARQL without external databases
- **RDF export formats**: Turtle, N-Triples, JSON-LD, RDF/XML

## CLI comparison

### Installation

| Python docling | RuddyDoc |
|----------------|----------|
| `pip install docling` | `cargo install ruddydoc` |
| Requires Python 3.10+ | No runtime dependencies |
| ~2GB with ML models | ~50MB binary |

### Basic conversion

| Python docling | RuddyDoc | Notes |
|----------------|----------|-------|
| `docling document.pdf` | `ruddydoc convert document.pdf` | Same behavior (defaults to JSON) |
| `docling --to markdown document.pdf` | `ruddydoc convert document.pdf --format markdown` | Equivalent |
| `docling --to json document.pdf` | `ruddydoc convert document.pdf --format json` | JSON output is structurally compatible |
| `docling --output ./out/ *.pdf` | `ruddydoc convert *.pdf --output ./out/` | Batch conversion |

### Format selection

| Python docling | RuddyDoc | Notes |
|----------------|----------|-------|
| `docling --to html` | `ruddydoc convert --format html` | |
| `docling --to text` | `ruddydoc convert --format text` | |
| `docling --to doctags` | `ruddydoc convert --format doctags` | |
| N/A | `ruddydoc convert --format turtle` | RDF export (new in RuddyDoc) |
| N/A | `ruddydoc convert --format jsonld` | JSON-LD export (new) |

### Input format filtering

| Python docling | RuddyDoc | Notes |
|----------------|----------|-------|
| `docling --from pdf` | `ruddydoc convert --from pdf` | Restrict to specific input formats |
| `docling --from docx,xlsx` | `ruddydoc convert --from docx --from xlsx` | Multiple format flags |

### OCR and table options

| Python docling | RuddyDoc | Notes |
|----------------|----------|-------|
| `docling --ocr` | `ruddydoc convert` (default) | OCR enabled by default when models available |
| `docling --no-ocr` | `ruddydoc convert --no-ocr` | Disable OCR |
| `docling --force-ocr` | Not needed | RuddyDoc auto-detects scanned PDFs |
| `docling --tables` | `ruddydoc convert` (default) | Table structure detection enabled by default |
| `docling --no-tables` | `ruddydoc convert --no-tables` | Disable table structure |

### Verbosity and debugging

| Python docling | RuddyDoc | Notes |
|----------------|----------|-------|
| `docling -v` | `ruddydoc --verbose convert` | INFO-level logging |
| `docling -vv` | `ruddydoc -vv convert` | DEBUG-level logging |
| `docling --debug-visualize-layout` | Not available | Use `--verbose` for processing details |

### Version information

| Python docling | RuddyDoc | Notes |
|----------------|----------|-------|
| `docling --version` | `ruddydoc --version` | Shows version and build info |

## Python API comparison

### Basic conversion

**Python docling:**

```python
from docling.document_converter import DocumentConverter

converter = DocumentConverter()
result = converter.convert("document.pdf")
markdown = result.document.export_to_markdown()
json_dict = result.document.export_to_dict()
```

**RuddyDoc:**

RuddyDoc is a CLI-first tool. For programmatic use, call the CLI from your language of choice:

```python
import subprocess
import json

# Convert to Markdown
result = subprocess.run(
    ["ruddydoc", "convert", "document.pdf", "--format", "markdown"],
    capture_output=True,
    text=True
)
markdown = result.stdout

# Convert to JSON
result = subprocess.run(
    ["ruddydoc", "convert", "document.pdf", "--format", "json"],
    capture_output=True,
    text=True
)
json_dict = json.loads(result.stdout)
```

Or use the REST API (see "Server mode" below).

### Batch conversion

**Python docling:**

```python
from docling.document_converter import DocumentConverter

converter = DocumentConverter()
results = converter.convert_all(["doc1.pdf", "doc2.pdf"])
for result in results:
    print(result.document.export_to_markdown())
```

**RuddyDoc:**

```bash
ruddydoc convert doc1.pdf doc2.pdf --format markdown
```

Or from Python:

```python
subprocess.run([
    "ruddydoc", "convert", "doc1.pdf", "doc2.pdf",
    "--format", "markdown", "--output", "./output/"
])
```

## JSON output compatibility

RuddyDoc's JSON output is **structurally compatible** with docling's `DoclingDocument` schema.

### Structure

Both use the same top-level structure:

```json
{
  "name": "document.pdf",
  "texts": [...],
  "tables": [...],
  "pictures": [...],
  "body": {...}
}
```

### Differences

| Field | Python docling | RuddyDoc | Notes |
|-------|----------------|----------|-------|
| `texts` | Array of text elements | Same | Element types: `title`, `section-header`, `paragraph`, `list-item`, etc. |
| `tables` | Array of table objects | Same | Includes `cells` with row/col/span |
| `pictures` | Array of picture objects | Same | Includes format, dimensions |
| `body` | Tree structure | Same | Hierarchical document body |
| `metadata` | Document metadata | Extended | RuddyDoc adds `graph_iri`, `triple_count`, `provenance` |

### Migration strategy

1. **Drop-in replacement**: For most use cases, RuddyDoc JSON works with existing docling-based code
2. **Test thoroughly**: Validate output format with your downstream pipeline
3. **Use compatibility mode**: Future RuddyDoc versions may add a `--compat docling` flag for byte-for-byte identical output

## New features in RuddyDoc

### SPARQL queries

Query the embedded knowledge graph directly:

```bash
# Find all section headings
ruddydoc query 'SELECT ?text WHERE {
  ?h a <https://ruddydoc.chapeaux.io/ontology#SectionHeader> ;
     rdoc:textContent ?text .
}' document.pdf

# Count tables
ruddydoc query 'SELECT (COUNT(?t) AS ?count) WHERE {
  ?t a <https://ruddydoc.chapeaux.io/ontology#TableElement> .
}' document.pdf
```

No Python API required — pure command-line operation.

### RDF export

Export to semantic web formats:

```bash
# Export as RDF Turtle
ruddydoc convert document.pdf --format turtle > document.ttl

# Export as JSON-LD
ruddydoc convert document.pdf --format jsonld > document.jsonld

# Export as N-Triples
ruddydoc convert document.pdf --format ntriples > document.nt
```

Use cases:
- Load into triple stores (Blazegraph, Virtuoso, Apache Jena)
- Semantic search with SPARQL endpoints
- Knowledge graph integration

### Document chunking

Built-in chunking for RAG workflows:

```bash
ruddydoc chunk document.pdf --max-tokens 512 > chunks.json
```

Output is ready for vector embedding and retrieval:

```json
{
  "chunks": [
    {
      "id": "chunk-0",
      "text": "...",
      "tokens": 487,
      "metadata": {
        "source": "document.pdf",
        "page": 1,
        "section": "Introduction"
      }
    }
  ]
}
```

### MCP server for AI agents

Integrate with Claude Desktop, LM Studio, and other MCP clients:

**Start the server:**

```bash
ruddydoc serve
```

**Add to Claude Desktop config** (`~/Library/Application Support/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "ruddydoc": {
      "command": "ruddydoc",
      "args": ["serve"]
    }
  }
}
```

Claude can now convert documents, query graphs, and chunk content via MCP tools.

### Metadata inspection

Quickly inspect document properties without full conversion:

```bash
ruddydoc info document.pdf
```

Output:

```
document.pdf
  Format: PDF
  Pages: 42
  Size: 2.3 MB
  Title: Annual Report 2025
  Author: Acme Corp
```

## Not yet available in RuddyDoc

The following Python docling features are not yet implemented (as of RuddyDoc 0.1.0):

1. **VLM pipeline**: Visual Language Model support (planned for future release)
2. **ASR pipeline**: Audio processing with speech recognition (planned)
3. **Enrichment flags**: `--enrich-code`, `--enrich-formula`, `--enrich-picture-description` (may be added)
4. **HTTP input**: URL/remote file support (may be added)
5. **External plugins**: Python docling supports plugins; RuddyDoc backends are built-in only

If you need these features, continue using Python docling or track RuddyDoc's [roadmap](https://github.com/chapeaux/ruddydoc/issues).

## Performance expectations

### Text-based formats (Markdown, HTML, CSV)

- **10x faster** conversion
- **Instant startup** (<100ms vs ~2s for Python import)
- **Lower memory**: 10x reduction for batch jobs

### PDFs with ML models

- **3x faster** with layout analysis and table structure models
- **Similar accuracy** (same underlying models via ONNX Runtime)

### Large batch jobs

- **Parallel processing** enabled by default (use `--parallel N` to customize)
- **Progress bars** for visual feedback
- **Streaming output** (no need to wait for all files to finish)

## Server mode

RuddyDoc includes a REST API server (not available in Python docling as of 2.85.0):

```bash
ruddydoc serve --port 8080
```

Endpoints:

- `POST /convert` — upload file, get conversion result
- `GET /documents` — list all converted documents
- `GET /documents/{id}/export?format=json` — export document
- `POST /documents/{id}/query` — run SPARQL query
- `GET /health` — health check

Example (cURL):

```bash
# Convert a file
curl -F "file=@document.pdf" http://localhost:8080/convert

# Query the document
curl -X POST http://localhost:8080/documents/{id}/query \
  -H "Content-Type: application/json" \
  -d '{"sparql": "SELECT ?h WHERE { ?h a <rdoc:SectionHeader> }"}'
```

This enables integration from any language without CLI subprocess calls.

## Migration checklist

- [ ] Install RuddyDoc binary or cargo package
- [ ] Test conversion on a sample document: `ruddydoc convert sample.pdf --format json`
- [ ] Compare output with Python docling JSON (should be structurally compatible)
- [ ] Update CI/CD scripts to use `ruddydoc convert` instead of `docling`
- [ ] Verify downstream consumers can parse RuddyDoc JSON output
- [ ] (Optional) Explore SPARQL queries for advanced document analysis
- [ ] (Optional) Use `ruddydoc chunk` for RAG workflows
- [ ] (Optional) Set up MCP server for AI agent integration

## Getting help

- **Documentation**: [RuddyDoc docs](https://github.com/chapeaux/ruddydoc)
- **Issues**: [GitHub Issues](https://github.com/chapeaux/ruddydoc/issues)
- **Discussions**: [GitHub Discussions](https://github.com/chapeaux/ruddydoc/discussions)
- **Python docling**: Continue using [docling](https://github.com/docling-project/docling) for features not yet in RuddyDoc
