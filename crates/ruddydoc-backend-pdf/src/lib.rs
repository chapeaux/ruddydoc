//! PDF parser backend for RuddyDoc.
//!
//! Uses `lopdf` for text extraction and metadata from PDF documents. When the
//! optional `pdfium` feature is enabled, `pdfium-render` provides word-level
//! text extraction with bounding boxes and high-quality page rendering.
//!
//! # Features
//!
//! - **default**: lopdf-based parsing with improved paragraph splitting, heading
//!   detection heuristics, and Dublin Core metadata extraction.
//! - **pdfium**: Enables PDFium-powered word-level extraction, page rendering,
//!   and font-based heading detection.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use ruddydoc_core::{
    BoundingBox, DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore,
    InputFormat,
};
use ruddydoc_ontology as ont;

/// Dublin Core Terms namespace.
const DCTERMS: &str = "http://purl.org/dc/terms/";

// -----------------------------------------------------------------------
// Enhanced PDF types (always available)
// -----------------------------------------------------------------------

/// A word extracted from a PDF page with its bounding box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfWord {
    /// The text content of this word.
    pub text: String,
    /// Bounding box in page coordinates (points).
    pub bbox: BoundingBox,
    /// Name of the font used to render this word.
    pub font_name: String,
    /// Font size in points.
    pub font_size: f32,
    /// Whether the word appears bold.
    pub is_bold: bool,
    /// Whether the word appears italic.
    pub is_italic: bool,
}

/// A rendered page image from a PDF.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    /// 1-based page number.
    pub page_number: u32,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Raw RGB pixel data in HWC layout. Length = width * height * 3.
    pub rgb_data: Vec<u8>,
    /// Render scale factor (e.g., 2.0 for 144 DPI).
    pub scale: f32,
}

/// An image embedded in a PDF page.
#[derive(Debug, Clone)]
pub struct EmbeddedImage {
    /// 1-based page number where this image appears.
    pub page_number: u32,
    /// Bounding box in page coordinates (points).
    pub bbox: BoundingBox,
    /// Image format (e.g., "png", "jpeg").
    pub format: String,
    /// Raw image data.
    pub data: Vec<u8>,
}

/// A hyperlink in a PDF page.
#[derive(Debug, Clone)]
pub struct PdfLink {
    /// 1-based page number where this link appears.
    pub page_number: u32,
    /// Bounding box in page coordinates (points).
    pub bbox: BoundingBox,
    /// Target URI of the link.
    pub uri: String,
}

// -----------------------------------------------------------------------
// Heading detection result
// -----------------------------------------------------------------------

/// A detected heading with its text and inferred level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedHeading {
    /// The heading text.
    pub text: String,
    /// Inferred heading level (1 = top-level, 2, 3, ...).
    pub level: u32,
}

// -----------------------------------------------------------------------
// PDF document backend
// -----------------------------------------------------------------------

/// PDF document backend.
pub struct PdfBackend;

impl PdfBackend {
    /// Create a new PDF backend instance.
    pub fn new() -> Self {
        Self
    }

    /// Render all pages to images (for ML pipeline).
    ///
    /// Returns an empty `Vec` when the `pdfium` feature is not enabled.
    /// When the `pdfium` feature is enabled, each page is rendered at the
    /// requested DPI.
    pub fn render_pages(
        &self,
        _source: &DocumentSource,
        _dpi: f32,
    ) -> ruddydoc_core::Result<Vec<RenderedPage>> {
        #[cfg(feature = "pdfium")]
        {
            pdfium_backend::render_all_pages(_source, _dpi)
        }

        #[cfg(not(feature = "pdfium"))]
        {
            Ok(Vec::new())
        }
    }
}

impl Default for PdfBackend {
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------
// Hashing
// -----------------------------------------------------------------------

/// Compute a SHA-256 hash of the content bytes, returning a hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode(result.as_slice())
}

/// Hex-encode bytes.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

// -----------------------------------------------------------------------
// Dublin Core helpers
// -----------------------------------------------------------------------

/// Build a Dublin Core Terms IRI.
fn dcterms_iri(term: &str) -> String {
    format!("{DCTERMS}{term}")
}

// -----------------------------------------------------------------------
// PDF string decoding
// -----------------------------------------------------------------------

/// Attempt to decode a PDF byte string to a Rust String.
///
/// PDF strings in the Info dictionary may be PDFDocEncoding (Latin-1 superset)
/// or UTF-16BE (prefixed with BOM 0xFE 0xFF). We handle both cases.
fn decode_pdf_string(bytes: &[u8]) -> String {
    // Check for UTF-16BE BOM
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        // UTF-16BE: skip BOM, decode pairs
        let u16_iter = bytes[2..]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]));
        char::decode_utf16(u16_iter)
            .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
    } else {
        // PDFDocEncoding is a superset of Latin-1 for the printable range.
        // For simplicity, try UTF-8 first, then fall back to Latin-1 mapping.
        String::from_utf8(bytes.to_vec())
            .unwrap_or_else(|_| bytes.iter().map(|&b| b as char).collect())
    }
}

/// Parse a PDF date string (e.g., `D:20231201120000+00'00'`) into an
/// ISO 8601 date string. Returns the raw string if parsing fails.
fn parse_pdf_date(raw: &str) -> String {
    let s = raw.strip_prefix("D:").unwrap_or(raw);
    if s.len() >= 8 {
        let year = &s[0..4];
        let month = if s.len() >= 6 { &s[4..6] } else { "01" };
        let day = if s.len() >= 8 { &s[6..8] } else { "01" };
        format!("{year}-{month}-{day}")
    } else {
        raw.to_string()
    }
}

// -----------------------------------------------------------------------
// Page dimensions
// -----------------------------------------------------------------------

/// Extract page dimensions (width, height) from a page dictionary's MediaBox.
///
/// MediaBox is `[x0, y0, x1, y1]` where width = x1-x0, height = y1-y0.
/// Returns `None` if the MediaBox is missing or malformed.
fn extract_page_dimensions(doc: &lopdf::Document, page_id: lopdf::ObjectId) -> Option<(f32, f32)> {
    let page_dict = doc.get_dictionary(page_id).ok()?;
    // MediaBox can be on the page or inherited from parent.
    // Try the page first, then walk up via get_deref.
    let media_box = page_dict
        .get_deref(b"MediaBox", doc)
        .or_else(|_| page_dict.get(b"MediaBox"))
        .ok()?;
    let arr = media_box.as_array().ok()?;
    if arr.len() >= 4 {
        let x0 = arr[0].as_float().ok()?;
        let y0 = arr[1].as_float().ok()?;
        let x1 = arr[2].as_float().ok()?;
        let y1 = arr[3].as_float().ok()?;
        Some(((x1 - x0).abs(), (y1 - y0).abs()))
    } else {
        None
    }
}

// -----------------------------------------------------------------------
// Improved paragraph splitting and heading detection (lopdf fallback)
// -----------------------------------------------------------------------

/// Split raw page text into paragraph strings using improved heuristics.
///
/// This is an enhanced version that handles:
/// - Double-newline paragraph boundaries
/// - Indented lines (treated as paragraph starts)
/// - Single newlines within a paragraph are joined with a space
fn split_into_paragraphs(text: &str) -> Vec<String> {
    // First, split on explicit double-newlines
    let chunks: Vec<&str> = text.split("\n\n").collect();

    let mut paragraphs = Vec::new();

    for chunk in chunks {
        let trimmed = chunk.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Within each chunk, look for indentation-based paragraph breaks.
        // Lines that start with leading whitespace (after the first line)
        // may indicate a new paragraph within the chunk.
        let lines: Vec<&str> = trimmed.lines().collect();
        if lines.is_empty() {
            continue;
        }

        let mut current_para = String::new();

        for (i, line) in lines.iter().enumerate() {
            let stripped = line.trim();
            if stripped.is_empty() {
                // Empty line within a chunk -> paragraph break
                if !current_para.is_empty() {
                    paragraphs.push(current_para.trim().to_string());
                    current_para = String::new();
                }
                continue;
            }

            // Detect indentation-based paragraph break:
            // If a line starts with significant leading whitespace (>= 4 spaces or tab)
            // AND it is not the first line, treat it as a new paragraph.
            let leading_spaces = line.len() - line.trim_start().len();
            if i > 0 && leading_spaces >= 4 && !current_para.is_empty() {
                paragraphs.push(current_para.trim().to_string());
                current_para = String::new();
            }

            if !current_para.is_empty() {
                current_para.push(' ');
            }
            current_para.push_str(stripped);
        }

        if !current_para.is_empty() {
            paragraphs.push(current_para.trim().to_string());
        }
    }

    paragraphs
}

