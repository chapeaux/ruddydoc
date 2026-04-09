//! Format detection and backend dispatch for RuddyDoc.
//!
//! The converter auto-detects input formats using a multi-layered strategy
//! (file extension, magic bytes, content sniffing, ZIP inspection) and
//! dispatches to the appropriate backend for parsing.

mod detect;
mod registry;

pub use detect::{detect_format_from_bytes, detect_format_full};
pub use registry::BackendRegistry;

use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tracing::debug;

use ruddydoc_core::{ConversionStatus, DocumentMeta, DocumentSource, DocumentStore, InputFormat};
use ruddydoc_graph::OxigraphStore;
use ruddydoc_ontology as ont;

/// Result of a document conversion operation.
pub struct ConversionResult {
    /// Metadata about the input document.
    pub input: DocumentMeta,
    /// Status of the conversion.
    pub status: ConversionStatus,
    /// Named graph IRI containing the parsed document.
    pub doc_graph: String,
    /// Reference to the store holding the parsed RDF data.
    pub store: Arc<OxigraphStore>,
}

/// A language variant for a translation group.
#[derive(Debug, Clone)]
pub struct LanguageVariant {
    /// The document source for this language variant.
    pub source: DocumentSource,
    /// BCP 47 language tag (e.g., "en", "fr", "zh-Hans").
    pub language: String,
}

/// Configuration for a set of translated documents.
#[derive(Debug, Clone)]
pub struct TranslationGroupConfig {
    /// Unique identifier for this translation group.
    pub group_id: String,
    /// The language variants in this group.
    pub variants: Vec<LanguageVariant>,
}

/// Result of converting a translation group.
pub struct TranslationGroupResult {
    /// The group identifier.
    pub group_id: String,
    /// The IRI of the TranslationGroup node in the metadata graph.
    pub group_iri: String,
    /// Conversion results for each language variant.
    pub variants: Vec<ConversionResult>,
}

/// Options controlling how documents are converted.
#[derive(Default)]
pub struct ConvertOptions {
    /// Maximum file size in bytes.
    pub max_file_size: Option<u64>,
    /// Maximum number of pages to process.
    pub max_pages: Option<u32>,
}

/// The main document converter.
///
/// Selects the appropriate backend based on format detection and
/// dispatches parsing through the pipeline.
pub struct DocumentConverter {
    options: ConvertOptions,
    registry: BackendRegistry,
}

/// Compute a SHA-256 hash of the given bytes, returning a hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Read the content bytes from a `DocumentSource`.
fn read_source_bytes(source: &DocumentSource) -> ruddydoc_core::Result<Vec<u8>> {
    match source {
        DocumentSource::File(path) => {
            let mut file = std::fs::File::open(path)?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            Ok(buf)
        }
        DocumentSource::Stream { data, .. } => Ok(data.clone()),
    }
}

/// Extract the display name from a `DocumentSource`.
fn source_name(source: &DocumentSource) -> String {
    match source {
        DocumentSource::File(path) => path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string()),
        DocumentSource::Stream { name, .. } => name.clone(),
    }
}

/// Extract the file path from a `DocumentSource`, if it is a file.
fn source_path(source: &DocumentSource) -> Option<PathBuf> {
    match source {
        DocumentSource::File(path) => Some(path.clone()),
        DocumentSource::Stream { .. } => None,
    }
}

impl DocumentConverter {
    /// Create a new converter with the given options.
    pub fn new(options: ConvertOptions) -> Self {
        Self {
            options,
            registry: BackendRegistry::new(),
        }
    }

    /// Create a converter with default options.
    pub fn default_converter() -> Self {
        Self::new(ConvertOptions::default())
    }

    /// Return the backend registry.
    pub fn registry(&self) -> &BackendRegistry {
        &self.registry
    }

    /// Detect the input format of a document source.
    ///
    /// Uses the full detection strategy: extension, magic bytes, content
    /// sniffing, and ZIP inspection.
    pub fn detect_format(source: &DocumentSource) -> Option<InputFormat> {
        match source {
            DocumentSource::File(path) => {
                // Try extension first for speed
                if let Some(fmt) = ruddydoc_core::detect_format(path) {
                    return Some(fmt);
                }
                // Fall back to reading bytes
                if let Ok(mut file) = std::fs::File::open(path) {
                    let mut buf = vec![0u8; 8192];
                    if let Ok(n) = file.read(&mut buf) {
                        buf.truncate(n);
                        return detect_format_full(None, &buf);
                    }
                }
                None
            }
            DocumentSource::Stream { name, data } => {
                let path = std::path::Path::new(name);
                let ext_format = ruddydoc_core::detect_format(path);
                detect_format_full(ext_format, data).or(ext_format)
            }
        }
    }

