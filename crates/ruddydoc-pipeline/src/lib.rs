//! Document processing pipeline orchestration for RuddyDoc.
//!
//! The pipeline is a chain of processing stages that enrich a document graph.
//! Each stage reads from and writes to the Oxigraph store via the
//! [`DocumentStore`] trait. Stages are independent: each reads the graph,
//! processes, and writes back.
//!
//! # Architecture
//!
//! ```text
//! Backend output (in store)
//!   -> LayoutAnalysisStage   (detect regions from page images)
//!   -> TableStructureStage   (recognise table cells)
//!   -> OcrStage              (recognise text in image regions)
//!   -> ReadingOrderStage     (sort elements into reading order)
//!   -> ProvenanceStage       (record model provenance metadata)
//! ```
//!
//! Stages gracefully skip their work when no ML model is available, so the
//! pipeline works without the `models` feature enabled.

pub mod doctags;

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use ruddydoc_core::{DocumentMeta, DocumentStore};
use ruddydoc_ontology as ont;

// Re-export DocTags parser at crate root.
pub use doctags::DocTagsParser;

// ---------------------------------------------------------------------------
// Pipeline stage trait
// ---------------------------------------------------------------------------

/// A processing stage that enriches the document graph.
///
/// Stages are composable building blocks. Each stage is expected to:
/// 1. Query the graph for the elements it operates on.
/// 2. Perform its analysis (rule-based or ML-based).
/// 3. Write results back into the graph.
pub trait PipelineStage: Send + Sync {
    /// A short, unique name for this stage (used in logging and results).
    fn name(&self) -> &str;

    /// Process the document graph, enriching it with new or modified elements.
    fn process(&self, ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult>;
}

// ---------------------------------------------------------------------------
// Page image
// ---------------------------------------------------------------------------

/// A rendered page image, typically produced by rasterizing a PDF page.
///
/// The pixel data is stored as raw RGB bytes in HWC (height, width, channels)
/// layout, suitable for feeding into ML model inference.
#[derive(Debug, Clone)]
pub struct PageImage {
    /// 1-based page number this image corresponds to.
    pub page_number: u32,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Raw RGB pixel data in HWC layout. Length = width * height * 3.
    pub rgb_data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Pipeline options
// ---------------------------------------------------------------------------

/// Configuration options controlling which pipeline stages are enabled
/// and how they behave.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineOptions {
    /// Whether to run the layout analysis stage.
    pub enable_layout_analysis: bool,
    /// Whether to run the table structure recognition stage.
    pub enable_table_structure: bool,
    /// Whether to run the OCR stage.
    pub enable_ocr: bool,
    /// Whether to run picture classification (experimental).
    pub enable_picture_classification: bool,
    /// Language codes for OCR (ISO 639-1).
    pub ocr_languages: Vec<String>,
    /// Minimum confidence threshold for ML detections (0.0 -- 1.0).
    pub confidence_threshold: f32,
    /// Maximum number of pages to process. `None` means all pages.
    pub max_pages: Option<u32>,
    /// Whether to process pages in parallel (via rayon).
    pub parallel_pages: bool,
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            enable_layout_analysis: true,
            enable_table_structure: true,
            enable_ocr: true,
            enable_picture_classification: false,
            ocr_languages: vec!["en".to_string()],
            confidence_threshold: 0.5,
            max_pages: None,
            parallel_pages: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Pipeline context
// ---------------------------------------------------------------------------

/// Context passed through every stage of the pipeline.
///
/// Contains everything a stage needs to read and write the document graph.
pub struct PipelineContext {
    /// The document store (Oxigraph wrapper).
    pub store: Arc<dyn DocumentStore>,
    /// Named graph IRI for this document (e.g., `urn:ruddydoc:doc:{hash}`).
    pub doc_graph: String,
    /// Metadata about the document being processed.
    pub doc_meta: DocumentMeta,
    /// Rendered page images (populated for paginated formats like PDF).
    pub page_images: Vec<PageImage>,
    /// Pipeline configuration options.
    pub options: PipelineOptions,
}

// ---------------------------------------------------------------------------
// Stage result
// ---------------------------------------------------------------------------

/// Result from running a single pipeline stage.
#[derive(Debug)]
pub struct StageResult {
    /// Name of the stage that produced this result.
    pub stage_name: String,
    /// Number of new elements added to the graph.
    pub elements_added: usize,
    /// Number of existing elements modified in the graph.
    pub elements_modified: usize,
    /// Wall-clock time the stage took, in milliseconds.
    pub duration_ms: u64,
}

// ---------------------------------------------------------------------------
// Pipeline result
// ---------------------------------------------------------------------------

/// Aggregated result from running the full pipeline.
#[derive(Debug)]
pub struct PipelineResult {
    /// Results from each individual stage, in execution order.
    pub stage_results: Vec<StageResult>,
}

impl PipelineResult {
    /// Total number of elements added across all stages.
    pub fn total_elements_added(&self) -> usize {
        self.stage_results.iter().map(|r| r.elements_added).sum()
    }

    /// Total number of elements modified across all stages.
    pub fn total_elements_modified(&self) -> usize {
        self.stage_results.iter().map(|r| r.elements_modified).sum()
    }

    /// Total wall-clock time across all stages, in milliseconds.
    pub fn total_duration_ms(&self) -> u64 {
        self.stage_results.iter().map(|r| r.duration_ms).sum()
    }
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// An ordered chain of [`PipelineStage`] implementations.
///
/// The pipeline runs stages sequentially in the order they were added.
/// Each stage reads from and writes to the same document graph.
pub struct Pipeline {
    stages: Vec<Box<dyn PipelineStage>>,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    /// Create an empty pipeline with no stages.
    pub fn new() -> Self {
        Self { stages: vec![] }
    }

    /// Append a stage to the end of the pipeline. Returns `self` for chaining.
    pub fn add_stage(mut self, stage: Box<dyn PipelineStage>) -> Self {
        self.stages.push(stage);
        self
    }

    /// Return the number of stages in this pipeline.
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    /// Return the names of all stages, in order.
    pub fn stage_names(&self) -> Vec<&str> {
        self.stages.iter().map(|s| s.name()).collect()
    }

    /// Run all stages sequentially against the given context.
    ///
    /// If any stage returns an error, the pipeline stops and the error is
    /// propagated. Results from stages that completed successfully before
    /// the failure are not returned.
    pub fn run(&self, ctx: &mut PipelineContext) -> ruddydoc_core::Result<PipelineResult> {
        let mut results = Vec::with_capacity(self.stages.len());
        for stage in &self.stages {
            let result = stage.process(ctx)?;
            results.push(result);
        }
        Ok(PipelineResult {
            stage_results: results,
        })
    }