/// Detect headings from text content using pattern-based heuristics.
///
/// Returns a vector of `DetectedHeading` with inferred heading levels.
/// Heuristics used:
/// - Numbered section patterns ("1.", "1.1", "1.1.1", etc.)
/// - ALL CAPS short lines (< 80 chars)
/// - Short lines (< 60 chars) that end without period and appear standalone
pub fn detect_headings_from_text(paragraphs: &[String]) -> Vec<DetectedHeading> {
    let mut headings = Vec::new();

    // Count non-empty paragraphs for context -- short-line heuristics
    // only apply when there is enough context (multiple paragraphs).
    let non_empty_count = paragraphs.iter().filter(|p| !p.trim().is_empty()).count();

    for para in paragraphs {
        let trimmed = para.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Check for numbered section patterns (always applies)
        if let Some(heading) = detect_numbered_heading(trimmed) {
            headings.push(heading);
            continue;
        }

        // Check for ALL CAPS headings (short lines only, always applies)
        if trimmed.len() < 80 && is_all_caps_heading(trimmed) {
            headings.push(DetectedHeading {
                text: trimmed.to_string(),
                level: 1,
            });
            continue;
        }

        // Short lines without trailing punctuation may be headings.
        // Only apply this heuristic when there are at least 3 paragraphs,
        // so we have enough context to distinguish headings from body text.
        if non_empty_count >= 3
            && trimmed.len() < 60
            && !trimmed.ends_with('.')
            && !trimmed.ends_with(',')
            && !trimmed.ends_with(';')
            && !trimmed.ends_with(':')
            && !trimmed.contains('\n')
            && trimmed.split_whitespace().count() <= 8
        {
            // Only classify as heading if it looks title-like
            // (starts with uppercase, not too many words)
            let first_char = trimmed.chars().next();
            if first_char.is_some_and(|c| c.is_uppercase()) {
                headings.push(DetectedHeading {
                    text: trimmed.to_string(),
                    level: 2,
                });
            }
        }
    }

    headings
}

/// Detect numbered section headings (e.g., "1. Introduction", "1.1 Background").
///
/// Returns the heading level based on the numbering depth (number of number
/// groups):
/// - "1." -> level 1 (1 number group)
/// - "1.1" -> level 2 (2 number groups)
/// - "1.1.1" -> level 3 (3 number groups)
fn detect_numbered_heading(text: &str) -> Option<DetectedHeading> {
    let trimmed = text.trim();

    // Match patterns like "1.", "1.1", "1.1.1", optionally followed by text.
    // We count the number of *number groups* (digit sequences separated by dots).
    let mut chars = trimmed.chars().peekable();

    // Must start with a digit
    if !chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }

    let mut number_group_count = 0u32;
    let mut in_digits = false;
    let mut number_end = 0usize;

    for (i, ch) in trimmed.char_indices() {
        if ch.is_ascii_digit() {
            if !in_digits {
                number_group_count += 1;
                in_digits = true;
            }
            number_end = i + 1;
        } else if ch == '.' && in_digits {
            in_digits = false;
            number_end = i + 1;
        } else if ch == ' ' || ch == '\t' {
            number_end = i;
            break;
        } else {
            // Not a numbered heading pattern
            return None;
        }
    }

    // Must have at least one number group with a dot (e.g., "1." or "1.1")
    // Check that the prefix contains at least one dot
    let prefix = &trimmed[..number_end];
    if !prefix.contains('.') {
        return None;
    }

    // The remaining text after the number should exist and be short-ish
    let remaining = trimmed[number_end..].trim();
    if remaining.is_empty() {
        return None;
    }

    // A heading shouldn't be too long
    if remaining.len() > 100 {
        return None;
    }

    Some(DetectedHeading {
        text: trimmed.to_string(),
        level: number_group_count.min(6),
    })
}

/// Check if a string is an ALL CAPS heading.
///
/// Requirements:
/// - All alphabetic characters are uppercase
/// - At least 2 alphabetic characters
/// - Not just numbers/symbols
fn is_all_caps_heading(text: &str) -> bool {
    let alpha_count = text.chars().filter(|c| c.is_alphabetic()).count();
    if alpha_count < 2 {
        return false;
    }
    text.chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_uppercase())
}

/// Detect headings from word-level font size data.
///
/// Uses the median font size as the "body" font size. Words significantly
/// larger than the median are classified as headings. The heading level
/// is determined by how much larger the font is relative to the body.
///
/// Returns `(heading_text, heading_level)` pairs.
pub fn detect_headings_from_font_size(words: &[PdfWord]) -> Vec<(String, u32)> {
    if words.is_empty() {
        return Vec::new();
    }

    // Collect all font sizes to find the median (body text size).
    let mut sizes: Vec<f32> = words.iter().map(|w| w.font_size).collect();
    sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_size = sizes[sizes.len() / 2];
    if median_size <= 0.0 {
        return Vec::new();
    }

    // Group consecutive words that share a similar large font size into heading lines.
    let threshold = median_size * 1.2; // 20% larger than body text

    let mut headings: Vec<(String, u32)> = Vec::new();
    let mut current_heading_words: Vec<&PdfWord> = Vec::new();
    let mut current_size: f32 = 0.0;

    for word in words {
        if word.font_size >= threshold {
            if current_heading_words.is_empty()
                || (word.font_size - current_size).abs() < median_size * 0.1
            {
                current_heading_words.push(word);
                current_size = word.font_size;
            } else {
                // Different size -> flush and start new group
                if !current_heading_words.is_empty() {
                    let text: String = current_heading_words
                        .iter()
                        .map(|w| w.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let level = font_size_to_heading_level(current_size, median_size);
                    headings.push((text, level));
                }
                current_heading_words = vec![word];
                current_size = word.font_size;
            }
        } else {
            // Flush current heading group
            if !current_heading_words.is_empty() {
                let text: String = current_heading_words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let level = font_size_to_heading_level(current_size, median_size);
                headings.push((text, level));
                current_heading_words.clear();
            }
        }
    }

    // Flush trailing heading group
    if !current_heading_words.is_empty() {
        let text: String = current_heading_words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let level = font_size_to_heading_level(current_size, median_size);
        headings.push((text, level));
    }

    headings
}

/// Map a font size ratio to a heading level (1-6).
fn font_size_to_heading_level(heading_size: f32, body_size: f32) -> u32 {
    if body_size <= 0.0 {
        return 2;
    }
    let ratio = heading_size / body_size;
    if ratio >= 2.0 {
        1
    } else if ratio >= 1.6 {
        2
    } else if ratio >= 1.4 {
        3
    } else if ratio >= 1.3 {
        4
    } else if ratio >= 1.2 {
        5
    } else {
        6
    }
}

/// Group words into lines based on vertical proximity.
///
/// Words whose vertical centers are within `tolerance` points of each other
/// are grouped into the same line. Lines are sorted top-to-bottom.
pub fn words_to_lines(words: &[PdfWord], tolerance: f64) -> Vec<Vec<&PdfWord>> {
    if words.is_empty() {
        return Vec::new();
    }

    // Sort words by vertical center, then by left edge
    let mut indexed: Vec<(usize, f64)> = words
        .iter()
        .enumerate()
        .map(|(i, w)| (i, (w.bbox.top + w.bbox.bottom) / 2.0))
        .collect();
    indexed.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                words[a.0]
                    .bbox
                    .left
                    .partial_cmp(&words[b.0].bbox.left)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let mut lines: Vec<Vec<&PdfWord>> = Vec::new();
    let mut current_line: Vec<&PdfWord> = Vec::new();
    let mut current_y = f64::NEG_INFINITY;

    for (idx, y_center) in &indexed {
        if (y_center - current_y).abs() > tolerance {
            // New line
            if !current_line.is_empty() {
                lines.push(current_line);
                current_line = Vec::new();
            }
            current_y = *y_center;
        }
        current_line.push(&words[*idx]);
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // Sort words within each line by left edge
    for line in &mut lines {
        line.sort_by(|a, b| {
            a.bbox
                .left
                .partial_cmp(&b.bbox.left)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    lines
}

/// Group lines into paragraphs using vertical spacing heuristics.
///
/// Lines separated by more than `line_spacing_factor` times the average
/// line height are treated as belonging to different paragraphs.
pub fn lines_to_paragraphs(lines: &[Vec<&PdfWord>]) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }

    if lines.len() == 1 {
        let text: String = lines[0]
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        return vec![text];
    }

    // Compute the bottom of each line and the gap to the next line.
    let line_bottoms: Vec<f64> = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|w| w.bbox.bottom)
                .fold(f64::NEG_INFINITY, f64::max)
        })
        .collect();
    let line_tops: Vec<f64> = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|w| w.bbox.top)
                .fold(f64::INFINITY, f64::min)
        })
        .collect();

    // Compute average line height
    let avg_line_height: f64 = lines
        .iter()
        .enumerate()
        .map(|(i, _)| (line_bottoms[i] - line_tops[i]).abs())
        .sum::<f64>()
        / lines.len() as f64;

    let gap_threshold = avg_line_height * 1.5;

    let mut paragraphs: Vec<String> = Vec::new();
    let mut current_para: Vec<String> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let line_text: String = line
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        current_para.push(line_text);

        // Check gap to next line
        if i + 1 < lines.len() {
            let gap = (line_tops[i + 1] - line_bottoms[i]).abs();
            if gap > gap_threshold {
                paragraphs.push(current_para.join(" "));
                current_para = Vec::new();
            }
        }
    }

    if !current_para.is_empty() {
        paragraphs.push(current_para.join(" "));
    }

    paragraphs
}