    /// Convert a single document.
    ///
    /// 1. Reads the source bytes
    /// 2. Detects the format (extension / magic bytes / content sniffing)
    /// 3. Finds the appropriate backend
    /// 4. Creates a content-hash-based named graph IRI
    /// 5. Creates an `OxigraphStore` and loads the ontology
    /// 6. Calls the backend's `parse()` method
    /// 7. Returns a `ConversionResult` with a reference to the store
    pub fn convert(&self, source: DocumentSource) -> ruddydoc_core::Result<ConversionResult> {
        let bytes = read_source_bytes(&source)?;
        let file_size = bytes.len() as u64;

        // Check file size limit
        if let Some(max_size) = self.options.max_file_size
            && file_size > max_size
        {
            return Err(
                format!("file size {file_size} exceeds maximum allowed size {max_size}").into(),
            );
        }

        // Detect format
        let format = Self::detect_format(&source)
            .ok_or_else(|| format!("could not detect format for '{}'", source_name(&source)))?;
        debug!(format = %format, name = %source_name(&source), "detected format");

        // Find backend
        let backend = self
            .registry
            .backend_for(format)
            .ok_or_else(|| format!("no backend registered for format '{format}'"))?;

        // Compute content hash and named graph IRI
        let hash_str = compute_hash(&bytes);
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        // Create store and load ontology
        let store = Arc::new(OxigraphStore::new()?);
        ruddydoc_ontology::load_ontology(store.as_ref())?;

        // Build the source for parsing (use the bytes we already read)
        let parse_source = DocumentSource::Stream {
            name: source_name(&source),
            data: bytes,
        };

        // Parse
        match backend.parse(&parse_source, store.as_ref(), &doc_graph) {
            Ok(meta) => Ok(ConversionResult {
                input: meta,
                status: ConversionStatus::Success,
                doc_graph,
                store,
            }),
            Err(e) => {
                // Return a result with Failure status and minimal metadata
                let meta = DocumentMeta {
                    file_path: source_path(&source),
                    hash: ruddydoc_core::DocumentHash(hash_str),
                    format,
                    file_size,
                    page_count: None,
                    language: None,
                };
                debug!(error = %e, "backend parse failed, returning failure status");
                Ok(ConversionResult {
                    input: meta,
                    status: ConversionStatus::Failure,
                    doc_graph,
                    store,
                })
            }
        }
    }

    /// Set the `rdoc:language` property on the document node within a
    /// conversion result.
    ///
    /// This is used by the CLI when `--language` is provided, or when
    /// processing a translation manifest.
    pub fn set_language(result: &ConversionResult, language: &str) -> ruddydoc_core::Result<()> {
        result.store.insert_literal(
            &result.doc_graph,
            &ont::iri(ont::PROP_LANGUAGE),
            language,
            "string",
            &result.doc_graph,
        )
    }

    /// Convert a translation group: multiple language variants of the
    /// same document.
    ///
    /// Each variant is converted independently using `convert()`. A
    /// `rdoc:TranslationGroup` node is created in a metadata graph and
    /// linked to each document via `rdoc:hasTranslation` /
    /// `rdoc:translationGroup`. The `rdoc:language` property is set on
    /// each document node.
    ///
    /// Note: each variant gets its own `OxigraphStore` because
    /// `convert()` creates one per document. The TranslationGroup node
    /// is inserted into the **first** variant's store for convenience.
    pub fn convert_translation_group(
        &self,
        config: TranslationGroupConfig,
    ) -> ruddydoc_core::Result<TranslationGroupResult> {
        let mut variant_results = Vec::new();

        for variant in &config.variants {
            let result = self.convert(variant.source.clone())?;

            // Set rdoc:language on the document node
            Self::set_language(&result, &variant.language)?;

            variant_results.push(result);
        }

        // Create TranslationGroup node in the first variant's store
        let group_iri = format!("urn:ruddydoc:translation:{}", config.group_id);
        let rdf_type = ont::rdf_iri("type");

        if let Some(first) = variant_results.first() {
            let meta_graph = &first.doc_graph;

            // Create the TranslationGroup node
            first.store.insert_triple_into(
                &group_iri,
                &rdf_type,
                &ont::iri(ont::CLASS_TRANSLATION_GROUP),
                meta_graph,
            )?;

            // Link group <-> each document
            for vr in &variant_results {
                first.store.insert_triple_into(
                    &group_iri,
                    &ont::iri(ont::PROP_HAS_TRANSLATION),
                    &vr.doc_graph,
                    meta_graph,
                )?;
                // Each document references back to its translation group
                // (stored in its own graph for per-document queries).
                vr.store.insert_triple_into(
                    &vr.doc_graph,
                    &ont::iri(ont::PROP_TRANSLATION_GROUP),
                    &group_iri,
                    &vr.doc_graph,
                )?;
            }
        }

        Ok(TranslationGroupResult {
            group_id: config.group_id,
            group_iri,
            variants: variant_results,
        })
    }