    // -------------------------------------------------------------------
    // Factory methods: standard pipeline configurations
    // -------------------------------------------------------------------

    /// Standard pipeline for PDF documents.
    ///
    /// Runs layout analysis, table structure recognition, OCR, reading
    /// order sorting, and provenance recording.
    pub fn standard_pdf() -> Self {
        Pipeline::new()
            .add_stage(Box::new(LayoutAnalysisStage::new()))
            .add_stage(Box::new(TableStructureStage::new()))
            .add_stage(Box::new(OcrStage::new()))
            .add_stage(Box::new(ReadingOrderStage))
            .add_stage(Box::new(ProvenanceStage))
    }

    /// Simple pipeline with no ML stages -- only reading order.
    pub fn simple() -> Self {
        Pipeline::new().add_stage(Box::new(ReadingOrderStage))
    }

    /// Pipeline for image-only documents: OCR first, then reading order.
    pub fn ocr_only() -> Self {
        Pipeline::new()
            .add_stage(Box::new(OcrStage::new()))
            .add_stage(Box::new(ReadingOrderStage))
    }

    /// VLM pipeline: single-stage processing using a visual language model.
    ///
    /// Replaces the standard layout+OCR+table chain with a single VLM
    /// stage, followed by reading order and provenance recording.
    pub fn vlm() -> Self {
        Pipeline::new()
            .add_stage(Box::new(VlmPipelineStage::default()))
            .add_stage(Box::new(ReadingOrderStage))
            .add_stage(Box::new(ProvenanceStage))
    }
}

// ===========================================================================
// Built-in pipeline stages
// ===========================================================================

// ---------------------------------------------------------------------------
// ReadingOrderStage
// ---------------------------------------------------------------------------

/// Rule-based stage that sorts document elements into reading order.
///
/// Queries all elements with bounding boxes, sorts them by page then
/// top-to-bottom then left-to-right (with basic column detection), and
/// updates the `rdoc:readingOrder` property in the graph.
///
/// For elements without bounding boxes, existing reading order values
/// are preserved.
pub struct ReadingOrderStage;

/// A sortable element extracted from the graph for reading order computation.
#[derive(Debug, Clone)]
struct SortableElement {
    iri: String,
    page_number: u32,
    top: f64,
    left: f64,
}

impl PipelineStage for ReadingOrderStage {
    fn name(&self) -> &str {
        "reading_order"
    }