// -----------------------------------------------------------------------
// DocumentBackend implementation (lopdf-based, always available)
// -----------------------------------------------------------------------

impl DocumentBackend for PdfBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Pdf]
    }

    fn supports_pagination(&self) -> bool {
        true
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(path.extension().and_then(|e| e.to_str()), Some("pdf"))
            }
            DocumentSource::Stream { name, .. } => name.ends_with(".pdf"),
        }
    }

    fn parse(
        &self,
        source: &DocumentSource,
        store: &dyn DocumentStore,
        doc_graph: &str,
    ) -> ruddydoc_core::Result<DocumentMeta> {
        // Read the source bytes
        let (bytes, file_path, file_name) = match source {
            DocumentSource::File(path) => {
                let bytes = std::fs::read(path)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown.pdf".to_string());
                (bytes, Some(path.clone()), name)
            }
            DocumentSource::Stream { name, data } => (data.clone(), None, name.clone()),
        };

        let file_size = bytes.len() as u64;
        let hash_str = compute_hash(&bytes);
        let doc_hash = DocumentHash(hash_str.clone());

        // Load the PDF
        let pdf = lopdf::Document::load_mem(&bytes)?;
        let pages = pdf.get_pages();
        let page_count = pages.len() as u32;

        // Create the document node
        let doc_iri = ruddydoc_core::doc_iri(&hash_str);
        let rdf_type = ont::rdf_iri("type");
        let g = doc_graph;

        store.insert_triple_into(&doc_iri, &rdf_type, &ont::iri(ont::CLASS_DOCUMENT), g)?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_SOURCE_FORMAT),
            "pdf",
            "string",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_DOCUMENT_HASH),
            &hash_str,
            "string",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_FILE_NAME),
            &file_name,
            "string",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_FILE_SIZE),
            &file_size.to_string(),
            "integer",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_PAGE_COUNT),
            &page_count.to_string(),
            "integer",
            g,
        )?;

        // Extract metadata from the PDF Info dictionary
        extract_pdf_metadata(&pdf, store, &doc_iri, g)?;

        // Process each page
        let mut reading_order: usize = 0;
        let mut all_page_iris: Vec<String> = Vec::new();
        let mut all_element_iris: Vec<String> = Vec::new();

        for (&page_num, &page_id) in &pages {
            // Create the Page node
            let page_iri = ruddydoc_core::element_iri(&hash_str, &format!("page-{page_num}"));
            store.insert_triple_into(&page_iri, &rdf_type, &ont::iri(ont::CLASS_PAGE), g)?;
            store.insert_literal(
                &page_iri,
                &ont::iri(ont::PROP_PAGE_NUMBER),
                &page_num.to_string(),
                "integer",
                g,
            )?;

            // Page dimensions from MediaBox
            if let Some((width, height)) = extract_page_dimensions(&pdf, page_id) {
                store.insert_literal(
                    &page_iri,
                    &ont::iri(ont::PROP_PAGE_WIDTH),
                    &format!("{width}"),
                    "float",
                    g,
                )?;
                store.insert_literal(
                    &page_iri,
                    &ont::iri(ont::PROP_PAGE_HEIGHT),
                    &format!("{height}"),
                    "float",
                    g,
                )?;
            }

            // Link document -> page
            store.insert_triple_into(&doc_iri, &ont::iri(ont::PROP_HAS_PAGE), &page_iri, g)?;

            // Link previous/next pages
            if let Some(prev_page_iri) = all_page_iris.last() {
                store.insert_triple_into(
                    prev_page_iri,
                    &ont::iri(ont::PROP_NEXT_ELEMENT),
                    &page_iri,
                    g,
                )?;
                store.insert_triple_into(
                    &page_iri,
                    &ont::iri(ont::PROP_PREVIOUS_ELEMENT),
                    prev_page_iri,
                    g,
                )?;
            }
            all_page_iris.push(page_iri.clone());

            // Extract text from the page
            let page_text = pdf.extract_text(&[page_num]).unwrap_or_default();
            let paragraphs = split_into_paragraphs(&page_text);

            // Detect headings from the paragraph text
            let detected_headings = detect_headings_from_text(&paragraphs);
            let heading_texts: std::collections::HashSet<&str> =
                detected_headings.iter().map(|h| h.text.as_str()).collect();

            for (para_idx, para_text) in paragraphs.iter().enumerate() {
                // Check if this paragraph was detected as a heading
                let is_heading = heading_texts.contains(para_text.as_str());
                let heading_info = if is_heading {
                    detected_headings.iter().find(|h| h.text == *para_text)
                } else {
                    None
                };

                let (element_class, element_tag) = if is_heading {
                    (ont::CLASS_SECTION_HEADER, "header")
                } else {
                    (ont::CLASS_PARAGRAPH, "paragraph")
                };

                let para_iri = ruddydoc_core::element_iri(
                    &hash_str,
                    &format!("{element_tag}-{page_num}-{para_idx}"),
                );

                // rdf:type
                store.insert_triple_into(&para_iri, &rdf_type, &ont::iri(element_class), g)?;

                // rdoc:textContent
                store.insert_literal(
                    &para_iri,
                    &ont::iri(ont::PROP_TEXT_CONTENT),
                    para_text,
                    "string",
                    g,
                )?;

                // rdoc:headingLevel (for detected headings)
                if let Some(heading) = heading_info {
                    store.insert_literal(
                        &para_iri,
                        &ont::iri(ont::PROP_HEADING_LEVEL),
                        &heading.level.to_string(),
                        "integer",
                        g,
                    )?;

                    // Confidence for heuristic-detected headings
                    store.insert_literal(
                        &para_iri,
                        &ont::iri(ont::PROP_CONFIDENCE),
                        "0.6",
                        "float",
                        g,
                    )?;
                }

                // rdoc:readingOrder
                store.insert_literal(
                    &para_iri,
                    &ont::iri(ont::PROP_READING_ORDER),
                    &reading_order.to_string(),
                    "integer",
                    g,
                )?;

                // rdoc:onPage
                store.insert_triple_into(&para_iri, &ont::iri(ont::PROP_ON_PAGE), &page_iri, g)?;

                // rdoc:hasElement (document -> paragraph)
                store.insert_triple_into(
                    &doc_iri,
                    &ont::iri(ont::PROP_HAS_ELEMENT),
                    &para_iri,
                    g,
                )?;

                // Previous/next element links
                if let Some(prev_iri) = all_element_iris.last() {
                    store.insert_triple_into(
                        prev_iri,
                        &ont::iri(ont::PROP_NEXT_ELEMENT),
                        &para_iri,
                        g,
                    )?;
                    store.insert_triple_into(
                        &para_iri,
                        &ont::iri(ont::PROP_PREVIOUS_ELEMENT),
                        prev_iri,
                        g,
                    )?;
                }
                all_element_iris.push(para_iri);
                reading_order += 1;
            }
        }

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Pdf,
            file_size,
            page_count: Some(page_count),
        })
    }
}

