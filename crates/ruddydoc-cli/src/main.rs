//! Command-line interface for RuddyDoc document conversion.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use ruddydoc_converter::{ConversionResult, ConvertOptions, DocumentConverter};
use ruddydoc_core::{ConversionStatus, DocumentSource, DocumentStore, OutputFormat};

/// RuddyDoc: fast document conversion with an embedded knowledge graph.
///
/// Convert documents between formats, run structured queries, and chunk
/// content for AI workflows -- all from the command line.
#[derive(Parser)]
#[command(name = "ruddydoc", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert one or more documents to the specified output format.
    Convert(ConvertArgs),

    /// List all supported input and output formats.
    Formats,

    /// Show detected format and file metadata without full conversion.
    Info {
        /// Input file path.
        input: PathBuf,
    },

    /// Run a structured query against one or more documents.
    Query(QueryArgs),

    /// Split documents into chunks for AI retrieval workflows.
    Chunk(ChunkArgs),

    /// Start the document server (requires the "server" feature).
    Serve(ServeArgs),

    /// Manage ML models for layout analysis, OCR, and table detection.
    Models(ModelsArgs),
}

// ---------------------------------------------------------------------------
// Output format argument
// ---------------------------------------------------------------------------

/// Output format argument for the CLI.
#[derive(Debug, Clone, ValueEnum)]
enum OutputFormatArg {
    /// JSON (docling-compatible)
    Json,
    /// Markdown
    Markdown,
    /// HTML
    Html,
    /// Plain text
    Text,
    /// RDF Turtle
    Turtle,
    /// RDF N-Triples
    Ntriples,
    /// JSON-LD (linked data)
    #[value(name = "jsonld")]
    JsonLd,
    /// RDF/XML
    #[value(name = "rdfxml")]
    RdfXml,
    /// DocTags (tagged format)
    #[value(name = "doctags")]
    DocTags,
    /// WebVTT subtitles
    #[value(name = "webvtt")]
    WebVtt,
}

impl OutputFormatArg {
    fn to_output_format(&self) -> OutputFormat {
        match self {
            Self::Json => OutputFormat::Json,
            Self::Markdown => OutputFormat::Markdown,
            Self::Html => OutputFormat::Html,
            Self::Text => OutputFormat::Text,
            Self::Turtle => OutputFormat::Turtle,
            Self::Ntriples => OutputFormat::NTriples,
            Self::JsonLd => OutputFormat::JsonLd,
            Self::RdfXml => OutputFormat::RdfXml,
            Self::DocTags => OutputFormat::DocTags,
            Self::WebVtt => OutputFormat::WebVtt,
        }
    }
}

// ---------------------------------------------------------------------------
// Query output format argument
// ---------------------------------------------------------------------------

/// Output format for query results.
#[derive(Debug, Clone, ValueEnum)]
enum QueryOutputFormat {
    /// JSON array of result bindings.
    Json,
    /// Human-readable ASCII table.
    Table,
}

// ---------------------------------------------------------------------------
// Convert subcommand
// ---------------------------------------------------------------------------

/// Arguments for the `convert` subcommand.
#[derive(Parser)]
struct ConvertArgs {
    /// One or more input files to convert.
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Output file path (for single-file conversion) or directory (for batch).
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format.
    #[arg(short, long, default_value = "json")]
    format: OutputFormatArg,

    /// Maximum file size in bytes.
    #[arg(long)]
    max_file_size: Option<u64>,
}

// ---------------------------------------------------------------------------
// Query subcommand
// ---------------------------------------------------------------------------

/// Arguments for the `query` subcommand.
#[derive(Parser)]
struct QueryArgs {
    /// Structured query string (SPARQL syntax).
    sparql: String,

    /// One or more input files to parse and query.
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Output format for query results.
    #[arg(long, default_value = "json")]
    format: QueryOutputFormat,
}

// ---------------------------------------------------------------------------
// Chunk subcommand
// ---------------------------------------------------------------------------

/// Arguments for the `chunk` subcommand.
#[derive(Parser)]
struct ChunkArgs {
    /// One or more input files to chunk.
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Maximum tokens per chunk.
    #[arg(long, default_value = "512")]
    max_tokens: usize,

    /// Include heading hierarchy in each chunk for better context.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    include_headings: bool,

