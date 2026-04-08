//! Core types, traits, and error handling for RuddyDoc.
//!
//! This crate is the leaf of the dependency graph. Every other RuddyDoc crate
//! depends on it. It defines the shared vocabulary: input/output formats,
//! document metadata, backend and exporter traits, and the unified error type.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error type (following beret convention: Box<dyn std::error::Error>)
// ---------------------------------------------------------------------------

/// Unified error type for all RuddyDoc operations.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Convenience alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

// ---------------------------------------------------------------------------
// Input formats (matching Python docling's 17 formats)
// ---------------------------------------------------------------------------

/// All document input formats supported by RuddyDoc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputFormat {
    /// Markdown (.md, .markdown)
    Markdown,
    /// HTML (.html, .htm, .xhtml)
    Html,
    /// CSV / TSV (.csv, .tsv)
    Csv,
    /// Microsoft Word (.docx)
    Docx,
    /// PDF (.pdf)
    Pdf,
    /// LaTeX (.tex, .latex)
    Latex,
    /// Microsoft PowerPoint (.pptx)
    Pptx,
    /// Microsoft Excel (.xlsx, .xls)
    Xlsx,
    /// Image files (.png, .jpg, .jpeg, .tiff, .bmp, .webp)
    Image,
    /// XML (generic, JATS, USPTO) (.xml)
    Xml,
    /// WebVTT subtitles (.vtt)
    WebVtt,
    /// AsciiDoc (.adoc, .asciidoc, .asc)
    AsciiDoc,
    /// JSON (docling-format round-trip) (.json)
    Json,
    /// Plain text (.txt)
    Text,
    /// XBRL financial reporting (.xbrl)
    Xbrl,
    /// EPUB (.epub)
    Epub,
    /// RTF (.rtf)
    Rtf,
}

impl InputFormat {
    /// Return the canonical file extensions for this format.
    pub fn extensions(&self) -> &[&str] {
        match self {
            Self::Markdown => &["md", "markdown"],
            Self::Html => &["html", "htm", "xhtml"],
            Self::Csv => &["csv", "tsv"],
            Self::Docx => &["docx"],
            Self::Pdf => &["pdf"],
            Self::Latex => &["tex", "latex"],
            Self::Pptx => &["pptx"],
            Self::Xlsx => &["xlsx", "xls"],
            Self::Image => &["png", "jpg", "jpeg", "tiff", "tif", "bmp", "webp"],
            Self::Xml => &["xml"],
            Self::WebVtt => &["vtt"],
            Self::AsciiDoc => &["adoc", "asciidoc", "asc"],
            Self::Json => &["json"],
            Self::Text => &["txt"],
            Self::Xbrl => &["xbrl"],
            Self::Epub => &["epub"],
            Self::Rtf => &["rtf"],
        }
    }

    /// Return the primary MIME type for this format.
    pub fn mime_type(&self) -> &str {
        match self {
            Self::Markdown => "text/markdown",
            Self::Html => "text/html",
            Self::Csv => "text/csv",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Pdf => "application/pdf",
            Self::Latex => "application/x-latex",
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Self::Image => "image/png",
            Self::Xml => "application/xml",
            Self::WebVtt => "text/vtt",
            Self::AsciiDoc => "text/asciidoc",
            Self::Json => "application/json",
            Self::Text => "text/plain",
            Self::Xbrl => "application/xbrl+xml",
            Self::Epub => "application/epub+zip",
            Self::Rtf => "application/rtf",
        }
    }
}

impl std::fmt::Display for InputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Markdown => "Markdown",
            Self::Html => "HTML",
            Self::Csv => "CSV",
            Self::Docx => "DOCX",
            Self::Pdf => "PDF",
            Self::Latex => "LaTeX",
            Self::Pptx => "PPTX",
            Self::Xlsx => "XLSX",
            Self::Image => "Image",
            Self::Xml => "XML",
            Self::WebVtt => "WebVTT",
            Self::AsciiDoc => "AsciiDoc",
            Self::Json => "JSON",
            Self::Text => "Text",
            Self::Xbrl => "XBRL",
            Self::Epub => "EPUB",
            Self::Rtf => "RTF",
        };
        write!(f, "{name}")
    }
}