/// Extract PDF metadata from the Info dictionary and insert as Dublin Core
/// triples on the document node.
fn extract_pdf_metadata(
    pdf: &lopdf::Document,
    store: &dyn DocumentStore,
    doc_iri: &str,
    graph: &str,
) -> ruddydoc_core::Result<()> {
    // The Info dictionary is referenced from the trailer
    let info_ref = match pdf.trailer.get(b"Info") {
        Ok(obj) => obj,
        Err(_) => return Ok(()), // No Info dictionary; not an error
    };

    let info_id = match info_ref.as_reference() {
        Ok(id) => id,
        Err(_) => return Ok(()), // Info is not a reference; unusual but skip
    };

    let info_dict = match pdf.get_dictionary(info_id) {
        Ok(dict) => dict,
        Err(_) => return Ok(()), // Could not resolve; skip
    };

    // Title -> dcterms:title
    if let Ok(bytes) = info_dict.get(b"Title").and_then(|o| o.as_str()) {
        let title = decode_pdf_string(bytes);
        if !title.is_empty() {
            store.insert_literal(doc_iri, &dcterms_iri("title"), &title, "string", graph)?;
        }
    }

    // Author -> dcterms:creator
    if let Ok(bytes) = info_dict.get(b"Author").and_then(|o| o.as_str()) {
        let author = decode_pdf_string(bytes);
        if !author.is_empty() {
            store.insert_literal(doc_iri, &dcterms_iri("creator"), &author, "string", graph)?;
        }
    }

    // CreationDate -> dcterms:date
    if let Ok(bytes) = info_dict.get(b"CreationDate").and_then(|o| o.as_str()) {
        let raw_date = decode_pdf_string(bytes);
        if !raw_date.is_empty() {
            let date = parse_pdf_date(&raw_date);
            store.insert_literal(doc_iri, &dcterms_iri("date"), &date, "string", graph)?;
        }
    }

    // Subject -> dcterms:description
    if let Ok(bytes) = info_dict.get(b"Subject").and_then(|o| o.as_str()) {
        let subject = decode_pdf_string(bytes);
        if !subject.is_empty() {
            store.insert_literal(
                doc_iri,
                &dcterms_iri("description"),
                &subject,
                "string",
                graph,
            )?;
        }
    }

    Ok(())
}

// -----------------------------------------------------------------------
// PDFium backend (feature-gated)
// -----------------------------------------------------------------------

/// PDFium-enhanced backend providing word-level extraction and page rendering.
///
/// This module is only available when the `pdfium` feature is enabled.
#[cfg(feature = "pdfium")]
pub mod pdfium_backend {
    use super::*;
    use pdfium_render::prelude::*;

    /// Extract words with positions from a PDFium page.
    ///
    /// Each character is extracted with its bounding box, font name, and size.
    /// Characters are grouped into words by whitespace boundaries.
    pub fn extract_words(page: &PdfPage) -> Vec<PdfWord> {
        let mut words: Vec<PdfWord> = Vec::new();
        let text = page.text().ok();
        let text_page = match text {
            Some(ref t) => t,
            None => return words,
        };

        let mut current_word = String::new();
        let mut word_left = f64::INFINITY;
        let mut word_top = f64::INFINITY;
        let mut word_right = f64::NEG_INFINITY;
        let mut word_bottom = f64::NEG_INFINITY;
        let mut word_font_name = String::new();
        let mut word_font_size: f32 = 0.0;
        let mut word_is_bold = false;
        let mut word_is_italic = false;

        for char_info in text_page.chars().iter() {
            let ch = char_info.unicode_char();

            if ch.is_whitespace() {
                // Flush current word
                if !current_word.is_empty() {
                    words.push(PdfWord {
                        text: current_word.clone(),
                        bbox: BoundingBox {
                            left: word_left,
                            top: word_top,
                            right: word_right,
                            bottom: word_bottom,
                        },
                        font_name: word_font_name.clone(),
                        font_size: word_font_size,
                        is_bold: word_is_bold,
                        is_italic: word_is_italic,
                    });
                    current_word.clear();
                    word_left = f64::INFINITY;
                    word_top = f64::INFINITY;
                    word_right = f64::NEG_INFINITY;
                    word_bottom = f64::NEG_INFINITY;
                }
                continue;
            }

            // Accumulate into current word
            current_word.push(ch);

            if let Some(rect) = char_info.tight_bounds() {
                let bounds = rect;
                let left = bounds.left.value as f64;
                let top = bounds.top.value as f64;
                let right = bounds.right.value as f64;
                let bottom = bounds.bottom.value as f64;

                if left < word_left {
                    word_left = left;
                }
                if top < word_top {
                    word_top = top;
                }
                if right > word_right {
                    word_right = right;
                }
                if bottom > word_bottom {
                    word_bottom = bottom;
                }
            }

            // Font info from the first character in the word
            if current_word.len() == 1 {
                if let Some(font) = char_info.font() {
                    word_font_name = font.name().unwrap_or_default();
                    word_is_bold = font.is_bold();
                    word_is_italic = font.is_italic();
                }
                word_font_size = char_info.font_size().value;
            }
        }

        // Flush trailing word
        if !current_word.is_empty() {
            words.push(PdfWord {
                text: current_word,
                bbox: BoundingBox {
                    left: word_left,
                    top: word_top,
                    right: word_right,
                    bottom: word_bottom,
                },
                font_name: word_font_name,
                font_size: word_font_size,
                is_bold: word_is_bold,
                is_italic: word_is_italic,
            });
        }

        words
    }

    /// Render a PDFium page to an RGB image at the given DPI.
    pub fn render_page(
        page: &PdfPage,
        page_number: u32,
        dpi: f32,
    ) -> ruddydoc_core::Result<RenderedPage> {
        let scale = dpi / 72.0;
        let width_pts = page.width().value;
        let height_pts = page.height().value;
        let pixel_width = (width_pts * scale) as u32;
        let pixel_height = (height_pts * scale) as u32;

        let config = PdfRenderConfig::new()
            .set_target_width(pixel_width as i32)
            .set_target_height(pixel_height as i32);

        let bitmap = page.render_with_config(&config)?;
        let image = bitmap.as_image();
        let rgb_image = image.to_rgb8();
        let rgb_data = rgb_image.into_raw();

        Ok(RenderedPage {
            page_number,
            width: pixel_width,
            height: pixel_height,
            rgb_data,
            scale,
        })
    }