    /// Merge consecutive list items into a single chunk.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    merge_lists: bool,

    /// Output format.
    #[arg(long, short, default_value = "json")]
    format: OutputFormatArg,
}

// ---------------------------------------------------------------------------
// Serve subcommand
// ---------------------------------------------------------------------------

/// Arguments for the `serve` subcommand.
#[derive(Parser)]
struct ServeArgs {
    /// Port to listen on.
    #[arg(long, default_value = "8080")]
    port: u16,

    /// Run MCP server on stdio instead of HTTP.
    #[arg(long)]
    mcp: bool,
}

// ---------------------------------------------------------------------------
// Models subcommand
// ---------------------------------------------------------------------------

/// Arguments for the `models` subcommand.
#[derive(Parser)]
struct ModelsArgs {
    #[command(subcommand)]
    action: ModelsAction,
}

/// Model management actions.
#[derive(Subcommand)]
enum ModelsAction {
    /// List available models and their download status.
    List,
    /// Download a model by name.
    Download {
        /// Name of the model to download.
        model: String,
    },
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Convert(args) => {
            let exit_code = run_convert(&args);
            std::process::exit(exit_code);
        }
        Commands::Formats => {
            run_formats();
        }
        Commands::Info { input } => {
            if let Err(e) = run_info(&input) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Query(args) => {
            if let Err(e) = run_query(&args) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Chunk(args) => {
            if let Err(e) = run_chunk(&args) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Serve(args) => {
            run_serve(&args);
        }
        Commands::Models(args) => {
            run_models(&args);
        }
    }
}

// ---------------------------------------------------------------------------
// convert
// ---------------------------------------------------------------------------

/// Run the convert subcommand. Returns an exit code (0, 1, or 2).
fn run_convert(args: &ConvertArgs) -> i32 {
    let is_batch = args.files.len() > 1;
    let output_dir = if is_batch {
        args.output.as_deref()
    } else {
        None
    };
    let single_output = if !is_batch {
        args.output.as_deref()
    } else {
        None
    };

    // Validate output directory for batch mode.
    if let Some(dir) = output_dir
        && !dir.is_dir()
    {
        eprintln!("error: output directory does not exist: {}", dir.display());
        return 1;
    }

    let mut success_count = 0usize;
    let mut fail_count = 0usize;
    let total = args.files.len();

    for (i, input) in args.files.iter().enumerate() {
        if is_batch {
            eprintln!("[{}/{}] {}", i + 1, total, input.display());
        }
        match run_convert_single(
            input,
            single_output,
            output_dir,
            &args.format,
            args.max_file_size,
        ) {
            Ok(()) => success_count += 1,
            Err(e) => {
                eprintln!("error: {e}");
                fail_count += 1;
            }
        }
    }

    if fail_count == 0 {
        0
    } else if success_count == 0 {
        1
    } else {
        2
    }
}

/// Convert a single file. For batch mode, `output_dir` determines where to
/// write the result. For single-file mode, `single_output` is used (stdout
/// if None).
fn run_convert_single(
    input: &Path,
    single_output: Option<&Path>,
    output_dir: Option<&Path>,
    format: &OutputFormatArg,
    max_file_size: Option<u64>,
) -> ruddydoc_core::Result<()> {
    if !input.exists() {
        return Err(format!("file not found: {}", input.display()).into());
    }

    let options = ConvertOptions {
        max_file_size,
        max_pages: None,
    };
    let converter = DocumentConverter::new(options);
    let source = DocumentSource::File(input.to_path_buf());
    let result = converter.convert(source)?;

    if result.status == ConversionStatus::Failure {
        return Err(format!(
            "conversion failed for '{}' (format: {})",
            input.display(),
            result.input.format
        )
        .into());
    }

    eprintln!(
        "converted {} ({}, {} bytes, {} triples)",
        input.display(),
        result.input.format,
        result.input.file_size,
        result.store.triple_count().unwrap_or(0),
    );

    let output_format = format.to_output_format();
    let exported = export_result(&result, output_format)?;

    // Determine where to write.
    let write_path: Option<PathBuf> = if let Some(dir) = output_dir {
        let stem = input
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output".to_string());
        let ext = output_format_extension(output_format);
        Some(dir.join(format!("{stem}.{ext}")))
    } else {
        single_output.map(|p| p.to_path_buf())
    };

    match write_path {
        Some(path) => {
            std::fs::write(&path, &exported)?;
            eprintln!("wrote {}", path.display());
        }
        None => {
            print!("{exported}");
        }
    }

    Ok(())
}

/// Export a conversion result in the given format.
fn export_result(
    result: &ConversionResult,
    output_format: OutputFormat,
) -> ruddydoc_core::Result<String> {
    let exporter = ruddydoc_export::exporter_for(output_format)?;
    exporter.export(result.store.as_ref(), &result.doc_graph)
}

/// Return the conventional file extension for an output format.
fn output_format_extension(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Json => "json",
        OutputFormat::Markdown => "md",
        OutputFormat::Html => "html",
        OutputFormat::Text => "txt",
        OutputFormat::Turtle => "ttl",
        OutputFormat::NTriples => "nt",
        OutputFormat::JsonLd => "jsonld",
        OutputFormat::RdfXml => "rdf",
        OutputFormat::DocTags => "dt",
        OutputFormat::WebVtt => "vtt",
    }
}

// ---------------------------------------------------------------------------
// formats
// ---------------------------------------------------------------------------

fn run_formats() {
    let converter = DocumentConverter::default_converter();
    let registry = converter.registry();
    let registered = registry.supported_formats();

    println!("Input formats:");
    println!("{:<12} {:<50} EXTENSIONS", "FORMAT", "MIME TYPE");
    println!("{}", "-".repeat(80));

    for info in ruddydoc_converter::list_supported_formats() {
        let has_backend = if registered.contains(&info.format) {
            ""
        } else {
            " (no backend)"
        };
        println!("{info}{has_backend}");
    }

    println!();
    println!("Output formats:");
    let output_formats = [
        OutputFormat::Json,
        OutputFormat::Markdown,
        OutputFormat::Html,
        OutputFormat::Text,
        OutputFormat::Turtle,
        OutputFormat::NTriples,
        OutputFormat::JsonLd,
        OutputFormat::RdfXml,
        OutputFormat::DocTags,
        OutputFormat::WebVtt,
    ];
    for fmt in &output_formats {
        println!("  {fmt}");
    }
}

// ---------------------------------------------------------------------------
// info
// ---------------------------------------------------------------------------

fn run_info(input: &Path) -> ruddydoc_core::Result<()> {
    if !input.exists() {
        return Err(format!("file not found: {}", input.display()).into());
    }

    let source = DocumentSource::File(input.to_path_buf());
    let info = DocumentConverter::file_info(&source)?;
    print!("{info}");
    Ok(())
}

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

fn run_query(args: &QueryArgs) -> ruddydoc_core::Result<()> {
    // Parse each file and load into a shared store.
    let store = Arc::new(ruddydoc_graph::OxigraphStore::new()?);
    ruddydoc_ontology::load_ontology(store.as_ref())?;

    for input in &args.files {
        if !input.exists() {
            return Err(format!("file not found: {}", input.display()).into());
        }
        convert_into_store(input, &store)?;
    }

    // Execute the query.
    let results = store.query_to_json(&args.sparql)?;

    match args.format {
        QueryOutputFormat::Json => {
            let json_str = serde_json::to_string_pretty(&results)
                .map_err(|e| -> ruddydoc_core::Error { e.to_string().into() })?;
            println!("{json_str}");
        }
        QueryOutputFormat::Table => {
            print_results_table(&results);
        }
    }

    Ok(())
}

/// Convert a file and insert its triples into an existing store.
fn convert_into_store(
    input: &Path,
    store: &ruddydoc_graph::OxigraphStore,
) -> ruddydoc_core::Result<String> {
    let bytes = std::fs::read(input)?;
    let file_size = bytes.len() as u64;
    let source = DocumentSource::Stream {
        name: input
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string()),
        data: bytes.clone(),
    };