    /// Convert a batch of documents.
    ///
    /// Each document is converted independently. Errors in one document
    /// do not affect others.
    pub fn convert_batch(
        &self,
        sources: Vec<DocumentSource>,
    ) -> Vec<ruddydoc_core::Result<ConversionResult>> {
        sources.into_iter().map(|s| self.convert(s)).collect()
    }

    /// Return information about a file without performing full conversion.
    ///
    /// Detects the format and returns file metadata.
    pub fn file_info(source: &DocumentSource) -> ruddydoc_core::Result<FileInfo> {
        let bytes = read_source_bytes(source)?;
        let file_size = bytes.len() as u64;
        let format = Self::detect_format(source);
        let hash_str = compute_hash(&bytes);
        let name = source_name(source);
        let path = source_path(source);

        Ok(FileInfo {
            name,
            path,
            format,
            file_size,
            hash: hash_str,
        })
    }
}

/// Information about a file without full conversion.
#[derive(Debug)]
pub struct FileInfo {
    /// Display name of the file.
    pub name: String,
    /// File path, if from disk.
    pub path: Option<PathBuf>,
    /// Detected format, if any.
    pub format: Option<InputFormat>,
    /// File size in bytes.
    pub file_size: u64,
    /// Content hash.
    pub hash: String,
}

impl std::fmt::Display for FileInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "File: {}", self.name)?;
        if let Some(path) = &self.path {
            writeln!(f, "Path: {}", path.display())?;
        }
        match self.format {
            Some(fmt) => writeln!(f, "Format: {fmt}")?,
            None => writeln!(f, "Format: unknown")?,
        }
        writeln!(f, "Size: {} bytes", self.file_size)?;
        writeln!(f, "Hash: {}", self.hash)?;
        Ok(())
    }
}

/// List all supported input formats with their extensions and MIME types.
pub fn list_supported_formats() -> Vec<FormatInfo> {
    use InputFormat::*;
    let all = [
        Markdown, Html, Csv, Docx, Pdf, Latex, Pptx, Xlsx, Image, Xml, WebVtt, AsciiDoc, Json,
        Text, Xbrl, Epub, Rtf,
    ];
    all.iter()
        .map(|fmt| FormatInfo {
            format: *fmt,
            extensions: fmt.extensions().iter().map(|s| (*s).to_string()).collect(),
            mime_type: fmt.mime_type().to_string(),
        })
        .collect()
}

/// Information about a supported format.
#[derive(Debug)]
pub struct FormatInfo {
    /// The format.
    pub format: InputFormat,
    /// Recognized file extensions.
    pub extensions: Vec<String>,
    /// Primary MIME type.
    pub mime_type: String,
}