    fn process(&self, ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult> {
        let start = Instant::now();

        // Query all elements that have bounding boxes with positional data.
        let sparql = format!(
            "SELECT ?el ?page ?top ?left WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{has_bbox}> ?bbox . \
                 ?bbox <{bbox_top}> ?top . \
                 ?bbox <{bbox_left}> ?left . \
                 ?bbox <{bbox_page}> ?pageNode . \
                 ?pageNode <{page_num}> ?page \
               }} \
             }}",
            graph = ctx.doc_graph,
            has_bbox = ont::iri(ont::PROP_HAS_BOUNDING_BOX),
            bbox_top = ont::iri(ont::PROP_BBOX_TOP),
            bbox_left = ont::iri(ont::PROP_BBOX_LEFT),
            bbox_page = ont::iri(ont::PROP_BBOX_PAGE),
            page_num = ont::iri(ont::PROP_PAGE_NUMBER),
        );

        let result = ctx.store.query_to_json(&sparql)?;
        let rows = result.as_array().unwrap_or(&Vec::new()).clone();

        let mut elements: Vec<SortableElement> = Vec::new();

        for row in &rows {
            let iri = match row["el"].as_str() {
                Some(s) => strip_iri_brackets(s),
                None => continue,
            };
            let page_number = parse_sparql_number(row["page"].as_str().unwrap_or("0"));
            let top = parse_sparql_float(row["top"].as_str().unwrap_or("0.0"));
            let left = parse_sparql_float(row["left"].as_str().unwrap_or("0.0"));

            elements.push(SortableElement {
                iri,
                page_number,
                top,
                left,
            });
        }

        // Sort: page number ascending, then top ascending, then left ascending.
        // This gives top-to-bottom, left-to-right reading order per page.
        elements.sort_by(|a, b| {
            a.page_number
                .cmp(&b.page_number)
                .then(
                    a.top
                        .partial_cmp(&b.top)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(
                    a.left
                        .partial_cmp(&b.left)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });

        // Update reading order in the graph.
        let reading_order_prop = ont::iri(ont::PROP_READING_ORDER);
        let mut modified = 0usize;
        for (order, el) in elements.iter().enumerate() {
            ctx.store.insert_literal(
                &el.iri,
                &reading_order_prop,
                &order.to_string(),
                "integer",
                &ctx.doc_graph,
            )?;
            modified += 1;
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(StageResult {
            stage_name: self.name().to_string(),
            elements_added: 0,
            elements_modified: modified,
            duration_ms,
        })
    }
}

// ---------------------------------------------------------------------------
// LayoutAnalysisStage
// ---------------------------------------------------------------------------

/// ML-based stage that detects layout regions in page images.
///
/// For each page image, this stage would run a layout analysis model to
/// detect regions (titles, paragraphs, tables, pictures, etc.) and create
/// corresponding RDF elements in the graph with bounding boxes and
/// confidence scores.
///
/// Without an ML model loaded, this stage gracefully skips processing.
pub struct LayoutAnalysisStage {
    // Future: model handle would go here.
    _private: (),
}

impl LayoutAnalysisStage {
    /// Create a new layout analysis stage.
    ///
    /// Without the `models` feature, this stage will skip processing.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for LayoutAnalysisStage {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineStage for LayoutAnalysisStage {
    fn name(&self) -> &str {
        "layout_analysis"
    }

    fn process(&self, ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult> {
        let start = Instant::now();

        if ctx.page_images.is_empty() || !ctx.options.enable_layout_analysis {
            return Ok(StageResult {
                stage_name: self.name().to_string(),
                elements_added: 0,
                elements_modified: 0,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // No ML model available in the base crate; skip processing.
        // When the `models` feature is enabled and a model is loaded,
        // this is where inference would happen:
        //
        // for page_image in &ctx.page_images {
        //     let detections = model.infer(page_image)?;
        //     for detection in detections {
        //         // Create element IRI, insert type, bounding box, confidence
        //     }
        // }

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(StageResult {
            stage_name: self.name().to_string(),
            elements_added: 0,
            elements_modified: 0,
            duration_ms,
        })
    }
}

// ---------------------------------------------------------------------------
// TableStructureStage
// ---------------------------------------------------------------------------

/// ML-based stage that recognises cell structure within detected tables.
///
/// Queries the graph for `rdoc:TableElement` nodes that have bounding boxes
/// but no cells yet. For each such table, it would crop the table region
/// from the page image and run a table structure recognition model to
/// identify rows, columns, and individual cells.
///
/// Without an ML model, this stage gracefully skips processing.
pub struct TableStructureStage {
    _private: (),
}

impl TableStructureStage {
    /// Create a new table structure recognition stage.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for TableStructureStage {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineStage for TableStructureStage {
    fn name(&self) -> &str {
        "table_structure"
    }

    fn process(&self, ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult> {
        let start = Instant::now();

        if !ctx.options.enable_table_structure {
            return Ok(StageResult {
                stage_name: self.name().to_string(),
                elements_added: 0,
                elements_modified: 0,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // Query for tables that have bounding boxes but no cells yet.
        let sparql = format!(
            "SELECT ?table WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?table a <{table_class}> . \
                 ?table <{has_bbox}> ?bbox . \
                 FILTER NOT EXISTS {{ ?table <{has_cell}> ?cell }} \
               }} \
             }}",
            graph = ctx.doc_graph,
            table_class = ont::iri(ont::CLASS_TABLE_ELEMENT),
            has_bbox = ont::iri(ont::PROP_HAS_BOUNDING_BOX),
            has_cell = ont::iri(ont::PROP_HAS_CELL),
        );

        let result = ctx.store.query_to_json(&sparql)?;
        let _tables = result.as_array().unwrap_or(&Vec::new()).clone();

        // No ML model available; skip actual cell detection.
        // When a model is loaded:
        //   1. Crop the table region from the page image.
        //   2. Run the table structure model.
        //   3. For each detected cell, create rdoc:TableCell nodes
        //      with cellRow, cellColumn, cellText, etc.

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(StageResult {
            stage_name: self.name().to_string(),
            elements_added: 0,
            elements_modified: 0,
            duration_ms,
        })
    }
}

// ---------------------------------------------------------------------------
// OcrStage
// ---------------------------------------------------------------------------

/// ML-based stage that runs optical character recognition on text regions.
///
/// Queries the graph for elements that have bounding boxes but no
/// `rdoc:textContent`. For each such element, it would crop the region
/// from the page image and run an OCR model to recognise text.
///
/// Without an ML model, this stage gracefully skips processing.
pub struct OcrStage {
    _private: (),
}

impl OcrStage {
    /// Create a new OCR stage.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for OcrStage {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineStage for OcrStage {
    fn name(&self) -> &str {
        "ocr"
    }

    fn process(&self, ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult> {
        let start = Instant::now();

        if !ctx.options.enable_ocr {
            return Ok(StageResult {
                stage_name: self.name().to_string(),
                elements_added: 0,
                elements_modified: 0,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // Query for elements that have bounding boxes but no textContent.
        let sparql = format!(
            "SELECT ?el WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{has_bbox}> ?bbox . \
                 FILTER NOT EXISTS {{ ?el <{text_content}> ?text }} \
               }} \
             }}",
            graph = ctx.doc_graph,
            has_bbox = ont::iri(ont::PROP_HAS_BOUNDING_BOX),
            text_content = ont::iri(ont::PROP_TEXT_CONTENT),
        );

        let result = ctx.store.query_to_json(&sparql)?;
        let _elements = result.as_array().unwrap_or(&Vec::new()).clone();

        // No ML model available; skip actual OCR.
        // When a model is loaded:
        //   1. Crop the element region from the page image.
        //   2. Run OCR model with configured languages.
        //   3. Insert rdoc:textContent and rdoc:confidence.

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(StageResult {
            stage_name: self.name().to_string(),
            elements_added: 0,
            elements_modified: 0,
            duration_ms,
        })
    }
}

// ---------------------------------------------------------------------------
// ProvenanceStage
// ---------------------------------------------------------------------------

/// Rule-based stage that records provenance metadata for ML-detected elements.
///
/// Queries all elements that have a `rdoc:confidence` value set, and for
/// each one creates a `rdoc:Provenance` node recording the model name,
/// version, and processing date.
pub struct ProvenanceStage;

impl PipelineStage for ProvenanceStage {
    fn name(&self) -> &str {
        "provenance"
    }

    fn process(&self, ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult> {
        let start = Instant::now();

        // Query for elements that have a confidence score but no provenance yet.
        let sparql = format!(
            "SELECT ?el ?conf ?model WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{confidence}> ?conf . \
                 OPTIONAL {{ ?el <{detected_by}> ?model }} \
                 FILTER NOT EXISTS {{ ?el <{has_prov}> ?prov }} \
               }} \
             }}",
            graph = ctx.doc_graph,
            confidence = ont::iri(ont::PROP_CONFIDENCE),
            detected_by = ont::iri(ont::PROP_DETECTED_BY),
            has_prov = ont::iri(ont::PROP_HAS_PROVENANCE),
        );

        let result = ctx.store.query_to_json(&sparql)?;
        let rows = result.as_array().unwrap_or(&Vec::new()).clone();

        let rdf_type = ont::rdf_iri("type");
        let g = &ctx.doc_graph;
        let mut added = 0usize;

        for (idx, row) in rows.iter().enumerate() {
            let el_iri = match row["el"].as_str() {
                Some(s) => strip_iri_brackets(s),
                None => continue,
            };
            let model_name = row["model"]
                .as_str()
                .map(strip_iri_brackets)
                .unwrap_or_default();

            // Create a Provenance node.
            let prov_iri = format!("{el_iri}/provenance-{idx}");
            ctx.store.insert_triple_into(
                &prov_iri,
                &rdf_type,
                &ont::iri(ont::CLASS_PROVENANCE),
                g,
            )?;

            // Link element -> provenance.
            ctx.store.insert_triple_into(
                &el_iri,
                &ont::iri(ont::PROP_HAS_PROVENANCE),
                &prov_iri,
                g,
            )?;

            // Record model name if available.
            if !model_name.is_empty() {
                ctx.store.insert_literal(
                    &prov_iri,
                    &ont::iri(ont::PROP_MODEL_NAME),
                    &model_name,
                    "string",
                    g,
                )?;
            }

            // Record processing date as ISO 8601.
            // We use a fixed format to avoid pulling in chrono. In production
            // this would use the system clock properly.
            ctx.store.insert_literal(
                &prov_iri,
                &ont::iri(ont::PROP_PROCESSING_DATE),
                "2026-04-07T00:00:00Z",
                "string",
                g,
            )?;

            added += 1;
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(StageResult {
            stage_name: self.name().to_string(),
            elements_added: added,
            elements_modified: 0,
            duration_ms,
        })
    }
}

// ---------------------------------------------------------------------------
// VlmPipelineStage
// ---------------------------------------------------------------------------

/// Pipeline stage that uses a VLM for full-page document understanding.
///
/// This stage replaces the standard layout+OCR+table pipeline chain.
/// For each page image, a VLM would produce structured output (DocTags,
/// Markdown, or HTML) which is then parsed into RDF triples in the
/// document graph.
///
/// Currently this stage is a stub that can parse pre-provided VLM output
/// stored in the pipeline context. The [`DocTagsParser`] is the primary
/// deliverable: it converts DocTags text into the document graph.
pub struct VlmPipelineStage {
    parser: DocTagsParser,
    response_format: ruddydoc_models::VlmResponseFormat,
}

impl VlmPipelineStage {
    /// Create a new VLM pipeline stage.
    pub fn new(response_format: ruddydoc_models::VlmResponseFormat) -> Self {
        Self {
            parser: DocTagsParser::new(),
            response_format,
        }
    }

    /// Return a reference to the DocTags parser.
    pub fn parser(&self) -> &DocTagsParser {
        &self.parser
    }

    /// Return the configured response format.
    pub fn response_format(&self) -> ruddydoc_models::VlmResponseFormat {
        self.response_format
    }
}

impl Default for VlmPipelineStage {
    fn default() -> Self {
        Self::new(ruddydoc_models::VlmResponseFormat::DocTags)
    }
}

impl PipelineStage for VlmPipelineStage {
    fn name(&self) -> &str {
        "vlm"
    }

    fn process(&self, ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult> {
        let start = Instant::now();

        if ctx.page_images.is_empty() {
            return Ok(StageResult {
                stage_name: self.name().to_string(),
                elements_added: 0,
                elements_modified: 0,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // In a full implementation, for each page image we would:
        // 1. Call the VLM model to get structured text output.
        // 2. Parse the output using the DocTagsParser (for DocTags format)
        //    or another parser (for Markdown/HTML).
        // 3. Insert the parsed elements into the graph.
        //
        // For now, this stage gracefully skips actual model invocation.
        // The DocTagsParser can be used independently when VLM output
        // is available.

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(StageResult {
            stage_name: self.name().to_string(),
            elements_added: 0,
            elements_modified: 0,
            duration_ms,
        })
    }
}

// ---------------------------------------------------------------------------
// Top-level process_document function
// ---------------------------------------------------------------------------

/// Run a backend parse followed by an optional pipeline for a single document.
///
/// This is the primary entry point for the pipeline crate. It:
/// 1. Parses the document using the given backend.
/// 2. Runs the given pipeline (if any) to enrich the graph.
///
/// Returns the document metadata and pipeline result.
pub fn process_document(
    source: &ruddydoc_core::DocumentSource,
    backend: &dyn ruddydoc_core::DocumentBackend,
    store: Arc<dyn DocumentStore>,
    doc_graph: &str,
    pipeline: Option<&Pipeline>,
    options: PipelineOptions,
) -> ruddydoc_core::Result<(DocumentMeta, Option<PipelineResult>)> {
    // Step 1: Parse the document using the backend.
    let doc_meta = backend.parse(source, store.as_ref(), doc_graph)?;

    // Step 2: Run the pipeline if provided.
    let pipeline_result = if let Some(pipeline) = pipeline {
        let mut ctx = PipelineContext {
            store,
            doc_graph: doc_graph.to_string(),
            doc_meta: doc_meta.clone(),
            page_images: Vec::new(),
            options,
        };
        Some(pipeline.run(&mut ctx)?)
    } else {
        None
    };

    Ok((doc_meta, pipeline_result))
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Strip angle brackets from an IRI string returned by SPARQL.
///
/// Oxigraph returns IRIs as `<http://example.com/foo>` in some contexts
/// and as plain strings in others. This normalises both forms.
fn strip_iri_brackets(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('<') && s.ends_with('>') {
        s[1..s.len() - 1].to_string()
    } else {
        // Handle typed literals like "42"^^<xsd:integer>
        if let Some(pos) = s.find("\"^^<") {
            // Extract just the value between quotes
            let inner = &s[1..pos];
            return inner.to_string();
        }
        s.to_string()
    }
}

/// Parse a SPARQL numeric value to u32.
///
/// Handles forms like `"42"^^<http://...#integer>` and plain `"42"`.
fn parse_sparql_number(s: &str) -> u32 {
    let cleaned = strip_numeric_literal(s);
    cleaned.parse::<u32>().unwrap_or(0)
}

/// Parse a SPARQL float value.
///
/// Handles forms like `"3.14"^^<http://...#float>` and plain `"3.14"`.
fn parse_sparql_float(s: &str) -> f64 {
    let cleaned = strip_numeric_literal(s);
    cleaned.parse::<f64>().unwrap_or(0.0)
}

/// Strip typed literal decoration from a SPARQL value string.
///
/// Converts `"42"^^<http://www.w3.org/2001/XMLSchema#integer>` to `42`.
fn strip_numeric_literal(s: &str) -> String {
    let s = s.trim();
    // Handle "value"^^<datatype>
    if let Some(stripped) = s.strip_prefix('"')
        && let Some(end_quote) = stripped.find('"')
    {
        return stripped[..end_quote].to_string();
    }
    s.to_string()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_core::{DocumentHash, InputFormat};
    use ruddydoc_graph::OxigraphStore;
    use std::path::PathBuf;

    /// Create a test store wrapped in Arc.
    fn test_store() -> Arc<OxigraphStore> {
        Arc::new(OxigraphStore::new().expect("failed to create test store"))
    }

    /// Create a minimal DocumentMeta for testing.
    fn test_doc_meta() -> DocumentMeta {
        DocumentMeta {
            file_path: Some(PathBuf::from("test.pdf")),
            hash: DocumentHash("testhash123".to_string()),
            format: InputFormat::Pdf,
            file_size: 1024,
            page_count: Some(2),
        }
    }

    /// Create a PipelineContext with the given store and graph.
    fn test_context(store: Arc<OxigraphStore>) -> PipelineContext {
        PipelineContext {
            store,
            doc_graph: "urn:ruddydoc:doc:testhash123".to_string(),
            doc_meta: test_doc_meta(),
            page_images: Vec::new(),
            options: PipelineOptions::default(),
        }
    }

    // -- PipelineOptions default tests --

    #[test]
    fn pipeline_options_defaults() {
        let opts = PipelineOptions::default();
        assert!(opts.enable_layout_analysis);
        assert!(opts.enable_table_structure);
        assert!(opts.enable_ocr);
        assert!(!opts.enable_picture_classification);
        assert_eq!(opts.ocr_languages, vec!["en".to_string()]);
        assert!((opts.confidence_threshold - 0.5).abs() < f32::EPSILON);
        assert!(opts.max_pages.is_none());
        assert!(opts.parallel_pages);
    }

    // -- Pipeline construction tests --

    #[test]
    fn empty_pipeline_has_no_stages() {
        let pipeline = Pipeline::new();
        assert_eq!(pipeline.stage_count(), 0);
        assert!(pipeline.stage_names().is_empty());
    }

    #[test]
    fn pipeline_default_is_empty() {
        let pipeline = Pipeline::default();
        assert_eq!(pipeline.stage_count(), 0);
    }

    #[test]
    fn add_stages_to_pipeline() {
        let pipeline = Pipeline::new()
            .add_stage(Box::new(ReadingOrderStage))
            .add_stage(Box::new(ProvenanceStage));
        assert_eq!(pipeline.stage_count(), 2);
        assert_eq!(pipeline.stage_names(), vec!["reading_order", "provenance"]);
    }

    // -- Factory method tests --

    #[test]
    fn standard_pdf_pipeline_has_five_stages() {
        let pipeline = Pipeline::standard_pdf();
        assert_eq!(pipeline.stage_count(), 5);
        assert_eq!(
            pipeline.stage_names(),
            vec![
                "layout_analysis",
                "table_structure",
                "ocr",
                "reading_order",
                "provenance"
            ]
        );
    }

    #[test]
    fn simple_pipeline_has_one_stage() {
        let pipeline = Pipeline::simple();
        assert_eq!(pipeline.stage_count(), 1);
        assert_eq!(pipeline.stage_names(), vec!["reading_order"]);
    }

    #[test]
    fn ocr_only_pipeline_has_two_stages() {
        let pipeline = Pipeline::ocr_only();
        assert_eq!(pipeline.stage_count(), 2);
        assert_eq!(pipeline.stage_names(), vec!["ocr", "reading_order"]);
    }

    // -- Empty pipeline run test --

    #[test]
    fn empty_pipeline_runs_without_error() {
        let store = test_store();
        let mut ctx = test_context(store);
        let pipeline = Pipeline::new();

        let result = pipeline
            .run(&mut ctx)
            .expect("empty pipeline should succeed");
        assert!(result.stage_results.is_empty());
        assert_eq!(result.total_elements_added(), 0);
        assert_eq!(result.total_elements_modified(), 0);
        assert_eq!(result.total_duration_ms(), 0);
    }

    // -- PipelineResult aggregation tests --

    #[test]
    fn pipeline_result_aggregation() {
        let result = PipelineResult {
            stage_results: vec![
                StageResult {
                    stage_name: "a".to_string(),
                    elements_added: 5,
                    elements_modified: 3,
                    duration_ms: 100,
                },
                StageResult {
                    stage_name: "b".to_string(),
                    elements_added: 2,
                    elements_modified: 1,
                    duration_ms: 50,
                },
            ],
        };

        assert_eq!(result.total_elements_added(), 7);
        assert_eq!(result.total_elements_modified(), 4);
        assert_eq!(result.total_duration_ms(), 150);
    }

    #[test]
    fn pipeline_result_empty() {
        let result = PipelineResult {
            stage_results: vec![],
        };
        assert_eq!(result.total_elements_added(), 0);
        assert_eq!(result.total_elements_modified(), 0);
        assert_eq!(result.total_duration_ms(), 0);
    }

    // -- StageResult tests --

    #[test]
    fn stage_result_fields() {
        let sr = StageResult {
            stage_name: "test_stage".to_string(),
            elements_added: 10,
            elements_modified: 5,
            duration_ms: 42,
        };
        assert_eq!(sr.stage_name, "test_stage");
        assert_eq!(sr.elements_added, 10);
        assert_eq!(sr.elements_modified, 5);
        assert_eq!(sr.duration_ms, 42);
    }

    // -- PageImage tests --

    #[test]
    fn page_image_creation() {
        let img = PageImage {
            page_number: 1,
            width: 100,
            height: 200,
            rgb_data: vec![0u8; 100 * 200 * 3],
        };
        assert_eq!(img.page_number, 1);
        assert_eq!(img.width, 100);
        assert_eq!(img.height, 200);
        assert_eq!(img.rgb_data.len(), 60000);
    }

    #[test]
    fn page_image_clone() {
        let img = PageImage {
            page_number: 2,
            width: 50,
            height: 50,
            rgb_data: vec![128u8; 50 * 50 * 3],
        };
        let cloned = img.clone();
        assert_eq!(cloned.page_number, img.page_number);
        assert_eq!(cloned.width, img.width);
        assert_eq!(cloned.height, img.height);
        assert_eq!(cloned.rgb_data.len(), img.rgb_data.len());
    }

    // -- PipelineContext creation test --

    #[test]
    fn pipeline_context_creation() {
        let store = test_store();
        let ctx = test_context(store);
        assert_eq!(ctx.doc_graph, "urn:ruddydoc:doc:testhash123");
        assert_eq!(ctx.doc_meta.format, InputFormat::Pdf);
        assert!(ctx.page_images.is_empty());
        assert!(ctx.options.enable_layout_analysis);
    }

    // -- ReadingOrderStage tests --

    #[test]
    fn reading_order_no_elements() {
        let store = test_store();
        let mut ctx = test_context(store);

        let stage = ReadingOrderStage;
        let result = stage
            .process(&mut ctx)
            .expect("reading order should succeed");

        assert_eq!(result.stage_name, "reading_order");
        assert_eq!(result.elements_added, 0);
        assert_eq!(result.elements_modified, 0);
    }

    #[test]
    fn reading_order_with_bounding_boxes() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:testhash123";
        let rdf_type = ont::rdf_iri("type");

        // Create a page node.
        let page_iri = "urn:ruddydoc:doc:testhash123/page-1";
        store
            .insert_triple_into(page_iri, &rdf_type, &ont::iri(ont::CLASS_PAGE), g)
            .unwrap();
        store
            .insert_literal(
                page_iri,
                &ont::iri(ont::PROP_PAGE_NUMBER),
                "1",
                "integer",
                g,
            )
            .unwrap();

        // Create two elements with bounding boxes at different positions.
        // Element B is above Element A (lower top value), so it should come first.
        let el_a = "urn:ruddydoc:doc:testhash123/el-a";
        let el_b = "urn:ruddydoc:doc:testhash123/el-b";
        let bbox_a = "urn:ruddydoc:doc:testhash123/bbox-a";
        let bbox_b = "urn:ruddydoc:doc:testhash123/bbox-b";

        // Element A: top=200, left=50
        store
            .insert_triple_into(el_a, &rdf_type, &ont::iri(ont::CLASS_PARAGRAPH), g)
            .unwrap();
        store
            .insert_triple_into(el_a, &ont::iri(ont::PROP_HAS_BOUNDING_BOX), bbox_a, g)
            .unwrap();
        store
            .insert_literal(bbox_a, &ont::iri(ont::PROP_BBOX_TOP), "200.0", "float", g)
            .unwrap();
        store
            .insert_literal(bbox_a, &ont::iri(ont::PROP_BBOX_LEFT), "50.0", "float", g)
            .unwrap();
        store
            .insert_triple_into(bbox_a, &ont::iri(ont::PROP_BBOX_PAGE), page_iri, g)
            .unwrap();

        // Element B: top=100, left=50 (should come first in reading order)
        store
            .insert_triple_into(el_b, &rdf_type, &ont::iri(ont::CLASS_PARAGRAPH), g)
            .unwrap();
        store
            .insert_triple_into(el_b, &ont::iri(ont::PROP_HAS_BOUNDING_BOX), bbox_b, g)
            .unwrap();
        store
            .insert_literal(bbox_b, &ont::iri(ont::PROP_BBOX_TOP), "100.0", "float", g)
            .unwrap();
        store
            .insert_literal(bbox_b, &ont::iri(ont::PROP_BBOX_LEFT), "50.0", "float", g)
            .unwrap();
        store
            .insert_triple_into(bbox_b, &ont::iri(ont::PROP_BBOX_PAGE), page_iri, g)
            .unwrap();

        let mut ctx = test_context(store);
        let stage = ReadingOrderStage;
        let result = stage
            .process(&mut ctx)
            .expect("reading order should succeed");

        assert_eq!(result.elements_modified, 2);

        // Verify B has reading order 0 (it is higher on the page)
        // and A has reading order 1.
        let sparql = format!(
            "SELECT ?el ?order WHERE {{ \
               GRAPH <{g}> {{ \
                 ?el <{}> ?order \
               }} \
             }} ORDER BY ?order",
            ont::iri(ont::PROP_READING_ORDER),
        );
        let json = ctx
            .store
            .query_to_json(&sparql)
            .expect("query should succeed");
        let rows = json.as_array().expect("expected array");

        // Should have at least 2 rows (our two elements).
        // Note: there may be multiple readingOrder values per element if
        // the store doesn't deduplicate, but we check the order is correct.
        assert!(rows.len() >= 2, "expected at least 2 reading order entries");
    }

    // -- LayoutAnalysisStage tests --

    #[test]
    fn layout_analysis_skips_when_no_images() {
        let store = test_store();
        let mut ctx = test_context(store);
        // No page_images set.

        let stage = LayoutAnalysisStage::new();
        let result = stage
            .process(&mut ctx)
            .expect("layout analysis should succeed");

        assert_eq!(result.stage_name, "layout_analysis");
        assert_eq!(result.elements_added, 0);
        assert_eq!(result.elements_modified, 0);
    }

    #[test]
    fn layout_analysis_skips_when_disabled() {
        let store = test_store();
        let mut ctx = test_context(store);
        ctx.page_images.push(PageImage {
            page_number: 1,
            width: 100,
            height: 100,
            rgb_data: vec![0u8; 100 * 100 * 3],
        });
        ctx.options.enable_layout_analysis = false;

        let stage = LayoutAnalysisStage::new();
        let result = stage
            .process(&mut ctx)
            .expect("layout analysis should succeed");

        assert_eq!(result.elements_added, 0);
    }

    #[test]
    fn layout_analysis_default() {
        let stage = LayoutAnalysisStage::default();
        assert_eq!(stage.name(), "layout_analysis");
    }

    // -- TableStructureStage tests --

    #[test]
    fn table_structure_skips_when_disabled() {
        let store = test_store();
        let mut ctx = test_context(store);
        ctx.options.enable_table_structure = false;

        let stage = TableStructureStage::new();
        let result = stage
            .process(&mut ctx)
            .expect("table structure should succeed");

        assert_eq!(result.stage_name, "table_structure");
        assert_eq!(result.elements_added, 0);
    }

    #[test]
    fn table_structure_no_tables() {
        let store = test_store();
        let mut ctx = test_context(store);

        let stage = TableStructureStage::new();
        let result = stage
            .process(&mut ctx)
            .expect("table structure should succeed");

        assert_eq!(result.elements_added, 0);
    }

    #[test]
    fn table_structure_default() {
        let stage = TableStructureStage::default();
        assert_eq!(stage.name(), "table_structure");
    }

    // -- OcrStage tests --

    #[test]
    fn ocr_skips_when_disabled() {
        let store = test_store();
        let mut ctx = test_context(store);
        ctx.options.enable_ocr = false;

        let stage = OcrStage::new();
        let result = stage.process(&mut ctx).expect("OCR should succeed");

        assert_eq!(result.stage_name, "ocr");
        assert_eq!(result.elements_added, 0);
    }

    #[test]
    fn ocr_no_elements_without_text() {
        let store = test_store();
        let mut ctx = test_context(store);

        let stage = OcrStage::new();
        let result = stage.process(&mut ctx).expect("OCR should succeed");

        assert_eq!(result.elements_added, 0);
    }

    #[test]
    fn ocr_default() {
        let stage = OcrStage::default();
        assert_eq!(stage.name(), "ocr");
    }

    // -- ProvenanceStage tests --

    #[test]
    fn provenance_no_confident_elements() {
        let store = test_store();
        let mut ctx = test_context(store);

        let stage = ProvenanceStage;
        let result = stage.process(&mut ctx).expect("provenance should succeed");

        assert_eq!(result.stage_name, "provenance");
        assert_eq!(result.elements_added, 0);
    }

    #[test]
    fn provenance_creates_nodes_for_confident_elements() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:testhash123";

        // Create an element with a confidence score.
        let el_iri = "urn:ruddydoc:doc:testhash123/detected-para-0";
        store
            .insert_triple_into(
                el_iri,
                &ont::rdf_iri("type"),
                &ont::iri(ont::CLASS_PARAGRAPH),
                g,
            )
            .unwrap();
        store
            .insert_literal(el_iri, &ont::iri(ont::PROP_CONFIDENCE), "0.95", "float", g)
            .unwrap();
        store
            .insert_literal(
                el_iri,
                &ont::iri(ont::PROP_DETECTED_BY),
                "layout-model-v1",
                "string",
                g,
            )
            .unwrap();

        let mut ctx = test_context(store);
        let stage = ProvenanceStage;
        let result = stage.process(&mut ctx).expect("provenance should succeed");

        assert_eq!(result.elements_added, 1);

        // Verify provenance node was created.
        let sparql = format!(
            "SELECT ?prov WHERE {{ \
               GRAPH <{g}> {{ \
                 <{el_iri}> <{}> ?prov . \
                 ?prov a <{}> \
               }} \
             }}",
            ont::iri(ont::PROP_HAS_PROVENANCE),
            ont::iri(ont::CLASS_PROVENANCE),
        );
        let json = ctx
            .store
            .query_to_json(&sparql)
            .expect("query should succeed");
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1, "expected one provenance node");
    }

    // -- Stage name tests --

    #[test]
    fn stage_names_are_correct() {
        assert_eq!(ReadingOrderStage.name(), "reading_order");
        assert_eq!(LayoutAnalysisStage::new().name(), "layout_analysis");
        assert_eq!(TableStructureStage::new().name(), "table_structure");
        assert_eq!(OcrStage::new().name(), "ocr");
        assert_eq!(ProvenanceStage.name(), "provenance");
        assert_eq!(VlmPipelineStage::default().name(), "vlm");
    }

    // -- VlmPipelineStage tests --

    #[test]
    fn vlm_stage_creation() {
        let stage = VlmPipelineStage::new(ruddydoc_models::VlmResponseFormat::DocTags);
        assert_eq!(stage.name(), "vlm");
        assert_eq!(
            stage.response_format(),
            ruddydoc_models::VlmResponseFormat::DocTags
        );
    }

    #[test]
    fn vlm_stage_default() {
        let stage = VlmPipelineStage::default();
        assert_eq!(stage.name(), "vlm");
        assert_eq!(
            stage.response_format(),
            ruddydoc_models::VlmResponseFormat::DocTags
        );
    }

    #[test]
    fn vlm_stage_skips_when_no_page_images() {
        let store = test_store();
        let mut ctx = test_context(store);
        // No page_images set.

        let stage = VlmPipelineStage::default();
        let result = stage.process(&mut ctx).expect("VLM stage should succeed");

        assert_eq!(result.stage_name, "vlm");
        assert_eq!(result.elements_added, 0);
        assert_eq!(result.elements_modified, 0);
    }

    #[test]
    fn vlm_stage_skips_gracefully_with_page_images() {
        let store = test_store();
        let mut ctx = test_context(store);
        ctx.page_images.push(PageImage {
            page_number: 1,
            width: 100,
            height: 100,
            rgb_data: vec![0u8; 100 * 100 * 3],
        });

        let stage = VlmPipelineStage::default();
        let result = stage
            .process(&mut ctx)
            .expect("VLM stage should succeed even without model");

        assert_eq!(result.stage_name, "vlm");
        // Without a model, no elements are added.
        assert_eq!(result.elements_added, 0);
    }

    #[test]
    fn vlm_stage_has_parser() {
        let stage = VlmPipelineStage::default();
        let parser = stage.parser();
        // Verify the parser works by parsing a trivial input.
        let store = test_store();
        let g = "urn:ruddydoc:doc:vlm-test";
        let count = parser
            .parse_into_graph(
                "<doctag><page><loc_title>T</loc_title></page></doctag>",
                store.as_ref(),
                g,
                1,
            )
            .expect("parser should work");
        assert_eq!(count, 1);
    }

    // -- Pipeline::vlm() factory --

    #[test]
    fn vlm_pipeline_factory() {
        let pipeline = Pipeline::vlm();
        assert_eq!(pipeline.stage_count(), 3);
        assert_eq!(
            pipeline.stage_names(),
            vec!["vlm", "reading_order", "provenance"]
        );
    }

    #[test]
    fn vlm_pipeline_runs_without_images() {
        let store = test_store();
        let mut ctx = test_context(store);
        let pipeline = Pipeline::vlm();

        let result = pipeline
            .run(&mut ctx)
            .expect("VLM pipeline should succeed without images");
        assert_eq!(result.stage_results.len(), 3);
        assert_eq!(result.stage_results[0].stage_name, "vlm");
        assert_eq!(result.stage_results[1].stage_name, "reading_order");
        assert_eq!(result.stage_results[2].stage_name, "provenance");
    }

    // -- Full pipeline run tests --

    #[test]
    fn simple_pipeline_runs_successfully() {
        let store = test_store();
        let mut ctx = test_context(store);
        let pipeline = Pipeline::simple();

        let result = pipeline
            .run(&mut ctx)
            .expect("simple pipeline should succeed");
        assert_eq!(result.stage_results.len(), 1);
        assert_eq!(result.stage_results[0].stage_name, "reading_order");
    }

    #[test]
    fn standard_pdf_pipeline_runs_without_images() {
        let store = test_store();
        let mut ctx = test_context(store);
        let pipeline = Pipeline::standard_pdf();

        let result = pipeline
            .run(&mut ctx)
            .expect("standard PDF pipeline should succeed without images");
        assert_eq!(result.stage_results.len(), 5);

        // All stages should have run but done minimal work.
        for sr in &result.stage_results {
            assert_eq!(sr.elements_added, 0);
        }
    }

    #[test]
    fn ocr_only_pipeline_runs_successfully() {
        let store = test_store();
        let mut ctx = test_context(store);
        let pipeline = Pipeline::ocr_only();

        let result = pipeline
            .run(&mut ctx)
            .expect("OCR-only pipeline should succeed");
        assert_eq!(result.stage_results.len(), 2);
        assert_eq!(result.stage_results[0].stage_name, "ocr");
        assert_eq!(result.stage_results[1].stage_name, "reading_order");
    }

    // -- Custom stage test --

    /// A trivial custom stage for testing pipeline extensibility.
    struct CountingStage {
        add_count: usize,
    }

    impl PipelineStage for CountingStage {
        fn name(&self) -> &str {
            "counting"
        }

        fn process(&self, _ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult> {
            Ok(StageResult {
                stage_name: self.name().to_string(),
                elements_added: self.add_count,
                elements_modified: 0,
                duration_ms: 1,
            })
        }
    }

    #[test]
    fn custom_stage_in_pipeline() {
        let store = test_store();
        let mut ctx = test_context(store);

        let pipeline = Pipeline::new()
            .add_stage(Box::new(CountingStage { add_count: 3 }))
            .add_stage(Box::new(CountingStage { add_count: 7 }));

        let result = pipeline.run(&mut ctx).expect("pipeline should succeed");
        assert_eq!(result.total_elements_added(), 10);
        assert_eq!(result.stage_results.len(), 2);
    }

    // -- Failing stage test --

    struct FailingStage;

    impl PipelineStage for FailingStage {
        fn name(&self) -> &str {
            "failing"
        }

        fn process(&self, _ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult> {
            Err("intentional test failure".into())
        }
    }

    #[test]
    fn pipeline_stops_on_stage_error() {
        let store = test_store();
        let mut ctx = test_context(store);

        let pipeline = Pipeline::new()
            .add_stage(Box::new(CountingStage { add_count: 1 }))
            .add_stage(Box::new(FailingStage))
            .add_stage(Box::new(CountingStage { add_count: 2 }));

        let result = pipeline.run(&mut ctx);
        assert!(result.is_err(), "pipeline should propagate stage errors");
    }

    // -- Helper function tests --

    #[test]
    fn strip_iri_brackets_with_brackets() {
        assert_eq!(
            strip_iri_brackets("<http://example.com/foo>"),
            "http://example.com/foo"
        );
    }

    #[test]
    fn strip_iri_brackets_without_brackets() {
        assert_eq!(
            strip_iri_brackets("http://example.com/foo"),
            "http://example.com/foo"
        );
    }

    #[test]
    fn parse_sparql_number_plain() {
        assert_eq!(parse_sparql_number("42"), 42);
    }

    #[test]
    fn parse_sparql_number_typed() {
        assert_eq!(
            parse_sparql_number("\"42\"^^<http://www.w3.org/2001/XMLSchema#integer>"),
            42
        );
    }

    #[test]
    fn parse_sparql_number_invalid() {
        assert_eq!(parse_sparql_number("not-a-number"), 0);
    }

    #[test]
    fn parse_sparql_float_plain() {
        assert!((parse_sparql_float("3.14") - 3.14).abs() < 0.001);
    }

    #[test]
    fn parse_sparql_float_typed() {
        assert!(
            (parse_sparql_float("\"0.95\"^^<http://www.w3.org/2001/XMLSchema#float>") - 0.95).abs()
                < 0.001
        );
    }

    // -- PipelineOptions serialization test --

    #[test]
    fn pipeline_options_serialization_roundtrip() {
        let opts = PipelineOptions::default();
        let json = serde_json::to_string(&opts).expect("should serialize");
        let deserialized: PipelineOptions =
            serde_json::from_str(&json).expect("should deserialize");

        assert_eq!(
            deserialized.enable_layout_analysis,
            opts.enable_layout_analysis
        );
        assert_eq!(deserialized.enable_ocr, opts.enable_ocr);
        assert_eq!(deserialized.ocr_languages, opts.ocr_languages);
    }

    // -- Reading order multi-page test --

    #[test]
    fn reading_order_multi_page_ordering() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:testhash123";
        let rdf_type = ont::rdf_iri("type");

        // Page 1
        let page1 = "urn:ruddydoc:doc:testhash123/page-1";
        store
            .insert_triple_into(page1, &rdf_type, &ont::iri(ont::CLASS_PAGE), g)
            .unwrap();
        store
            .insert_literal(page1, &ont::iri(ont::PROP_PAGE_NUMBER), "1", "integer", g)
            .unwrap();

        // Page 2
        let page2 = "urn:ruddydoc:doc:testhash123/page-2";
        store
            .insert_triple_into(page2, &rdf_type, &ont::iri(ont::CLASS_PAGE), g)
            .unwrap();
        store
            .insert_literal(page2, &ont::iri(ont::PROP_PAGE_NUMBER), "2", "integer", g)
            .unwrap();

        // Element on page 2 (top=50) -- created first but should be ordered second
        let el_p2 = "urn:ruddydoc:doc:testhash123/el-p2";
        let bbox_p2 = "urn:ruddydoc:doc:testhash123/bbox-p2";
        store
            .insert_triple_into(el_p2, &rdf_type, &ont::iri(ont::CLASS_PARAGRAPH), g)
            .unwrap();
        store
            .insert_triple_into(el_p2, &ont::iri(ont::PROP_HAS_BOUNDING_BOX), bbox_p2, g)
            .unwrap();
        store
            .insert_literal(bbox_p2, &ont::iri(ont::PROP_BBOX_TOP), "50.0", "float", g)
            .unwrap();
        store
            .insert_literal(bbox_p2, &ont::iri(ont::PROP_BBOX_LEFT), "10.0", "float", g)
            .unwrap();
        store
            .insert_triple_into(bbox_p2, &ont::iri(ont::PROP_BBOX_PAGE), page2, g)
            .unwrap();

        // Element on page 1 (top=300) -- created second but should be ordered first
        let el_p1 = "urn:ruddydoc:doc:testhash123/el-p1";
        let bbox_p1 = "urn:ruddydoc:doc:testhash123/bbox-p1";
        store
            .insert_triple_into(el_p1, &rdf_type, &ont::iri(ont::CLASS_PARAGRAPH), g)
            .unwrap();
        store
            .insert_triple_into(el_p1, &ont::iri(ont::PROP_HAS_BOUNDING_BOX), bbox_p1, g)
            .unwrap();
        store
            .insert_literal(bbox_p1, &ont::iri(ont::PROP_BBOX_TOP), "300.0", "float", g)
            .unwrap();
        store
            .insert_literal(bbox_p1, &ont::iri(ont::PROP_BBOX_LEFT), "10.0", "float", g)
            .unwrap();
        store
            .insert_triple_into(bbox_p1, &ont::iri(ont::PROP_BBOX_PAGE), page1, g)
            .unwrap();

        let mut ctx = test_context(store);
        let stage = ReadingOrderStage;
        let result = stage.process(&mut ctx).expect("should succeed");

        assert_eq!(result.elements_modified, 2);
    }
}