// ---------------------------------------------------------------------------
// Output formats
// ---------------------------------------------------------------------------

/// Export output formats supported by RuddyDoc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    /// Markdown
    Markdown,
    /// HTML
    Html,
    /// JSON (docling-compatible schema)
    Json,
    /// Plain text
    Text,
    /// DocTags (docling's tagged format)
    DocTags,
    /// WebVTT subtitles
    WebVtt,
    /// RDF Turtle serialization
    Turtle,
    /// RDF N-Triples serialization
    NTriples,
    /// JSON-LD (schema.org-compatible linked data)
    JsonLd,
    /// RDF/XML serialization
    RdfXml,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Markdown => "Markdown",
            Self::Html => "HTML",
            Self::Json => "JSON",
            Self::Text => "Text",
            Self::DocTags => "DocTags",
            Self::WebVtt => "WebVTT",
            Self::Turtle => "Turtle",
            Self::NTriples => "N-Triples",
            Self::JsonLd => "JSON-LD",
            Self::RdfXml => "RDF/XML",
        };
        write!(f, "{name}")
    }
}

// ---------------------------------------------------------------------------
// Conversion status
// ---------------------------------------------------------------------------

/// Status of a document conversion operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionStatus {
    /// Conversion has not yet started.
    Pending,
    /// Conversion is in progress.
    Started,
    /// Conversion completed successfully.
    Success,
    /// Conversion completed with some elements skipped or degraded.
    PartialSuccess,
    /// Conversion failed entirely.
    Failure,
    /// Conversion was skipped (e.g., unsupported format).
    Skipped,
}

// ---------------------------------------------------------------------------
// Document hash
// ---------------------------------------------------------------------------

/// A content-addressable hash identifying a document.
///
/// Used to construct the document's named graph IRI:
/// `urn:ruddydoc:doc:{hash}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentHash(pub String);

impl std::fmt::Display for DocumentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Bounding box
// ---------------------------------------------------------------------------

/// A bounding box for a document element on a page.
///
/// Coordinates are in points (1/72 inch) from the top-left corner of the page.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Left edge (x-min).
    pub left: f64,
    /// Top edge (y-min).
    pub top: f64,
    /// Right edge (x-max).
    pub right: f64,
    /// Bottom edge (y-max).
    pub bottom: f64,
}

// ---------------------------------------------------------------------------
// Document metadata
// ---------------------------------------------------------------------------

/// Metadata about a parsed document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    /// Original file path, if the source was a file.
    pub file_path: Option<PathBuf>,
    /// Content-addressable hash of the document.
    pub hash: DocumentHash,
    /// Detected input format.
    pub format: InputFormat,
    /// File size in bytes.
    pub file_size: u64,
    /// Number of pages (for paginated formats like PDF, PPTX).
    pub page_count: Option<u32>,
}

// ---------------------------------------------------------------------------
// Document source
// ---------------------------------------------------------------------------

/// The source of a document to be parsed.
#[derive(Debug, Clone)]
pub enum DocumentSource {
    /// A file on disk.
    File(PathBuf),
    /// An in-memory byte stream with a name.
    Stream {
        /// Display name for the stream (e.g., original filename).
        name: String,
        /// Raw document bytes.
        data: Vec<u8>,
    },
}

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// Trait implemented by each format-specific parser backend.
///
/// A backend takes a `DocumentSource`, parses it, and inserts RDF triples
/// into the document store. Each backend is responsible for mapping its
/// format's structure to the RuddyDoc document ontology.
pub trait DocumentBackend: Send + Sync {
    /// Return the input formats this backend can handle.
    fn supported_formats(&self) -> &[InputFormat];

    /// Whether this backend produces paginated output (e.g., PDF, PPTX).
    fn supports_pagination(&self) -> bool;

    /// Quick validation: can this backend handle the given source?
    fn is_valid(&self, source: &DocumentSource) -> bool;

    /// Parse the source document and insert triples into the store.
    ///
    /// The `doc_graph` parameter is the named graph IRI for this document
    /// (e.g., `urn:ruddydoc:doc:{hash}`). All triples for this document
    /// should be inserted into this named graph.
    fn parse(
        &self,
        source: &DocumentSource,
        store: &dyn DocumentStore,
        doc_graph: &str,
    ) -> Result<DocumentMeta>;
}