    /// Render all pages of a PDF from a document source.
    pub fn render_all_pages(
        source: &DocumentSource,
        dpi: f32,
    ) -> ruddydoc_core::Result<Vec<RenderedPage>> {
        let bytes = match source {
            DocumentSource::File(path) => std::fs::read(path)?,
            DocumentSource::Stream { data, .. } => data.clone(),
        };

        let pdfium = Pdfium::default();
        let document = pdfium.load_pdf_from_byte_slice(&bytes, None)?;
        let mut rendered = Vec::new();

        for (i, page) in document.pages().iter().enumerate() {
            let rp = render_page(&page, (i + 1) as u32, dpi)?;
            rendered.push(rp);
        }

        Ok(rendered)
    }

    /// Group words into lines using vertical proximity.
    ///
    /// Convenience wrapper around the crate-level `words_to_lines`.
    pub fn group_words_to_lines(words: &[PdfWord]) -> Vec<Vec<&PdfWord>> {
        // Default tolerance: 3 points
        super::words_to_lines(words, 3.0)
    }

    /// Group lines into paragraphs using spacing heuristics.
    ///
    /// Convenience wrapper around the crate-level `lines_to_paragraphs`.
    pub fn group_lines_to_paragraphs(lines: &[Vec<&PdfWord>]) -> Vec<String> {
        super::lines_to_paragraphs(lines)
    }

    /// Detect headings from font size relative to body text.
    ///
    /// Returns `(heading_text, heading_level)` pairs.
    pub fn detect_headings(words: &[PdfWord]) -> Vec<(String, u32)> {
        super::detect_headings_from_font_size(words)
    }
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;

