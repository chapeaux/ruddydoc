# RuddyDoc CLI UX Specification

**Version**: 1.0  
**Last updated**: 2026-04-07  
**Owner**: Designer  
**Status**: Draft for Phase 5

## Purpose

This document defines the command-line interface user experience for RuddyDoc. It specifies commands, output formats, error messages, exit codes, and conventions that engineers must follow when implementing the CLI.

## Design Principles

1. **Users should never need to know RDF.** Avoid "IRI", "triple", "named graph", "SHACL violation", "SPARQL" in default output.
2. **Progressive disclosure**: Minimal output by default. `--verbose` for details. `--quiet` for silence.
3. **Every error answers three questions**: What happened? Why? How do I fix it?
4. **Color aids comprehension but is never the only signal.** Use symbols and structure for accessibility.
5. **Long operations (>1s) show progress.** Use progress bars for batch conversion.
6. **Structured output for machines.** `--json` mode for parseable output.

---

## Command Structure

### Global Options

Available for all commands:

- `--version, -V`: Show version and dependency information, then exit.
- `--help, -h`: Show help for the current command.
- `--verbose, -v`: Enable info logging. `-vv` for debug logging.
- `--quiet, -q`: Suppress all non-error output.
- `--json`: Output machine-readable JSON instead of human-friendly text.

**Phase**: All Phase 1+

---

## Commands

### 1. `convert` (Phase 1+)

**Purpose**: Convert documents to specified output format(s).

**Usage**:
```
ruddydoc convert [OPTIONS] <INPUT>...
```

**Arguments**:
- `<INPUT>...`: One or more input files or directories. Directories are scanned recursively for supported formats.

**Options**:
- `--output, -o <PATH>`: Output directory. Defaults to current directory.
- `--format, -f <FORMAT>`: Output format. Defaults to `markdown`. Can be specified multiple times.
  - Phase 1: `json`, `markdown`, `turtle`, `ntriples`
  - Phase 5: `html`, `text`, `doctags`, `vtt`, `jsonld`, `rdfxml`
- `--from <FORMAT>...`: Restrict input formats. Defaults to all supported formats.
- `--stdin`: Read a single file from stdin. Must specify `--format` for output and optionally `--input-format` for the input.
- `--input-format <FORMAT>`: Override format detection (when using `--stdin` or files with ambiguous extensions).
- `--max-pages <N>`: Limit processing to first N pages (for paginated formats).
- `--page-range <START>-<END>`: Process only pages in this range (1-indexed, inclusive).
- `--no-ocr`: Disable OCR (Phase 4+).
- `--no-tables`: Disable table structure detection (Phase 4+).
- `--image-mode <MODE>`: How to handle images in output. Values: `placeholder`, `embedded`, `referenced`. Default: `embedded` for JSON/Markdown/HTML, `placeholder` for text/doctags/vtt.
- `--parallel <N>`: Number of parallel workers for batch conversion. Defaults to CPU count.

**Examples**:
```bash
# Convert a single PDF to Markdown (default)
ruddydoc convert document.pdf

# Convert to JSON
ruddydoc convert document.pdf --format json

# Batch convert all PDFs in a directory to Markdown and JSON
ruddydoc convert ./docs/ --format markdown --format json --output ./output/

# Read from stdin, output to stdout
cat document.pdf | ruddydoc convert --stdin --input-format pdf --format json > output.json

# Convert only Word docs in a directory
ruddydoc convert ./docs/ --from docx --output ./output/
```

**Output**:
- **Single file**: Writes to stdout by default. If `--output` is a directory, writes `<input-stem>.<format-ext>` to that directory.
- **Batch**: Writes all files to `--output` directory. Shows progress bar with current file name, completed count, and ETA.
- **`--json` mode**: Outputs JSON array with conversion results:
  ```json
  {
    "status": "success",
    "processed": 5,
    "successful": 4,
    "failed": 1,
    "results": [
      {
        "input": "doc1.pdf",
        "status": "success",
        "output": "output/doc1.md",
        "format": "markdown"
      },
      {
        "input": "doc2.pdf",
        "status": "failure",
        "error": "Unsupported PDF encryption"
      }
    ]
  }
  ```