// ---------------------------------------------------------------------------
// Document store trait (abstraction over Oxigraph)
// ---------------------------------------------------------------------------

/// Trait abstracting the RDF triple store.
///
/// This ensures that no crate other than `ruddydoc-graph` depends directly
/// on Oxigraph. Backends and exporters interact with the store through this
/// trait only.
pub trait DocumentStore: Send + Sync {
    /// Insert a triple (subject, predicate, object are all IRIs) into the
    /// default graph.
    fn insert_triple(&self, subject: &str, predicate: &str, object: &str) -> Result<()>;

    /// Insert a triple into a named graph.
    fn insert_triple_into(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        graph: &str,
    ) -> Result<()>;

    /// Insert a literal value with a datatype into a named graph.
    fn insert_literal(
        &self,
        subject: &str,
        predicate: &str,
        value: &str,
        datatype: &str,
        graph: &str,
    ) -> Result<()>;

    /// Execute a SPARQL SELECT or ASK query and return results as JSON.
    fn query_to_json(&self, sparql: &str) -> Result<serde_json::Value>;

    /// Clear all triples in the store.
    fn clear(&self) -> Result<()>;

    /// Clear all triples in a specific named graph.
    fn clear_graph(&self, graph: &str) -> Result<()>;

    /// Serialize a named graph in the given RDF format.
    fn serialize_graph(&self, graph: &str, format: &str) -> Result<String>;

    /// Count all triples in the store.
    fn triple_count(&self) -> Result<usize>;

    /// Count triples in a specific named graph.
    fn triple_count_in(&self, graph: &str) -> Result<usize>;
}

// ---------------------------------------------------------------------------
// Exporter trait
// ---------------------------------------------------------------------------

/// Trait implemented by each output format exporter.
///
/// An exporter queries the document graph via SPARQL and produces a string
/// representation in its output format.
pub trait DocumentExporter: Send + Sync {
    /// The output format this exporter produces.
    fn format(&self) -> OutputFormat;

    /// Export a document from the store.
    ///
    /// The `doc_graph` parameter identifies which named graph contains the
    /// document to export.
    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> Result<String>;
}

// ---------------------------------------------------------------------------
// Format detection
// ---------------------------------------------------------------------------

/// Detect the input format of a file from its extension.
///
/// Returns `None` if the extension is not recognized. For more robust
/// detection (magic bytes, content sniffing), use `ruddydoc-converter`.
pub fn detect_format(path: &Path) -> Option<InputFormat> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    detect_format_from_extension(&ext)
}

/// Detect the input format from a file extension string (without the dot).
pub fn detect_format_from_extension(ext: &str) -> Option<InputFormat> {
    EXTENSION_MAP.iter().find_map(|(extension, format)| {
        if ext.eq_ignore_ascii_case(extension) {
            Some(*format)
        } else {
            None
        }
    })
}