    let format = DocumentConverter::detect_format(&source)
        .ok_or_else(|| format!("could not detect format for '{}'", input.display()))?;

    let converter = DocumentConverter::default_converter();
    let registry = converter.registry();
    let backend = registry
        .backend_for(format)
        .ok_or_else(|| format!("no backend registered for format '{format}'"))?;

    let hash_str = {
        use sha2::{Digest, Sha256};
        use std::fmt::Write;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let result = hasher.finalize();
        result.iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
    };
    let doc_graph = ruddydoc_core::doc_iri(&hash_str);

    backend.parse(&source, store, &doc_graph)?;
    eprintln!(
        "parsed {} ({}, {} bytes)",
        input.display(),
        format,
        file_size
    );

    Ok(doc_graph)
}

/// Print query results as a formatted ASCII table.
fn print_results_table(results: &serde_json::Value) {
    match results {
        serde_json::Value::Bool(b) => {
            println!("{b}");
        }
        serde_json::Value::Array(rows) if rows.is_empty() => {
            println!("(no results)");
        }
        serde_json::Value::Array(rows) => {
            // Collect column names from the first row.
            let columns: Vec<String> = if let Some(serde_json::Value::Object(first)) = rows.first()
            {
                first.keys().cloned().collect()
            } else {
                return;
            };

            // Calculate column widths.
            let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
            for row in rows {
                if let serde_json::Value::Object(obj) = row {
                    for (i, col) in columns.iter().enumerate() {
                        let val_len = obj
                            .get(col)
                            .and_then(|v| v.as_str())
                            .map(|s| s.len())
                            .unwrap_or(4); // "null"
                        if val_len > widths[i] {
                            widths[i] = val_len;
                        }
                    }
                }
            }

            // Print header.
            let header: Vec<String> = columns
                .iter()
                .zip(widths.iter())
                .map(|(c, w)| format!("{c:<w$}"))
                .collect();
            println!("{}", header.join("  "));

            // Print separator.
            let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            println!("{}", sep.join("  "));

            // Print rows.
            for row in rows {
                if let serde_json::Value::Object(obj) = row {
                    let cells: Vec<String> = columns
                        .iter()
                        .zip(widths.iter())
                        .map(|(col, w)| {
                            let val = obj.get(col).and_then(|v| v.as_str()).unwrap_or("null");
                            format!("{val:<w$}")
                        })
                        .collect();
                    println!("{}", cells.join("  "));
                }
            }
        }
        _ => {
            println!("{results}");
        }
    }
}