    /// Build a minimal valid PDF with the given text content on one page,
    /// optionally with metadata in the Info dictionary.
    fn build_test_pdf(text: &str, title: Option<&str>, author: Option<&str>) -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Object, Stream};

        let mut doc = lopdf::Document::new();
        doc.version = "1.4".to_string();

        // Font: Helvetica
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });

        // Content stream: render text
        let content_str = format!("BT /F1 12 Tf 100 700 Td ({text}) Tj ET");
        let content_bytes = content_str.into_bytes();
        let content_stream = Stream::new(
            dictionary! { "Length" => content_bytes.len() as i64 },
            content_bytes,
        );
        let content_id = doc.add_object(content_stream);

        // Page
        let resources = dictionary! {
            "Font" => dictionary! {
                "F1" => Object::Reference(font_id),
            },
        };
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), Object::Real(612.0), Object::Real(792.0)],
            "Contents" => Object::Reference(content_id),
            "Resources" => resources,
        });

        // Pages
        let pages_id = doc.add_object(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        });

        // Set parent on the page
        if let Ok(page_dict) = doc.get_object_mut(page_id) {
            if let Ok(dict) = page_dict.as_dict_mut() {
                dict.set("Parent", Object::Reference(pages_id));
            }
        }

        // Catalog
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });

        // Info dictionary (metadata)
        let mut info = lopdf::Dictionary::new();
        if let Some(t) = title {
            info.set(
                "Title",
                Object::String(t.as_bytes().to_vec(), lopdf::StringFormat::Literal),
            );
        }
        if let Some(a) = author {
            info.set(
                "Author",
                Object::String(a.as_bytes().to_vec(), lopdf::StringFormat::Literal),
            );
        }
        let info_id = doc.add_object(info);

        // Trailer
        doc.trailer.set("Root", Object::Reference(catalog_id));
        doc.trailer.set("Info", Object::Reference(info_id));

        let mut buf = Vec::new();
        doc.save_to(&mut buf).expect("failed to save test PDF");
        buf
    }

    /// Build a multi-page test PDF.
    fn build_multi_page_pdf(page_texts: &[&str]) -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Object, Stream};

        let mut doc = lopdf::Document::new();
        doc.version = "1.4".to_string();

        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });

        let mut page_ids = Vec::new();
        for text in page_texts {
            let content_str = format!("BT /F1 12 Tf 100 700 Td ({text}) Tj ET");
            let content_bytes = content_str.into_bytes();
            let content_stream = Stream::new(
                dictionary! { "Length" => content_bytes.len() as i64 },
                content_bytes,
            );
            let content_id = doc.add_object(content_stream);

            let resources = dictionary! {
                "Font" => dictionary! {
                    "F1" => Object::Reference(font_id),
                },
            };
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "MediaBox" => vec![0.into(), 0.into(), Object::Real(612.0), Object::Real(792.0)],
                "Contents" => Object::Reference(content_id),
                "Resources" => resources,
            });
            page_ids.push(page_id);
        }

        let kids: Vec<Object> = page_ids.iter().map(|id| Object::Reference(*id)).collect();
        let pages_id = doc.add_object(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => page_ids.len() as i64,
        });

        // Set parent on each page
        for &pid in &page_ids {
            if let Ok(page_obj) = doc.get_object_mut(pid) {
                if let Ok(dict) = page_obj.as_dict_mut() {
                    dict.set("Parent", Object::Reference(pages_id));
                }
            }
        }

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        });

        doc.trailer.set("Root", Object::Reference(catalog_id));

        let mut buf = Vec::new();
        doc.save_to(&mut buf).expect("failed to save test PDF");
        buf
    }

    fn parse_pdf(data: &[u8]) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = PdfBackend::new();
        let source = DocumentSource::Stream {
            name: "test.pdf".to_string(),
            data: data.to_vec(),
        };

        let hash_str = compute_hash(data);
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    // ===================================================================
    // Original tests (preserved)
    // ===================================================================

    #[test]
    fn is_valid_accepts_pdf_extension() {
        let backend = PdfBackend::new();
        let source = DocumentSource::File(std::path::PathBuf::from("document.pdf"));
        assert!(backend.is_valid(&source));
    }

    #[test]
    fn is_valid_rejects_non_pdf() {
        let backend = PdfBackend::new();
        let source = DocumentSource::File(std::path::PathBuf::from("document.md"));
        assert!(!backend.is_valid(&source));
    }

    #[test]
    fn is_valid_stream_pdf() {
        let backend = PdfBackend::new();
        let source = DocumentSource::Stream {
            name: "report.pdf".to_string(),
            data: vec![],
        };
        assert!(backend.is_valid(&source));
    }

    #[test]
    fn is_valid_stream_non_pdf() {
        let backend = PdfBackend::new();
        let source = DocumentSource::Stream {
            name: "report.docx".to_string(),
            data: vec![],
        };
        assert!(!backend.is_valid(&source));
    }

    #[test]
    fn supports_pagination_is_true() {
        let backend = PdfBackend::new();
        assert!(backend.supports_pagination());
    }

    #[test]
    fn supported_formats_contains_pdf() {
        let backend = PdfBackend::new();
        assert_eq!(backend.supported_formats(), &[InputFormat::Pdf]);
    }

    #[test]
    fn parse_extracts_text() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Hello World", None, None);
        let (store, _meta, graph) = parse_pdf(&pdf_bytes)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(!rows.is_empty(), "expected at least one paragraph");

        let all_text: String = rows
            .iter()
            .filter_map(|r| r["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            all_text.contains("Hello World"),
            "expected 'Hello World' in extracted text, got: {all_text}"
        );

        Ok(())
    }

    #[test]
    fn parse_page_count() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Test", None, None);
        let (_store, meta, _graph) = parse_pdf(&pdf_bytes)?;

        assert_eq!(meta.format, InputFormat::Pdf);
        assert_eq!(meta.page_count, Some(1));

        Ok(())
    }

    #[test]
    fn parse_multi_page_count() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_multi_page_pdf(&["Page one", "Page two", "Page three"]);
        let (_store, meta, _graph) = parse_pdf(&pdf_bytes)?;

        assert_eq!(meta.page_count, Some(3));

        Ok(())
    }

    #[test]
    fn page_count_in_graph() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_multi_page_pdf(&["A", "B"]);
        let (store, meta, graph) = parse_pdf(&pdf_bytes)?;

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?count WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?count \
               }} \
             }}",
            ont::iri(ont::PROP_PAGE_COUNT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let count_str = rows[0]["count"].as_str().expect("count");
        assert!(count_str.contains('2'));

        Ok(())
    }

    #[test]
    fn pages_have_page_number() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_multi_page_pdf(&["First", "Second"]);
        let (store, _meta, graph) = parse_pdf(&pdf_bytes)?;

        let sparql = format!(
            "SELECT ?num WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?page a <{}>. \
                 ?page <{}> ?num \
               }} \
             }} ORDER BY ?num",
            ont::iri(ont::CLASS_PAGE),
            ont::iri(ont::PROP_PAGE_NUMBER),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);

        Ok(())
    }

    #[test]
    fn paragraphs_have_on_page() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Sample text", None, None);
        let (store, _meta, graph) = parse_pdf(&pdf_bytes)?;

        let sparql = format!(
            "SELECT ?para ?page WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?para a <{}>. \
                 ?para <{}> ?page. \
                 ?page a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_ON_PAGE),
            ont::iri(ont::CLASS_PAGE),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(
            !rows.is_empty(),
            "expected paragraphs linked to pages via onPage"
        );

        Ok(())
    }

    #[test]
    fn document_metadata_title() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Content", Some("My Document Title"), None);
        let (store, meta, graph) = parse_pdf(&pdf_bytes)?;

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?title WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?title \
               }} \
             }}",
            dcterms_iri("title"),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let title = rows[0]["title"].as_str().expect("title");
        assert!(title.contains("My Document Title"));

        Ok(())
    }

    #[test]
    fn document_metadata_author() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Content", None, Some("Jane Doe"));
        let (store, meta, graph) = parse_pdf(&pdf_bytes)?;

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?creator WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?creator \
               }} \
             }}",
            dcterms_iri("creator"),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let creator = rows[0]["creator"].as_str().expect("creator");
        assert!(creator.contains("Jane Doe"));

        Ok(())
    }

    #[test]
    fn page_dimensions() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Dimensions test", None, None);
        let (store, _meta, graph) = parse_pdf(&pdf_bytes)?;

        let sparql = format!(
            "SELECT ?w ?h WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?page a <{}>. \
                 ?page <{}> ?w. \
                 ?page <{}> ?h \
               }} \
             }}",
            ont::iri(ont::CLASS_PAGE),
            ont::iri(ont::PROP_PAGE_WIDTH),
            ont::iri(ont::PROP_PAGE_HEIGHT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        // US Letter: 612 x 792 points
        let width_str = rows[0]["w"].as_str().expect("width");
        assert!(
            width_str.contains("612"),
            "expected width ~612, got: {width_str}"
        );
        let height_str = rows[0]["h"].as_str().expect("height");
        assert!(
            height_str.contains("792"),
            "expected height ~792, got: {height_str}"
        );

        Ok(())
    }

    #[test]
    fn error_on_invalid_pdf() {
        let backend = PdfBackend::new();
        let source = DocumentSource::Stream {
            name: "bad.pdf".to_string(),
            data: b"this is not a valid pdf".to_vec(),
        };
        let store = OxigraphStore::new().expect("store");
        let graph = "urn:test:graph";
        let result = backend.parse(&source, &store, graph);
        assert!(result.is_err());
    }

    #[test]
    fn document_source_format_is_pdf() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Format check", None, None);
        let (store, meta, graph) = parse_pdf(&pdf_bytes)?;

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?fmt WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?fmt \
               }} \
             }}",
            ont::iri(ont::PROP_SOURCE_FORMAT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let fmt = rows[0]["fmt"].as_str().expect("format");
        assert!(fmt.contains("pdf"));

        Ok(())
    }

    #[test]
    fn reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Hello World", None, None);
        let (store, _meta, graph) = parse_pdf(&pdf_bytes)?;

        let sparql = format!(
            "SELECT ?order WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{}> ?order \
               }} \
             }} ORDER BY ?order",
            ont::iri(ont::PROP_READING_ORDER),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(!rows.is_empty());

        // Verify orders are sequential starting from 0
        for (i, row) in rows.iter().enumerate() {
            let order = row["order"].as_str().expect("order");
            assert!(
                order.contains(&i.to_string()),
                "expected order {i}, got: {order}"
            );
        }

        Ok(())
    }

    #[test]
    fn paragraph_splitting() {
        let text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let paras = split_into_paragraphs(text);
        assert_eq!(paras.len(), 3);
        assert_eq!(paras[0], "First paragraph.");
        assert_eq!(paras[1], "Second paragraph.");
        assert_eq!(paras[2], "Third paragraph.");
    }

    #[test]
    fn paragraph_splitting_trims_whitespace() {
        let text = "  Leading spaces.  \n\n  Trailing spaces.  ";
        let paras = split_into_paragraphs(text);
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0], "Leading spaces.");
        assert_eq!(paras[1], "Trailing spaces.");
    }

    #[test]
    fn paragraph_splitting_skips_empty() {
        let text = "\n\n\n\nOnly one.\n\n\n\n";
        let paras = split_into_paragraphs(text);
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0], "Only one.");
    }

    #[test]
    fn parse_pdf_date_full() {
        let result = parse_pdf_date("D:20231201120000+00'00'");
        assert_eq!(result, "2023-12-01");
    }

    #[test]
    fn parse_pdf_date_short() {
        let result = parse_pdf_date("D:20231201");
        assert_eq!(result, "2023-12-01");
    }

    #[test]
    fn parse_pdf_date_no_prefix() {
        let result = parse_pdf_date("20231201");
        assert_eq!(result, "2023-12-01");
    }

    #[test]
    fn decode_pdf_string_utf8() {
        let result = decode_pdf_string(b"Hello");
        assert_eq!(result, "Hello");
    }

    #[test]
    fn decode_pdf_string_utf16be() {
        // UTF-16BE BOM + "Hi"
        let bytes: Vec<u8> = vec![0xFE, 0xFF, 0x00, b'H', 0x00, b'i'];
        let result = decode_pdf_string(&bytes);
        assert_eq!(result, "Hi");
    }

    #[test]
    fn default_implementation() {
        let backend = PdfBackend::default();
        assert!(backend.supports_pagination());
    }

    #[test]
    fn document_has_element_links() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Linked text", None, None);
        let (store, meta, graph) = parse_pdf(&pdf_bytes)?;

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?el WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?el \
               }} \
             }}",
            ont::iri(ont::PROP_HAS_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(!rows.is_empty(), "document should have hasElement links");

        Ok(())
    }

    #[test]
    fn document_has_page_links() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_multi_page_pdf(&["Page A", "Page B"]);
        let (store, meta, graph) = parse_pdf(&pdf_bytes)?;

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?page WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?page \
               }} \
             }}",
            ont::iri(ont::PROP_HAS_PAGE),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 2);

        Ok(())
    }

    #[test]
    fn file_hash_and_size() -> ruddydoc_core::Result<()> {
        let pdf_bytes = build_test_pdf("Hash test", None, None);
        let (_store, meta, _graph) = parse_pdf(&pdf_bytes)?;

        assert!(!meta.hash.0.is_empty());
        assert!(meta.file_size > 0);
        assert_eq!(meta.file_size, pdf_bytes.len() as u64);

        Ok(())
    }

    // ===================================================================
    // New tests: enhanced types
    // ===================================================================

    #[test]
    fn pdf_word_creation_and_fields() {
        let word = PdfWord {
            text: "Hello".to_string(),
            bbox: BoundingBox {
                left: 10.0,
                top: 20.0,
                right: 50.0,
                bottom: 35.0,
            },
            font_name: "Helvetica".to_string(),
            font_size: 12.0,
            is_bold: false,
            is_italic: false,
        };
        assert_eq!(word.text, "Hello");
        assert_eq!(word.font_name, "Helvetica");
        assert!((word.font_size - 12.0).abs() < f32::EPSILON);
        assert!(!word.is_bold);
        assert!(!word.is_italic);
    }

    #[test]
    fn pdf_word_serialization_roundtrip() {
        let word = PdfWord {
            text: "Test".to_string(),
            bbox: BoundingBox {
                left: 1.0,
                top: 2.0,
                right: 3.0,
                bottom: 4.0,
            },
            font_name: "Arial".to_string(),
            font_size: 14.0,
            is_bold: true,
            is_italic: false,
        };
        let json = serde_json::to_string(&word).expect("should serialize PdfWord");
        let deserialized: PdfWord =
            serde_json::from_str(&json).expect("should deserialize PdfWord");
        assert_eq!(deserialized.text, "Test");
        assert_eq!(deserialized.font_name, "Arial");
        assert!(deserialized.is_bold);
        assert!(!deserialized.is_italic);
        assert!((deserialized.font_size - 14.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pdf_word_clone() {
        let word = PdfWord {
            text: "Clone".to_string(),
            bbox: BoundingBox {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0,
            },
            font_name: "Times".to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: true,
        };
        let cloned = word.clone();
        assert_eq!(cloned.text, word.text);
        assert_eq!(cloned.font_name, word.font_name);
        assert!(cloned.is_italic);
    }

    #[test]
    fn rendered_page_creation() {
        let page = RenderedPage {
            page_number: 1,
            width: 612,
            height: 792,
            rgb_data: vec![0u8; 612 * 792 * 3],
            scale: 1.0,
        };
        assert_eq!(page.page_number, 1);
        assert_eq!(page.width, 612);
        assert_eq!(page.height, 792);
        assert_eq!(page.rgb_data.len(), 612 * 792 * 3);
        assert!((page.scale - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rendered_page_clone() {
        let page = RenderedPage {
            page_number: 2,
            width: 100,
            height: 100,
            rgb_data: vec![128u8; 100 * 100 * 3],
            scale: 2.0,
        };
        let cloned = page.clone();
        assert_eq!(cloned.page_number, page.page_number);
        assert_eq!(cloned.width, page.width);
        assert_eq!(cloned.height, page.height);
        assert_eq!(cloned.rgb_data.len(), page.rgb_data.len());
        assert!((cloned.scale - page.scale).abs() < f32::EPSILON);
    }

    #[test]
    fn embedded_image_creation() {
        let img = EmbeddedImage {
            page_number: 3,
            bbox: BoundingBox {
                left: 50.0,
                top: 100.0,
                right: 200.0,
                bottom: 300.0,
            },
            format: "jpeg".to_string(),
            data: vec![0xFF, 0xD8, 0xFF],
        };
        assert_eq!(img.page_number, 3);
        assert_eq!(img.format, "jpeg");
        assert_eq!(img.data.len(), 3);
    }

    #[test]
    fn embedded_image_clone() {
        let img = EmbeddedImage {
            page_number: 1,
            bbox: BoundingBox {
                left: 0.0,
                top: 0.0,
                right: 100.0,
                bottom: 100.0,
            },
            format: "png".to_string(),
            data: vec![0x89, 0x50, 0x4E, 0x47],
        };
        let cloned = img.clone();
        assert_eq!(cloned.format, img.format);
        assert_eq!(cloned.data, img.data);
    }

    #[test]
    fn pdf_link_creation() {
        let link = PdfLink {
            page_number: 1,
            bbox: BoundingBox {
                left: 10.0,
                top: 20.0,
                right: 100.0,
                bottom: 30.0,
            },
            uri: "https://example.com".to_string(),
        };
        assert_eq!(link.page_number, 1);
        assert_eq!(link.uri, "https://example.com");
    }

    #[test]
    fn pdf_link_clone() {
        let link = PdfLink {
            page_number: 2,
            bbox: BoundingBox {
                left: 0.0,
                top: 0.0,
                right: 50.0,
                bottom: 10.0,
            },
            uri: "https://rust-lang.org".to_string(),
        };
        let cloned = link.clone();
        assert_eq!(cloned.uri, link.uri);
        assert_eq!(cloned.page_number, link.page_number);
    }

    // ===================================================================
    // New tests: heading detection heuristics
    // ===================================================================

    #[test]
    fn detect_numbered_heading_simple() {
        let result = detect_numbered_heading("1. Introduction");
        assert!(result.is_some());
        let h = result.unwrap();
        assert_eq!(h.text, "1. Introduction");
        assert_eq!(h.level, 1);
    }

    #[test]
    fn detect_numbered_heading_subsection() {
        let result = detect_numbered_heading("1.1 Background");
        assert!(result.is_some());
        let h = result.unwrap();
        assert_eq!(h.level, 2);
    }

    #[test]
    fn detect_numbered_heading_sub_subsection() {
        let result = detect_numbered_heading("2.3.1 Details");
        assert!(result.is_some());
        let h = result.unwrap();
        assert_eq!(h.level, 3);
    }

    #[test]
    fn detect_numbered_heading_rejects_plain_text() {
        let result = detect_numbered_heading("This is just a sentence.");
        assert!(result.is_none());
    }

    #[test]
    fn detect_numbered_heading_rejects_no_text_after_number() {
        // "1." alone with nothing after it
        let result = detect_numbered_heading("1.");
        assert!(result.is_none());
    }

    #[test]
    fn is_all_caps_heading_true() {
        assert!(is_all_caps_heading("INTRODUCTION"));
        assert!(is_all_caps_heading("CHAPTER ONE"));
        assert!(is_all_caps_heading("A NOTE ON FORMATTING"));
    }

    #[test]
    fn is_all_caps_heading_false() {
        assert!(!is_all_caps_heading("Introduction"));
        assert!(!is_all_caps_heading("123"));
        assert!(!is_all_caps_heading("A")); // too short (only 1 alpha)
    }

    #[test]
    fn detect_headings_from_text_all_caps() {
        let paragraphs = vec![
            "INTRODUCTION".to_string(),
            "This is a regular paragraph with some text in it.".to_string(),
        ];
        let headings = detect_headings_from_text(&paragraphs);
        assert!(!headings.is_empty());
        assert_eq!(headings[0].text, "INTRODUCTION");
        assert_eq!(headings[0].level, 1);
    }

    #[test]
    fn detect_headings_from_text_numbered() {
        let paragraphs = vec![
            "1. Introduction".to_string(),
            "Some body text here.".to_string(),
            "1.1 Background".to_string(),
        ];
        let headings = detect_headings_from_text(&paragraphs);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].level, 1);
        assert_eq!(headings[1].level, 2);
    }

    #[test]
    fn detect_headings_from_text_short_title_like() {
        // Short-line heuristic requires >= 3 non-empty paragraphs for context
        let paragraphs = vec![
            "Summary".to_string(),
            "This is a detailed paragraph explaining many things and containing lots of words that make it clearly not a heading.".to_string(),
            "Another body paragraph providing additional context for the heading detection heuristic.".to_string(),
        ];
        let headings = detect_headings_from_text(&paragraphs);
        // "Summary" should be detected as a title-like heading
        assert!(
            headings.iter().any(|h| h.text == "Summary"),
            "expected 'Summary' to be detected as a heading"
        );
    }

    #[test]
    fn detect_headings_from_text_empty() {
        let paragraphs: Vec<String> = Vec::new();
        let headings = detect_headings_from_text(&paragraphs);
        assert!(headings.is_empty());
    }

    // ===================================================================
    // New tests: font-size-based heading detection
    // ===================================================================

    #[test]
    fn detect_headings_from_font_size_basic() {
        let words = vec![
            PdfWord {
                text: "Big".to_string(),
                bbox: BoundingBox {
                    left: 0.0,
                    top: 0.0,
                    right: 50.0,
                    bottom: 30.0,
                },
                font_name: "Helvetica-Bold".to_string(),
                font_size: 24.0,
                is_bold: true,
                is_italic: false,
            },
            PdfWord {
                text: "Title".to_string(),
                bbox: BoundingBox {
                    left: 55.0,
                    top: 0.0,
                    right: 100.0,
                    bottom: 30.0,
                },
                font_name: "Helvetica-Bold".to_string(),
                font_size: 24.0,
                is_bold: true,
                is_italic: false,
            },
            PdfWord {
                text: "normal".to_string(),
                bbox: BoundingBox {
                    left: 0.0,
                    top: 50.0,
                    right: 50.0,
                    bottom: 62.0,
                },
                font_name: "Helvetica".to_string(),
                font_size: 12.0,
                is_bold: false,
                is_italic: false,
            },
            PdfWord {
                text: "text".to_string(),
                bbox: BoundingBox {
                    left: 55.0,
                    top: 50.0,
                    right: 80.0,
                    bottom: 62.0,
                },
                font_name: "Helvetica".to_string(),
                font_size: 12.0,
                is_bold: false,
                is_italic: false,
            },
            PdfWord {
                text: "more".to_string(),
                bbox: BoundingBox {
                    left: 0.0,
                    top: 70.0,
                    right: 40.0,
                    bottom: 82.0,
                },
                font_name: "Helvetica".to_string(),
                font_size: 12.0,
                is_bold: false,
                is_italic: false,
            },
        ];
        let headings = detect_headings_from_font_size(&words);
        assert!(!headings.is_empty(), "should detect at least one heading");
        assert_eq!(headings[0].0, "Big Title");
        // 24/12 = 2.0 ratio -> level 1
        assert_eq!(headings[0].1, 1);
    }

    #[test]
    fn detect_headings_from_font_size_empty() {
        let headings = detect_headings_from_font_size(&[]);
        assert!(headings.is_empty());
    }

    #[test]
    fn detect_headings_from_font_size_uniform() {
        // All same size -> no headings (none exceed threshold)
        let words = vec![
            PdfWord {
                text: "same".to_string(),
                bbox: BoundingBox {
                    left: 0.0,
                    top: 0.0,
                    right: 30.0,
                    bottom: 12.0,
                },
                font_name: "Helvetica".to_string(),
                font_size: 12.0,
                is_bold: false,
                is_italic: false,
            },
            PdfWord {
                text: "size".to_string(),
                bbox: BoundingBox {
                    left: 35.0,
                    top: 0.0,
                    right: 60.0,
                    bottom: 12.0,
                },
                font_name: "Helvetica".to_string(),
                font_size: 12.0,
                is_bold: false,
                is_italic: false,
            },
        ];
        let headings = detect_headings_from_font_size(&words);
        assert!(
            headings.is_empty(),
            "uniform font sizes should produce no headings"
        );
    }

    // ===================================================================
    // New tests: paragraph grouping
    // ===================================================================

    #[test]
    fn paragraph_splitting_with_indentation() {
        let text = "First line.\n    Indented new paragraph.\nContinuation.";
        let paras = split_into_paragraphs(text);
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0], "First line.");
        assert_eq!(paras[1], "Indented new paragraph. Continuation.");
    }

    #[test]
    fn paragraph_splitting_joins_single_newlines() {
        let text = "Line one.\nLine two.\nLine three.";
        let paras = split_into_paragraphs(text);
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0], "Line one. Line two. Line three.");
    }

    // ===================================================================
    // New tests: words_to_lines
    // ===================================================================

    #[test]
    fn words_to_lines_empty() {
        let lines = words_to_lines(&[], 3.0);
        assert!(lines.is_empty());
    }

    #[test]
    fn words_to_lines_single_line() {
        let words = vec![
            PdfWord {
                text: "Hello".to_string(),
                bbox: BoundingBox {
                    left: 10.0,
                    top: 100.0,
                    right: 50.0,
                    bottom: 112.0,
                },
                font_name: "Helvetica".to_string(),
                font_size: 12.0,
                is_bold: false,
                is_italic: false,
            },
            PdfWord {
                text: "World".to_string(),
                bbox: BoundingBox {
                    left: 55.0,
                    top: 100.0,
                    right: 95.0,
                    bottom: 112.0,
                },
                font_name: "Helvetica".to_string(),
                font_size: 12.0,
                is_bold: false,
                is_italic: false,
            },
        ];
        let lines = words_to_lines(&words, 3.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 2);
        assert_eq!(lines[0][0].text, "Hello");
        assert_eq!(lines[0][1].text, "World");
    }

    #[test]
    fn words_to_lines_multiple_lines() {
        let words = vec![
            PdfWord {
                text: "Line1".to_string(),
                bbox: BoundingBox {
                    left: 10.0,
                    top: 100.0,
                    right: 50.0,
                    bottom: 112.0,
                },
                font_name: "Helvetica".to_string(),
                font_size: 12.0,
                is_bold: false,
                is_italic: false,
            },
            PdfWord {
                text: "Line2".to_string(),
                bbox: BoundingBox {
                    left: 10.0,
                    top: 130.0,
                    right: 50.0,
                    bottom: 142.0,
                },
                font_name: "Helvetica".to_string(),
                font_size: 12.0,
                is_bold: false,
                is_italic: false,
            },
        ];
        let lines = words_to_lines(&words, 3.0);
        assert_eq!(lines.len(), 2);
    }

    // ===================================================================
    // New tests: lines_to_paragraphs
    // ===================================================================

    #[test]
    fn lines_to_paragraphs_empty() {
        let result = lines_to_paragraphs(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn lines_to_paragraphs_single_line() {
        let word = PdfWord {
            text: "Solo".to_string(),
            bbox: BoundingBox {
                left: 10.0,
                top: 100.0,
                right: 40.0,
                bottom: 112.0,
            },
            font_name: "Helvetica".to_string(),
            font_size: 12.0,
            is_bold: false,
            is_italic: false,
        };
        let lines = vec![vec![&word]];
        let paras = lines_to_paragraphs(&lines);
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0], "Solo");
    }

    // ===================================================================
    // New tests: render_pages without pdfium feature
    // ===================================================================

    #[test]
    fn render_pages_returns_empty_without_pdfium() {
        let backend = PdfBackend::new();
        let source = DocumentSource::Stream {
            name: "test.pdf".to_string(),
            data: vec![],
        };
        let result = backend.render_pages(&source, 144.0);
        // Without the pdfium feature, this should return Ok(empty vec)
        #[cfg(not(feature = "pdfium"))]
        {
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }
        // With pdfium, it would try to parse and may fail on empty data
        #[cfg(feature = "pdfium")]
        {
            let _ = result;
        }
    }

    // ===================================================================
    // New tests: font_size_to_heading_level
    // ===================================================================

    #[test]
    fn font_size_to_heading_level_mappings() {
        // ratio >= 2.0 -> level 1
        assert_eq!(font_size_to_heading_level(24.0, 12.0), 1);
        // ratio >= 1.6 -> level 2
        assert_eq!(font_size_to_heading_level(20.0, 12.0), 2);
        // ratio >= 1.4 -> level 3
        assert_eq!(font_size_to_heading_level(17.0, 12.0), 3);
        // ratio >= 1.3 -> level 4
        assert_eq!(font_size_to_heading_level(16.0, 12.0), 4);
        // ratio >= 1.2 -> level 5
        assert_eq!(font_size_to_heading_level(15.0, 12.0), 5);
        // ratio < 1.2 -> level 6
        assert_eq!(font_size_to_heading_level(13.0, 12.0), 6);
    }

    #[test]
    fn font_size_to_heading_level_zero_body() {
        // Edge case: zero body size defaults to level 2
        assert_eq!(font_size_to_heading_level(24.0, 0.0), 2);
    }

    // ===================================================================
    // PDFium-specific tests (only run with pdfium feature)
    // ===================================================================

    #[cfg(feature = "pdfium")]
    mod pdfium_tests {
        use super::*;

        #[test]
        fn pdfium_backend_module_exists() {
            // Verify the module is accessible
            let words: Vec<PdfWord> = Vec::new();
            let headings = pdfium_backend::detect_headings(&words);
            assert!(headings.is_empty());
        }

        #[test]
        fn pdfium_group_words_to_lines_empty() {
            let words: Vec<PdfWord> = Vec::new();
            let lines = pdfium_backend::group_words_to_lines(&words);
            assert!(lines.is_empty());
        }

        #[test]
        fn pdfium_group_lines_to_paragraphs_empty() {
            let lines: Vec<Vec<&PdfWord>> = Vec::new();
            let paras = pdfium_backend::group_lines_to_paragraphs(&lines);
            assert!(paras.is_empty());
        }
    }
}