/// Detect the input format from a MIME type string.
pub fn detect_format_from_mime(mime: &str) -> Option<InputFormat> {
    MIME_MAP.iter().find_map(|(mime_type, format)| {
        if mime.eq_ignore_ascii_case(mime_type) {
            Some(*format)
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// MIME type and extension mapping tables
// ---------------------------------------------------------------------------

/// Mapping from file extensions to input formats.
const EXTENSION_MAP: &[(&str, InputFormat)] = &[
    ("md", InputFormat::Markdown),
    ("markdown", InputFormat::Markdown),
    ("html", InputFormat::Html),
    ("htm", InputFormat::Html),
    ("xhtml", InputFormat::Html),
    ("csv", InputFormat::Csv),
    ("tsv", InputFormat::Csv),
    ("docx", InputFormat::Docx),
    ("pdf", InputFormat::Pdf),
    ("tex", InputFormat::Latex),
    ("latex", InputFormat::Latex),
    ("pptx", InputFormat::Pptx),
    ("xlsx", InputFormat::Xlsx),
    ("xls", InputFormat::Xlsx),
    ("png", InputFormat::Image),
    ("jpg", InputFormat::Image),
    ("jpeg", InputFormat::Image),
    ("tiff", InputFormat::Image),
    ("tif", InputFormat::Image),
    ("bmp", InputFormat::Image),
    ("webp", InputFormat::Image),
    ("xml", InputFormat::Xml),
    ("vtt", InputFormat::WebVtt),
    ("adoc", InputFormat::AsciiDoc),
    ("asciidoc", InputFormat::AsciiDoc),
    ("asc", InputFormat::AsciiDoc),
    ("json", InputFormat::Json),
    ("txt", InputFormat::Text),
    ("xbrl", InputFormat::Xbrl),
    ("epub", InputFormat::Epub),
    ("rtf", InputFormat::Rtf),
];

/// Mapping from MIME types to input formats.
const MIME_MAP: &[(&str, InputFormat)] = &[
    ("text/markdown", InputFormat::Markdown),
    ("text/html", InputFormat::Html),
    ("application/xhtml+xml", InputFormat::Html),
    ("text/csv", InputFormat::Csv),
    ("text/tab-separated-values", InputFormat::Csv),
    (
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        InputFormat::Docx,
    ),
    ("application/pdf", InputFormat::Pdf),
    ("application/x-latex", InputFormat::Latex),
    ("text/x-tex", InputFormat::Latex),
    (
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        InputFormat::Pptx,
    ),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        InputFormat::Xlsx,
    ),
    ("application/vnd.ms-excel", InputFormat::Xlsx),
    ("image/png", InputFormat::Image),
    ("image/jpeg", InputFormat::Image),
    ("image/tiff", InputFormat::Image),
    ("image/bmp", InputFormat::Image),
    ("image/webp", InputFormat::Image),
    ("application/xml", InputFormat::Xml),
    ("text/xml", InputFormat::Xml),
    ("text/vtt", InputFormat::WebVtt),
    ("text/asciidoc", InputFormat::AsciiDoc),
    ("application/json", InputFormat::Json),
    ("text/plain", InputFormat::Text),
    ("application/xbrl+xml", InputFormat::Xbrl),
    ("application/epub+zip", InputFormat::Epub),
    ("application/rtf", InputFormat::Rtf),
];

// ---------------------------------------------------------------------------
// IRI construction helpers
// ---------------------------------------------------------------------------

/// Construct a document named graph IRI from a hash.
pub fn doc_iri(hash: &str) -> String {
    format!("urn:ruddydoc:doc:{hash}")
}

/// Construct an element IRI within a document graph.
pub fn element_iri(hash: &str, id: &str) -> String {
    format!("urn:ruddydoc:doc:{hash}/{id}")
}

/// Construct an ontology term IRI.
pub fn ontology_iri(term: &str) -> String {
    format!("https://ruddydoc.chapeaux.io/ontology#{term}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_markdown() {
        let path = Path::new("readme.md");
        assert_eq!(detect_format(path), Some(InputFormat::Markdown));
    }

    #[test]
    fn detect_html() {
        let path = Path::new("index.html");
        assert_eq!(detect_format(path), Some(InputFormat::Html));
    }

    #[test]
    fn detect_unknown_extension() {
        let path = Path::new("file.xyz");
        assert_eq!(detect_format(path), None);
    }

    #[test]
    fn detect_format_case_insensitive() {
        assert_eq!(detect_format_from_extension("PDF"), Some(InputFormat::Pdf));
    }

    #[test]
    fn detect_from_mime() {
        assert_eq!(
            detect_format_from_mime("application/pdf"),
            Some(InputFormat::Pdf)
        );
    }

    #[test]
    fn doc_iri_format() {
        assert_eq!(doc_iri("abc123"), "urn:ruddydoc:doc:abc123");
    }

    #[test]
    fn element_iri_format() {
        assert_eq!(
            element_iri("abc123", "heading-0"),
            "urn:ruddydoc:doc:abc123/heading-0"
        );
    }

    #[test]
    fn ontology_iri_format() {
        assert_eq!(
            ontology_iri("Document"),
            "https://ruddydoc.chapeaux.io/ontology#Document"
        );
    }
}