// ---------------------------------------------------------------------------
// chunk
// ---------------------------------------------------------------------------

/// Serializable chunk output for JSON rendering.
#[derive(Serialize)]
struct ChunkOutput {
    chunks: Vec<ruddydoc_export::Chunk>,
}

fn run_chunk(args: &ChunkArgs) -> ruddydoc_core::Result<()> {
    let options = ruddydoc_export::ChunkOptions {
        max_tokens: args.max_tokens,
        include_headings: args.include_headings,
        merge_list_items: args.merge_lists,
        ..Default::default()
    };

    let mut all_chunks: Vec<ruddydoc_export::Chunk> = Vec::new();

    for input in &args.files {
        if !input.exists() {
            return Err(format!("file not found: {}", input.display()).into());
        }

        let converter = DocumentConverter::default_converter();
        let source = DocumentSource::File(input.to_path_buf());
        let result = converter.convert(source)?;

        if result.status == ConversionStatus::Failure {
            return Err(format!(
                "conversion failed for '{}' (format: {})",
                input.display(),
                result.input.format
            )
            .into());
        }

        let chunks =
            ruddydoc_export::chunk_document(result.store.as_ref(), &result.doc_graph, &options)?;

        eprintln!("chunked {} into {} chunks", input.display(), chunks.len());

        all_chunks.extend(chunks);
    }

    // Re-index chunks across all files.
    for (i, chunk) in all_chunks.iter_mut().enumerate() {
        chunk.metadata.chunk_index = i;
    }

    let output = ChunkOutput { chunks: all_chunks };
    let json_str = serde_json::to_string_pretty(&output)
        .map_err(|e| -> ruddydoc_core::Error { e.to_string().into() })?;
    println!("{json_str}");

    Ok(())
}

// ---------------------------------------------------------------------------
// serve (stub)
// ---------------------------------------------------------------------------

fn run_serve(args: &ServeArgs) {
    if args.mcp {
        eprintln!(
            "The MCP server is not yet compiled in.\n\
             To use it, build with the 'server' feature:\n\
             \n\
             cargo install ruddydoc --features server\n"
        );
    } else {
        eprintln!(
            "The HTTP server on port {} is not yet compiled in.\n\
             To use it, build with the 'server' feature:\n\
             \n\
             cargo install ruddydoc --features server\n",
            args.port
        );
    }
    std::process::exit(1);
}