**Exit codes**:
- `0`: All conversions succeeded.
- `1`: All conversions failed.
- `2`: Partial success (some conversions succeeded, some failed). Only in batch mode.

**Phase 1 scope**: Single file input, `json`/`markdown`/`turtle`/`ntriples` output formats, Markdown input only.

---

### 2. `query` (Phase 5)

**Purpose**: Run a SPARQL query on parsed documents.

**Usage**:
```
ruddydoc query [OPTIONS] <QUERY> <INPUT>...
```

**Arguments**:
- `<QUERY>`: SPARQL SELECT, ASK, or CONSTRUCT query.
- `<INPUT>...`: One or more files to parse and query.

**Options**:
- `--format, -f <FORMAT>`: Output format for query results. Values: `json`, `table`, `csv`. Default: `table`.

**Examples**:
```bash
# Find all paragraphs in a document
ruddydoc query 'SELECT ?p WHERE { ?p a <https://ruddydoc.chapeaux.io/ontology#Paragraph> }' document.pdf

# Count elements by type
ruddydoc query 'SELECT ?type (COUNT(?e) AS ?count) WHERE { ?e a ?type } GROUP BY ?type' document.pdf --format json
```

**Output**:
- `--format table`: ASCII table with columns from SELECT.
- `--format json`: JSON array of result bindings.
- `--format csv`: CSV with header row.
- `--json` mode: Same as `--format json`.

**Exit codes**:
- `0`: Query executed successfully.
- `1`: Query error (syntax, execution failure, or document parsing failed).

---

### 3. `info` (Phase 5)

**Purpose**: Show document metadata without full conversion.

**Usage**:
```
ruddydoc info <INPUT>...
```

**Arguments**:
- `<INPUT>...`: One or more files.

**Output** (default):
```
document.pdf
  Format: PDF
  Pages: 42
  Size: 2.3 MB
  Title: Annual Report 2025
  Author: Acme Corp
  Created: 2025-03-15
```

**Output** (`--json`):
```json
[
  {
    "path": "document.pdf",
    "format": "pdf",
    "pages": 42,
    "size_bytes": 2411724,
    "metadata": {
      "title": "Annual Report 2025",
      "author": "Acme Corp",
      "created": "2025-03-15"
    }
  }
]
```

**Exit codes**:
- `0`: All files were readable.
- `1`: One or more files could not be read.

---

### 4. `formats` (Phase 5)

**Purpose**: List supported input and output formats.

**Usage**:
```
ruddydoc formats [--input|--output]
```

**Options**:
- `--input`: Show only input formats.
- `--output`: Show only output formats.

**Output** (default):
```
Input Formats:
  pdf        Portable Document Format (.pdf)
  docx       Microsoft Word (.docx)
  xlsx       Microsoft Excel (.xlsx)
  pptx       Microsoft PowerPoint (.pptx)
  html       HTML (.html, .htm)
  markdown   Markdown (.md, .markdown)
  csv        Comma-separated values (.csv)
  latex      LaTeX (.tex)
  ...

Output Formats:
  json       JSON (docling-compatible)
  markdown   Markdown
  html       HTML5
  text       Plain text
  turtle     RDF Turtle
  ntriples   RDF N-Triples
  jsonld     JSON-LD
  ...
```

**Output** (`--json`):
```json
{
  "input": [
    {"name": "pdf", "extensions": [".pdf"], "description": "Portable Document Format"},
    ...
  ],
  "output": [
    {"name": "json", "extensions": [".json"], "description": "JSON (docling-compatible)"},
    ...
  ]
}
```

**Exit codes**: Always `0`.

---

### 5. `models` (Phase 4+)

**Purpose**: Manage ML models for layout analysis, OCR, and table structure.

**Subcommands**:

#### `models list`

List available models and their status (downloaded or not).

**Usage**:
```
ruddydoc models list
```

**Output**:
```
Layout Models:
  docling-layout-v1  [downloaded]  IBM Docling layout analysis
  
OCR Models:
  rapid-ocr          [downloaded]  RapidOCR ONNX
  tesseract          [available]   Tesseract OCR (requires external install)
  
Table Structure Models:
  docling-table-v1   [downloaded]  IBM Docling table structure
```