impl std::fmt::Display for FormatInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let exts = self
            .extensions
            .iter()
            .map(|e| format!(".{e}"))
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "{:<12} {:<50} {}", self.format, self.mime_type, exts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_core::DocumentStore;

    #[test]
    fn detect_format_markdown_file() {
        let source = DocumentSource::File(PathBuf::from("test.md"));
        let fmt = DocumentConverter::detect_format(&source);
        assert_eq!(fmt, Some(InputFormat::Markdown));
    }

    #[test]
    fn detect_format_html_file() {
        let source = DocumentSource::File(PathBuf::from("index.html"));
        let fmt = DocumentConverter::detect_format(&source);
        assert_eq!(fmt, Some(InputFormat::Html));
    }

    #[test]
    fn detect_format_stream_markdown() {
        let source = DocumentSource::Stream {
            name: "readme.md".to_string(),
            data: b"# Hello\nWorld".to_vec(),
        };
        let fmt = DocumentConverter::detect_format(&source);
        assert_eq!(fmt, Some(InputFormat::Markdown));
    }

    #[test]
    fn detect_format_stream_pdf_magic() {
        let source = DocumentSource::Stream {
            name: "document".to_string(), // no extension
            data: b"%PDF-1.4 some pdf content".to_vec(),
        };
        let fmt = DocumentConverter::detect_format(&source);
        assert_eq!(fmt, Some(InputFormat::Pdf));
    }

    #[test]
    fn detect_format_stream_html_content() {
        let source = DocumentSource::Stream {
            name: "page".to_string(), // no extension
            data: b"<!DOCTYPE html><html><head></head><body>hello</body></html>".to_vec(),
        };
        let fmt = DocumentConverter::detect_format(&source);
        assert_eq!(fmt, Some(InputFormat::Html));
    }

    #[test]
    fn convert_markdown_stream() {
        let converter = DocumentConverter::default_converter();
        let source = DocumentSource::Stream {
            name: "test.md".to_string(),
            data: b"# Hello\n\nWorld".to_vec(),
        };
        let result = converter.convert(source).unwrap();
        assert_eq!(result.status, ConversionStatus::Success);
        assert_eq!(result.input.format, InputFormat::Markdown);
        assert!(!result.doc_graph.is_empty());
        assert!(result.store.triple_count().unwrap() > 0);
    }

    #[test]
    fn convert_file_size_limit() {
        let converter = DocumentConverter::new(ConvertOptions {
            max_file_size: Some(5),
            max_pages: None,
        });
        let source = DocumentSource::Stream {
            name: "test.md".to_string(),
            data: b"# Hello this is long".to_vec(),
        };
        let result = converter.convert(source);
        assert!(result.is_err());
    }

    #[test]
    fn convert_unknown_format() {
        let converter = DocumentConverter::default_converter();
        let source = DocumentSource::Stream {
            name: "file.xyz".to_string(),
            data: b"random content".to_vec(),
        };
        let result = converter.convert(source);
        assert!(result.is_err());
    }

    #[test]
    fn convert_batch_mixed() {
        let converter = DocumentConverter::default_converter();
        let sources = vec![
            DocumentSource::Stream {
                name: "a.md".to_string(),
                data: b"# A\n\nParagraph A".to_vec(),
            },
            DocumentSource::Stream {
                name: "b.md".to_string(),
                data: b"# B\n\nParagraph B".to_vec(),
            },
        ];
        let results = converter.convert_batch(sources);
        assert_eq!(results.len(), 2);
        for r in &results {
            let result = r.as_ref().unwrap();
            assert_eq!(result.status, ConversionStatus::Success);
        }
    }

    #[test]
    fn file_info_stream() {
        let source = DocumentSource::Stream {
            name: "readme.md".to_string(),
            data: b"# Hello".to_vec(),
        };
        let info = DocumentConverter::file_info(&source).unwrap();
        assert_eq!(info.name, "readme.md");
        assert_eq!(info.format, Some(InputFormat::Markdown));
        assert_eq!(info.file_size, 7);
        assert!(!info.hash.is_empty());
    }

    #[test]
    fn list_formats_includes_all() {
        let formats = list_supported_formats();
        assert!(formats.len() >= 17);
        let names: Vec<_> = formats.iter().map(|f| f.format).collect();
        assert!(names.contains(&InputFormat::Markdown));
        assert!(names.contains(&InputFormat::Pdf));
        assert!(names.contains(&InputFormat::Html));
        assert!(names.contains(&InputFormat::Docx));
    }

    #[test]
    fn file_info_display() {
        let source = DocumentSource::Stream {
            name: "test.csv".to_string(),
            data: b"a,b,c\n1,2,3".to_vec(),
        };
        let info = DocumentConverter::file_info(&source).unwrap();
        let display = format!("{info}");
        assert!(display.contains("test.csv"));
        assert!(display.contains("CSV"));
    }

    #[test]
    fn set_language_on_document() {
        let converter = DocumentConverter::default_converter();
        let source = DocumentSource::Stream {
            name: "test.md".to_string(),
            data: b"# Hello\n\nWorld".to_vec(),
        };
        let result = converter.convert(source).unwrap();
        DocumentConverter::set_language(&result, "fr").unwrap();

        // Query rdoc:language from the document node
        let sparql = format!(
            "SELECT ?lang WHERE {{ \
               GRAPH <{g}> {{ \
                 ?doc a <{cls}>. \
                 ?doc <{prop}> ?lang \
               }} \
             }}",
            g = result.doc_graph,
            cls = ont::iri(ont::CLASS_DOCUMENT),
            prop = ont::iri(ont::PROP_LANGUAGE),
        );
        let res = result.store.query_to_json(&sparql).unwrap();
        let rows = res.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let lang = rows[0]["lang"].as_str().expect("lang");
        assert!(lang.contains("fr"));
    }

    #[test]
    fn convert_translation_group_two_variants() {
        let converter = DocumentConverter::default_converter();
        let config = TranslationGroupConfig {
            group_id: "test-group".to_string(),
            variants: vec![
                LanguageVariant {
                    source: DocumentSource::Stream {
                        name: "en.md".to_string(),
                        data: b"# Hello\n\nWorld".to_vec(),
                    },
                    language: "en".to_string(),
                },
                LanguageVariant {
                    source: DocumentSource::Stream {
                        name: "fr.md".to_string(),
                        data: b"# Bonjour\n\nMonde".to_vec(),
                    },
                    language: "fr".to_string(),
                },
            ],
        };

        let result = converter.convert_translation_group(config).unwrap();
        assert_eq!(result.group_id, "test-group");
        assert_eq!(result.group_iri, "urn:ruddydoc:translation:test-group");
        assert_eq!(result.variants.len(), 2);

        // Verify both variants converted successfully
        for vr in &result.variants {
            assert_eq!(vr.status, ConversionStatus::Success);
        }

        // Verify language was set on each document
        for (i, lang_tag) in ["en", "fr"].iter().enumerate() {
            let vr = &result.variants[i];
            let sparql = format!(
                "SELECT ?lang WHERE {{ \
                   GRAPH <{g}> {{ \
                     ?doc a <{cls}>. \
                     ?doc <{prop}> ?lang \
                   }} \
                 }}",
                g = vr.doc_graph,
                cls = ont::iri(ont::CLASS_DOCUMENT),
                prop = ont::iri(ont::PROP_LANGUAGE),
            );
            let res = vr.store.query_to_json(&sparql).unwrap();
            let rows = res.as_array().expect("expected array");
            assert_eq!(rows.len(), 1);
            let lang = rows[0]["lang"].as_str().expect("lang");
            assert!(lang.contains(lang_tag));
        }

        // Verify TranslationGroup node exists in the first variant's store
        let first = &result.variants[0];
        let sparql = format!(
            "SELECT ?grp WHERE {{ \
               GRAPH <{g}> {{ \
                 ?grp a <{cls}> \
               }} \
             }}",
            g = first.doc_graph,
            cls = ont::iri(ont::CLASS_TRANSLATION_GROUP),
        );
        let res = first.store.query_to_json(&sparql).unwrap();
        let rows = res.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        // Verify hasTranslation links exist
        let sparql_links = format!(
            "SELECT ?doc WHERE {{ \
               GRAPH <{g}> {{ \
                 <{grp_iri}> <{prop}> ?doc \
               }} \
             }}",
            g = first.doc_graph,
            grp_iri = result.group_iri,
            prop = ont::iri(ont::PROP_HAS_TRANSLATION),
        );
        let res_links = first.store.query_to_json(&sparql_links).unwrap();
        let link_rows = res_links.as_array().expect("expected array");
        assert_eq!(link_rows.len(), 2);

        // Verify each document has translationGroup back-link
        for vr in &result.variants {
            let sparql = format!(
                "SELECT ?grp WHERE {{ \
                   GRAPH <{g}> {{ \
                     <{doc}> <{prop}> ?grp \
                   }} \
                 }}",
                g = vr.doc_graph,
                doc = vr.doc_graph,
                prop = ont::iri(ont::PROP_TRANSLATION_GROUP),
            );
            let res = vr.store.query_to_json(&sparql).unwrap();
            let rows = res.as_array().expect("expected array");
            assert_eq!(rows.len(), 1);
        }
    }
}