// ---------------------------------------------------------------------------
// models (stub)
// ---------------------------------------------------------------------------

fn run_models(args: &ModelsArgs) {
    match &args.action {
        ModelsAction::List => {
            println!("{:<20} {:<16}", "Model", "Status");
            println!("{}", "-".repeat(36));
            println!("{:<20} {:<16}", "layout-analysis", "not downloaded");
            println!("{:<20} {:<16}", "table-structure", "not downloaded");
            println!("{:<20} {:<16}", "ocr", "not downloaded");
            println!("{:<20} {:<16}", "vlm", "not downloaded");
        }
        ModelsAction::Download { model } => {
            eprintln!(
                "Model download is not yet implemented.\n\
                 Requested model: {model}\n\
                 This feature will be available in a future release."
            );
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // -----------------------------------------------------------------------
    // CLI parsing: convert
    // -----------------------------------------------------------------------

    #[test]
    fn parse_convert_single_file() {
        let cli = Cli::parse_from(["ruddydoc", "convert", "doc.pdf"]);
        match cli.command {
            Commands::Convert(args) => {
                assert_eq!(args.files.len(), 1);
                assert_eq!(args.files[0], PathBuf::from("doc.pdf"));
                assert!(args.output.is_none());
            }
            _ => panic!("expected Convert command"),
        }
    }

    #[test]
    fn parse_convert_multiple_files() {
        let cli = Cli::parse_from(["ruddydoc", "convert", "a.md", "b.html", "c.pdf"]);
        match cli.command {
            Commands::Convert(args) => {
                assert_eq!(args.files.len(), 3);
            }
            _ => panic!("expected Convert command"),
        }
    }

    #[test]
    fn parse_convert_with_output_dir() {
        let cli = Cli::parse_from([
            "ruddydoc", "convert", "a.md", "b.md", "--output", "/tmp/out",
        ]);
        match cli.command {
            Commands::Convert(args) => {
                assert_eq!(args.output, Some(PathBuf::from("/tmp/out")));
            }
            _ => panic!("expected Convert command"),
        }
    }

    #[test]
    fn parse_convert_with_format() {
        let cli = Cli::parse_from(["ruddydoc", "convert", "doc.md", "--format", "markdown"]);
        match cli.command {
            Commands::Convert(args) => {
                assert!(matches!(args.format, OutputFormatArg::Markdown));
            }
            _ => panic!("expected Convert command"),
        }
    }

    #[test]
    fn parse_convert_with_max_file_size() {
        let cli = Cli::parse_from(["ruddydoc", "convert", "doc.md", "--max-file-size", "1024"]);
        match cli.command {
            Commands::Convert(args) => {
                assert_eq!(args.max_file_size, Some(1024));
            }
            _ => panic!("expected Convert command"),
        }
    }

    // -----------------------------------------------------------------------
    // CLI parsing: all output format variants
    // -----------------------------------------------------------------------

    #[test]
    fn parse_all_output_formats() {
        let formats = [
            ("json", OutputFormat::Json),
            ("markdown", OutputFormat::Markdown),
            ("html", OutputFormat::Html),
            ("text", OutputFormat::Text),
            ("turtle", OutputFormat::Turtle),
            ("ntriples", OutputFormat::NTriples),
            ("jsonld", OutputFormat::JsonLd),
            ("rdfxml", OutputFormat::RdfXml),
            ("doctags", OutputFormat::DocTags),
            ("webvtt", OutputFormat::WebVtt),
        ];

        for (name, expected) in &formats {
            let cli = Cli::parse_from(["ruddydoc", "convert", "doc.md", "--format", name]);
            match cli.command {
                Commands::Convert(args) => {
                    assert_eq!(
                        args.format.to_output_format(),
                        *expected,
                        "format arg '{name}' should map to {expected}"
                    );
                }
                _ => panic!("expected Convert command"),
            }
        }
    }

    // -----------------------------------------------------------------------
    // CLI parsing: query
    // -----------------------------------------------------------------------

    #[test]
    fn parse_query_basic() {
        let cli = Cli::parse_from([
            "ruddydoc",
            "query",
            "SELECT ?s WHERE { ?s ?p ?o }",
            "doc.md",
        ]);
        match cli.command {
            Commands::Query(args) => {
                assert_eq!(args.sparql, "SELECT ?s WHERE { ?s ?p ?o }");
                assert_eq!(args.files.len(), 1);
                assert!(matches!(args.format, QueryOutputFormat::Json));
            }
            _ => panic!("expected Query command"),
        }
    }

    #[test]
    fn parse_query_multiple_files() {
        let cli = Cli::parse_from([
            "ruddydoc",
            "query",
            "SELECT ?s WHERE { ?s ?p ?o }",
            "a.md",
            "b.md",
        ]);
        match cli.command {
            Commands::Query(args) => {
                assert_eq!(args.files.len(), 2);
            }
            _ => panic!("expected Query command"),
        }
    }

    #[test]
    fn parse_query_table_format() {
        let cli = Cli::parse_from([
            "ruddydoc",
            "query",
            "SELECT ?s WHERE { ?s ?p ?o }",
            "doc.md",
            "--format",
            "table",
        ]);
        match cli.command {
            Commands::Query(args) => {
                assert!(matches!(args.format, QueryOutputFormat::Table));
            }
            _ => panic!("expected Query command"),
        }
    }

    // -----------------------------------------------------------------------
    // CLI parsing: chunk
    // -----------------------------------------------------------------------

    #[test]
    fn parse_chunk_basic() {
        let cli = Cli::parse_from(["ruddydoc", "chunk", "doc.md"]);
        match cli.command {
            Commands::Chunk(args) => {
                assert_eq!(args.files.len(), 1);
                assert_eq!(args.max_tokens, 512);
                assert!(args.include_headings);
                assert!(args.merge_lists);
            }
            _ => panic!("expected Chunk command"),
        }
    }

    #[test]
    fn parse_chunk_with_options() {
        let cli = Cli::parse_from([
            "ruddydoc",
            "chunk",
            "doc.md",
            "--max-tokens",
            "256",
            "--include-headings",
            "false",
            "--merge-lists",
            "false",
        ]);
        match cli.command {
            Commands::Chunk(args) => {
                assert_eq!(args.max_tokens, 256);
                assert!(!args.include_headings);
                assert!(!args.merge_lists);
            }
            _ => panic!("expected Chunk command"),
        }
    }

    #[test]
    fn parse_chunk_multiple_files() {
        let cli = Cli::parse_from(["ruddydoc", "chunk", "a.md", "b.md"]);
        match cli.command {
            Commands::Chunk(args) => {
                assert_eq!(args.files.len(), 2);
            }
            _ => panic!("expected Chunk command"),
        }
    }

    // -----------------------------------------------------------------------
    // CLI parsing: serve
    // -----------------------------------------------------------------------

    #[test]
    fn parse_serve_defaults() {
        let cli = Cli::parse_from(["ruddydoc", "serve"]);
        match cli.command {
            Commands::Serve(args) => {
                assert_eq!(args.port, 8080);
                assert!(!args.mcp);
            }
            _ => panic!("expected Serve command"),
        }
    }

    #[test]
    fn parse_serve_with_port_and_mcp() {
        let cli = Cli::parse_from(["ruddydoc", "serve", "--port", "3000", "--mcp"]);
        match cli.command {
            Commands::Serve(args) => {
                assert_eq!(args.port, 3000);
                assert!(args.mcp);
            }
            _ => panic!("expected Serve command"),
        }
    }

    // -----------------------------------------------------------------------
    // CLI parsing: models
    // -----------------------------------------------------------------------

    #[test]
    fn parse_models_list() {
        let cli = Cli::parse_from(["ruddydoc", "models", "list"]);
        match cli.command {
            Commands::Models(args) => {
                assert!(matches!(args.action, ModelsAction::List));
            }
            _ => panic!("expected Models command"),
        }
    }

    #[test]
    fn parse_models_download() {
        let cli = Cli::parse_from(["ruddydoc", "models", "download", "layout-analysis"]);
        match cli.command {
            Commands::Models(args) => match args.action {
                ModelsAction::Download { model } => {
                    assert_eq!(model, "layout-analysis");
                }
                _ => panic!("expected Download action"),
            },
            _ => panic!("expected Models command"),
        }
    }

    // -----------------------------------------------------------------------
    // CLI parsing: info
    // -----------------------------------------------------------------------

    #[test]
    fn parse_info() {
        let cli = Cli::parse_from(["ruddydoc", "info", "doc.pdf"]);
        match cli.command {
            Commands::Info { input } => {
                assert_eq!(input, PathBuf::from("doc.pdf"));
            }
            _ => panic!("expected Info command"),
        }
    }

    // -----------------------------------------------------------------------
    // CLI parsing: formats
    // -----------------------------------------------------------------------

    #[test]
    fn parse_formats() {
        let cli = Cli::parse_from(["ruddydoc", "formats"]);
        assert!(matches!(cli.command, Commands::Formats));
    }

    // -----------------------------------------------------------------------
    // Output format mapping completeness
    // -----------------------------------------------------------------------

    #[test]
    fn output_format_arg_covers_all_variants() {
        // Ensure every OutputFormatArg maps to a valid OutputFormat.
        let all_args = [
            OutputFormatArg::Json,
            OutputFormatArg::Markdown,
            OutputFormatArg::Html,
            OutputFormatArg::Text,
            OutputFormatArg::Turtle,
            OutputFormatArg::Ntriples,
            OutputFormatArg::JsonLd,
            OutputFormatArg::RdfXml,
            OutputFormatArg::DocTags,
            OutputFormatArg::WebVtt,
        ];
        for arg in &all_args {
            // Should not panic.
            let _ = arg.to_output_format();
        }
    }

    // -----------------------------------------------------------------------
    // output_format_extension
    // -----------------------------------------------------------------------

    #[test]
    fn format_extensions_are_nonempty() {
        let formats = [
            OutputFormat::Json,
            OutputFormat::Markdown,
            OutputFormat::Html,
            OutputFormat::Text,
            OutputFormat::Turtle,
            OutputFormat::NTriples,
            OutputFormat::JsonLd,
            OutputFormat::RdfXml,
            OutputFormat::DocTags,
            OutputFormat::WebVtt,
        ];
        for fmt in &formats {
            let ext = output_format_extension(*fmt);
            assert!(!ext.is_empty(), "extension for {fmt} should not be empty");
        }
    }

    // -----------------------------------------------------------------------
    // print_results_table
    // -----------------------------------------------------------------------

    #[test]
    fn print_table_empty_results() {
        // Should not panic.
        print_results_table(&serde_json::Value::Array(vec![]));
    }

    #[test]
    fn print_table_boolean_result() {
        // Should not panic.
        print_results_table(&serde_json::Value::Bool(true));
    }

    #[test]
    fn print_table_with_rows() {
        // Should not panic.
        let rows = serde_json::json!([
            {"name": "Alice", "age": "30"},
            {"name": "Bob", "age": "25"},
        ]);
        print_results_table(&rows);
    }

    // -----------------------------------------------------------------------
    // serve stub message
    // -----------------------------------------------------------------------

    #[test]
    fn serve_args_defaults() {
        let args = ServeArgs {
            port: 8080,
            mcp: false,
        };
        assert_eq!(args.port, 8080);
        assert!(!args.mcp);
    }

    #[test]
    fn serve_args_mcp() {
        let args = ServeArgs {
            port: 9090,
            mcp: true,
        };
        assert_eq!(args.port, 9090);
        assert!(args.mcp);
    }

    // -----------------------------------------------------------------------
    // Negative: missing required args
    // -----------------------------------------------------------------------

    #[test]
    fn parse_convert_no_files_fails() {
        let result = Cli::try_parse_from(["ruddydoc", "convert"]);
        assert!(result.is_err(), "convert with no files should fail");
    }

    #[test]
    fn parse_query_no_files_fails() {
        let result = Cli::try_parse_from(["ruddydoc", "query", "SELECT ?s WHERE { ?s ?p ?o }"]);
        assert!(result.is_err(), "query with no files should fail");
    }

    #[test]
    fn parse_chunk_no_files_fails() {
        let result = Cli::try_parse_from(["ruddydoc", "chunk"]);
        assert!(result.is_err(), "chunk with no files should fail");
    }

    #[test]
    fn parse_invalid_format_fails() {
        let result =
            Cli::try_parse_from(["ruddydoc", "convert", "doc.md", "--format", "badformat"]);
        assert!(result.is_err(), "invalid format should fail");
    }
}