**Output** (`--json`):
```json
{
  "layout": [
    {"name": "docling-layout-v1", "downloaded": true, "description": "IBM Docling layout analysis"}
  ],
  "ocr": [
    {"name": "rapid-ocr", "downloaded": true, "description": "RapidOCR ONNX"},
    {"name": "tesseract", "downloaded": false, "description": "Tesseract OCR (requires external install)"}
  ],
  "table": [
    {"name": "docling-table-v1", "downloaded": true, "description": "IBM Docling table structure"}
  ]
}
```

#### `models download`

Download a model from HuggingFace Hub.

**Usage**:
```
ruddydoc models download <MODEL>
```

**Arguments**:
- `<MODEL>`: Model name (from `models list`).

**Output**:
```
Downloading docling-layout-v1...
  [################] 100% (245 MB / 245 MB) - 12 MB/s
Downloaded to: ~/.cache/ruddydoc/models/docling-layout-v1
```

**Exit codes**:
- `0`: Download succeeded.
- `1`: Download failed (network error, disk full, invalid model name).

---

### 6. `chunk` (Phase 5)

**Purpose**: Chunk a document for RAG (Retrieval Augmented Generation) workflows.

**Usage**:
```
ruddydoc chunk [OPTIONS] <INPUT>
```

**Arguments**:
- `<INPUT>`: Input file.

**Options**:
- `--strategy <STRATEGY>`: Chunking strategy. Values: `hierarchical`, `hybrid`. Default: `hierarchical`.
- `--max-tokens <N>`: Maximum tokens per chunk. Default: 512.
- `--overlap <N>`: Overlap tokens between chunks. Default: 50.
- `--format, -f <FORMAT>`: Output format. Values: `json`, `jsonl`. Default: `json`.

**Output** (`json`):
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
        "section": "Introduction",
        "element_types": ["Paragraph", "SectionHeader"]
      }
    },
    ...
  ]
}
```

**Output** (`jsonl`):
```jsonl
{"id":"chunk-0","text":"...","tokens":487,"metadata":{...}}
{"id":"chunk-1","text":"...","tokens":512,"metadata":{...}}
```

**Exit codes**:
- `0`: Chunking succeeded.
- `1`: Chunking failed (parse error, invalid strategy).

---

### 7. `serve` (Phase 5)

**Purpose**: Start MCP server for AI agent integration.

**Usage**:
```
ruddydoc serve [OPTIONS]
```

**Options**:
- `--port, -p <PORT>`: HTTP/SSE port. If omitted, runs in stdio mode (for MCP clients like Claude Desktop).
- `--host <HOST>`: Bind address. Default: `127.0.0.1`.

**Examples**:
```bash
# Start stdio MCP server (for Claude Desktop config)
ruddydoc serve

# Start HTTP/SSE server on port 8080
ruddydoc serve --port 8080
```

**Output** (stdio mode):
```
RuddyDoc MCP server ready (stdio mode)
```

**Output** (HTTP mode):
```
RuddyDoc MCP server listening on http://127.0.0.1:8080
```

**MCP Tools Exposed**:
- `convert_document`: Convert a file, return document graph IRI.
- `query_document`: Run SPARQL on a converted document.
- `export_document`: Export in a given format.
- `list_elements`: List document elements by type.
- `get_element`: Get a specific element's details.
- `chunk_document`: Chunk a document for RAG.

**Exit codes**:
- `0`: Server shut down cleanly.
- `1`: Server startup failed (port in use, invalid config).

---

### 8. `completions` (Phase 5)

**Purpose**: Generate shell completion scripts.

**Usage**:
```
ruddydoc completions <SHELL>
```

**Arguments**:
- `<SHELL>`: Shell type. Values: `bash`, `zsh`, `fish`, `powershell`.

**Example**:
```bash
# Install bash completions
ruddydoc completions bash > /etc/bash_completion.d/ruddydoc

# Install zsh completions
ruddydoc completions zsh > ~/.zsh/completions/_ruddydoc
```

**Exit codes**: Always `0`.

---

## Output Formatting

### Progress Bars

Used for batch conversion when processing multiple files:

```
Converting documents... [42/100] document-042.pdf
[#################                    ] 42% - ETA: 2m 15s
```

Components:
- Current file name (truncated to terminal width)
- Progress bar with percentage
- ETA (estimated time remaining)

**Implementation**: Use `indicatif` crate.

**When to show**:
- Batch conversion with >1 file
- Only in non-`--quiet`, non-`--json` mode
- Only if stderr is a TTY

### Color Usage

**Default mode** (when stderr is a TTY and `NO_COLOR` is not set):
- Success messages: Green
- Warnings: Yellow
- Errors: Red
- File names: Cyan
- Counts/numbers: Bold

**Symbols for accessibility** (always shown, regardless of color):
- Success: `✓`
- Warning: `⚠`
- Error: `✗`
- Info: `ℹ`

**Implementation**: Use `colored` or `owo-colors` crate. Check `atty::is(Stream::Stderr)` and `std::env::var("NO_COLOR")`.

### Quiet Mode (`--quiet`)

- Suppresses all output except errors.
- Exit codes still indicate success/failure.
- Errors still go to stderr.

### Verbose Mode (`--verbose`)

- `-v`: INFO-level logs (file processing, backend selection, model loading).
- `-vv`: DEBUG-level logs (SPARQL queries, triple insertion, detailed timing).

**Format**:
```
[INFO] Detected format: pdf
[INFO] Using backend: docling-parse
[INFO] Loading layout model: docling-layout-v1
[DEBUG] Inserted 1247 triples into graph urn:ruddydoc:doc:abc123
```

### JSON Mode (`--json`)

All commands support `--json` for machine-readable output:

- **Success**: Valid JSON to stdout.
- **Errors**: JSON to stdout with `"status": "error"`, non-zero exit code.
- **No progress bars, no color, no human-friendly formatting.**

Example error in JSON mode:
```json
{
  "status": "error",
  "message": "File not found: missing.pdf",
  "code": "ERR_FILE_NOT_FOUND"
}
```

---

## Error Messages

### Design

Every error message must answer:
1. **What happened?** (concise description)
2. **Why?** (root cause, if known)
3. **How do I fix it?** (actionable suggestion)

**Format** (non-JSON):
```
Error: <What happened>
Cause: <Why it happened>
Fix: <How to resolve it>
```

**Format** (JSON):
```json
{
  "status": "error",
  "message": "<What happened>",
  "cause": "<Why it happened>",
  "fix": "<How to resolve it>",
  "code": "ERR_CODE"
}
```

### Error Codes

| Code | Meaning |
|------|---------|
| `ERR_FILE_NOT_FOUND` | Input file does not exist |
| `ERR_UNSUPPORTED_FORMAT` | File format is not supported |
| `ERR_PARSE_ERROR` | Document could not be parsed |
| `ERR_OUTPUT_DIR_INVALID` | Output directory does not exist or is not writable |
| `ERR_SPARQL_SYNTAX` | SPARQL query has syntax errors |
| `ERR_SPARQL_EXECUTION` | SPARQL query failed during execution |
| `ERR_MODEL_NOT_FOUND` | Requested ML model is not downloaded |
| `ERR_CONVERSION_TIMEOUT` | Document conversion exceeded timeout |
| `ERR_INVALID_ARGUMENT` | CLI argument is invalid |

### Example Error Messages

#### 1. File Not Found

**Non-JSON**:
```
Error: File not found: missing.pdf
Cause: The file does not exist at the specified path.
Fix: Check the file path and try again. Use 'ruddydoc formats --input' to see supported formats.
```

**JSON**:
```json
{
  "status": "error",
  "message": "File not found: missing.pdf",
  "cause": "The file does not exist at the specified path.",
  "fix": "Check the file path and try again. Use 'ruddydoc formats --input' to see supported formats.",
  "code": "ERR_FILE_NOT_FOUND"
}
```

#### 2. Unsupported Format

**Non-JSON**:
```
Error: Unsupported format: .xyz
Cause: RuddyDoc does not recognize the file extension '.xyz'.
Fix: Convert the file to a supported format first, or use '--input-format' to override detection. Run 'ruddydoc formats --input' to see supported formats.
```

**JSON**:
```json
{
  "status": "error",
  "message": "Unsupported format: .xyz",
  "cause": "RuddyDoc does not recognize the file extension '.xyz'.",
  "fix": "Convert the file to a supported format first, or use '--input-format' to override detection. Run 'ruddydoc formats --input' to see supported formats.",
  "code": "ERR_UNSUPPORTED_FORMAT"
}
```

#### 3. Parse Error

**Non-JSON**:
```
Error: Failed to parse document.pdf
Cause: The PDF file is encrypted and requires a password.
Fix: Use '--pdf-password <PASSWORD>' to provide the password, or decrypt the file first.
```

**JSON**:
```json
{
  "status": "error",
  "message": "Failed to parse document.pdf",
  "cause": "The PDF file is encrypted and requires a password.",
  "fix": "Use '--pdf-password <PASSWORD>' to provide the password, or decrypt the file first.",
  "code": "ERR_PARSE_ERROR"
}
```

#### 4. Output Directory Invalid

**Non-JSON**:
```
Error: Output directory does not exist: /nonexistent/
Cause: The specified output directory was not found.
Fix: Create the directory first with 'mkdir -p /nonexistent/' or specify an existing directory.
```

**JSON**:
```json
{
  "status": "error",
  "message": "Output directory does not exist: /nonexistent/",
  "cause": "The specified output directory was not found.",
  "fix": "Create the directory first with 'mkdir -p /nonexistent/' or specify an existing directory.",
  "code": "ERR_OUTPUT_DIR_INVALID"
}
```

#### 5. SPARQL Syntax Error

**Non-JSON**:
```
Error: Invalid SPARQL query syntax
Cause: Expected '}' at line 1, column 45.
Fix: Check your query syntax. SPARQL SELECT queries must have the form:
  SELECT ?var WHERE { ?subject ?predicate ?object }
```

**JSON**:
```json
{
  "status": "error",
  "message": "Invalid SPARQL query syntax",
  "cause": "Expected '}' at line 1, column 45.",
  "fix": "Check your query syntax. SPARQL SELECT queries must have the form: SELECT ?var WHERE { ?subject ?predicate ?object }",
  "code": "ERR_SPARQL_SYNTAX"
}
```

#### 6. SPARQL Execution Error

**Non-JSON**:
```
Error: SPARQL query execution failed
Cause: The document graph is empty. No triples were extracted from the input file.
Fix: Verify that the input file contains parseable content. Try converting to JSON first to inspect the document structure.
```

**JSON**:
```json
{
  "status": "error",
  "message": "SPARQL query execution failed",
  "cause": "The document graph is empty. No triples were extracted from the input file.",
  "fix": "Verify that the input file contains parseable content. Try converting to JSON first to inspect the document structure.",
  "code": "ERR_SPARQL_EXECUTION"
}
```

#### 7. Model Not Found

**Non-JSON**:
```
Error: ML model not found: docling-layout-v1
Cause: The model has not been downloaded yet.
Fix: Download the model with 'ruddydoc models download docling-layout-v1' or disable ML features with '--no-ocr --no-tables'.
```

**JSON**:
```json
{
  "status": "error",
  "message": "ML model not found: docling-layout-v1",
  "cause": "The model has not been downloaded yet.",
  "fix": "Download the model with 'ruddydoc models download docling-layout-v1' or disable ML features with '--no-ocr --no-tables'.",
  "code": "ERR_MODEL_NOT_FOUND"
}
```

#### 8. Conversion Timeout

**Non-JSON**:
```
Error: Document conversion timed out after 120 seconds
Cause: The document is very large or processing is stuck.
Fix: Try increasing the timeout with '--timeout 300', or process only a subset of pages with '--max-pages 10'.
```

**JSON**:
```json
{
  "status": "error",
  "message": "Document conversion timed out after 120 seconds",
  "cause": "The document is very large or processing is stuck.",
  "fix": "Try increasing the timeout with '--timeout 300', or process only a subset of pages with '--max-pages 10'.",
  "code": "ERR_CONVERSION_TIMEOUT"
}
```

#### 9. Invalid Argument

**Non-JSON**:
```
Error: Invalid value for '--format': badformat
Cause: 'badformat' is not a recognized output format.
Fix: Use one of: json, markdown, html, text, turtle, ntriples, jsonld, rdfxml, doctags, vtt. Run 'ruddydoc formats --output' for the full list.
```

**JSON**:
```json
{
  "status": "error",
  "message": "Invalid value for '--format': badformat",
  "cause": "'badformat' is not a recognized output format.",
  "fix": "Use one of: json, markdown, html, text, turtle, ntriples, jsonld, rdfxml, doctags, vtt. Run 'ruddydoc formats --output' for the full list.",
  "code": "ERR_INVALID_ARGUMENT"
}
```

#### 10. Warning (Non-Fatal)

**Non-JSON**:
```
Warning: Page 7 has no extractable text
Cause: The page may be a scanned image without OCR data.
Fix: Enable OCR with the default settings (OCR is enabled by default in Phase 4+).
```

**Note**: Warnings do not cause non-zero exit codes unless the entire conversion fails.

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success (all operations succeeded) |
| `1` | Error (all operations failed, or a fatal error occurred) |
| `2` | Partial success (batch mode: some conversions succeeded, some failed) |

**Usage**:
- Single-file commands: Use `0` or `1`.
- Batch commands: Use `0`, `1`, or `2`.
- CI/CD scripts can check `$?` to determine success/failure.

---

## Comparison with Python Docling CLI

### What Python Docling Does

1. **Single command**: `docling convert <sources>` with many options.
2. **Input**: Accepts files, directories, and URLs. Auto-detects formats.
3. **Output formats**: JSON, YAML, Markdown, HTML, HTML split-page, Text, DocTags, WebVTT.
4. **Image export modes**: `placeholder`, `embedded`, `referenced`.
5. **Pipelines**: `--pipeline standard|vlm|asr` for different processing strategies.
6. **OCR options**: `--ocr`, `--force-ocr`, `--ocr-engine`, `--ocr-lang`, `--psm`.
7. **Table options**: `--tables`, `--table-mode accurate|fast`.
8. **Enrichment**: `--enrich-code`, `--enrich-formula`, `--enrich-picture-classes`, `--enrich-picture-description`, `--enrich-chart-extraction`.
9. **PDF backend**: `--pdf-backend docling-parse|pypdfium2`.
10. **Verbosity**: `--verbose` (`-v`, `-vv`).
11. **Debug visualization**: `--debug-visualize-cells`, `--debug-visualize-ocr`, `--debug-visualize-layout`, `--debug-visualize-tables`.
12. **Profiling**: `--profiling`, `--save-profiling`.
13. **Version**: `--version` shows detailed version info for all dependencies.
14. **Logo**: `--logo` prints ASCII art.
15. **External plugins**: `--allow-external-plugins`, `--show-external-plugins`.
16. **Output**: Single `--output <dir>` for all formats.

### What RuddyDoc Adds

1. **SPARQL querying**: `ruddydoc query` for direct graph queries.
2. **RDF export formats**: `turtle`, `ntriples`, `jsonld`, `rdfxml` (not in Python docling).
3. **MCP server**: `ruddydoc serve` for AI agent integration (not in Python docling).
4. **Document metadata**: `ruddydoc info` for quick metadata inspection.
5. **Model management**: `ruddydoc models list/download` for ML model management.
6. **Chunking**: `ruddydoc chunk` for RAG workflows.
7. **Shell completions**: `ruddydoc completions`.

### What's Equivalent

| Python Docling | RuddyDoc | Notes |
|----------------|----------|-------|
| `docling convert --to markdown` | `ruddydoc convert --format markdown` | Same functionality |
| `docling convert --to json` | `ruddydoc convert --format json` | RuddyDoc JSON is docling-compatible |
| `docling convert --to html` | `ruddydoc convert --format html` | Same |
| `docling convert --from pdf` | `ruddydoc convert --from pdf` | Same |
| `docling convert --ocr` | `ruddydoc convert` (default in Phase 4+) | OCR enabled by default |
| `docling convert --no-ocr` | `ruddydoc convert --no-ocr` | Disable OCR |
| `docling convert --tables` | `ruddydoc convert` (default in Phase 4+) | Tables enabled by default |
| `docling convert --no-tables` | `ruddydoc convert --no-tables` | Disable table detection |
| `docling convert --verbose` | `ruddydoc convert --verbose` | Same |
| `docling convert --output ./out/` | `ruddydoc convert --output ./out/` | Same |
| `docling convert --version` | `ruddydoc --version` | RuddyDoc shows version info globally |

### What RuddyDoc Does NOT Include (vs Python Docling)

1. **VLM pipeline**: Not in initial phases. May be added later.
2. **ASR pipeline**: Not in initial phases. May be added later.
3. **Enrichment flags**: `--enrich-code`, `--enrich-formula`, etc. — not in Phase 1-5. May be added as a Phase 6 feature.
4. **Debug visualization flags**: Not in initial scope. Use `--verbose` instead.
5. **Profiling flags**: Not in initial scope. Internal profiling may be added for benchmarking.
6. **External plugins**: Not in initial scope. RuddyDoc backends are built-in only.
7. **HTTP input sources**: Not in Phase 1. May be added in Phase 5 or 6.
8. **`--logo` flag**: Not planned. RuddyDoc focuses on utility over branding.

---

## Implementation Notes for Engineers

1. **Use `clap` derive macros** for argument parsing. Follow beret's pattern.
2. **Progress bars**: Use `indicatif`. Only show when stderr is a TTY and not in `--json` or `--quiet` mode.
3. **Logging**: Use `env_logger` or `tracing`. Map `--verbose` to log levels: default = WARN, `-v` = INFO, `-vv` = DEBUG.
4. **Colors**: Use `colored` or `owo-colors`. Disable if `atty::is(Stream::Stderr)` is false or `NO_COLOR` is set.
5. **Exit codes**: Use `std::process::exit(code)` for explicit exit codes. Ensure batch mode returns `2` for partial success.
6. **Error messages**: Define error types in `ruddydoc-core` with `Display` impls that follow the "What/Why/Fix" format.
7. **JSON mode**: Use `serde_json::to_string_pretty` for all JSON output.
8. **Stdin input**: Use `std::io::stdin()` with buffering. Require `--input-format` unless format can be auto-detected from magic bytes.
9. **Shell completions**: Use `clap_complete` to generate completions for bash, zsh, fish, powershell.
10. **Help text**: Follow clap conventions. Use `#[command(about = "...")]` for command descriptions. Use `#[arg(help = "...")]` for argument descriptions.

---

## Acceptance Criteria

- [ ] All Phase 1 commands implemented and tested: `convert`, `--version`, `--help`.
- [ ] `--json` mode works for all commands.
- [ ] Progress bars show correctly for batch conversion (>1 file).
- [ ] Error messages follow the "What/Why/Fix" format.
- [ ] Colors and symbols render correctly in TTY mode, disable in non-TTY mode.
- [ ] Exit codes are correct for single-file and batch modes.
- [ ] `--verbose` and `--quiet` modes work as specified.
- [ ] Shell completions generate correctly for bash, zsh, fish, powershell.
- [ ] All Phase 5 commands implemented: `query`, `info`, `formats`, `models`, `chunk`, `serve`, `completions`.

---

## Open Questions

1. **Should `--json` be a global flag or per-command?** → **Decision: Global flag.** Consistent with `--verbose` and `--quiet`.
2. **Should `convert` default to stdout or require `--output`?** → **Decision: Default to stdout for single file, require `--output` for batch.**
3. **Should we support URL input sources in Phase 1?** → **Decision: No, defer to Phase 5.**
4. **Should we support `--pdf-password` in Phase 1?** → **Decision: Yes, add to Phase 3 when PDF backend is implemented.**
5. **Should we add a `--watch` mode for live reloading?** → **Decision: Defer to Phase 6 or later.**

---

## Changelog

- **2026-04-07**: Initial draft (Designer, Phase 5).
