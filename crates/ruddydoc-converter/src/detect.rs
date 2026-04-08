//! Multi-layered format detection for RuddyDoc.
//!
//! Detection strategy (in order of precedence):
//! 1. Magic bytes (binary formats: PDF, ZIP/OOXML, images)
//! 2. ZIP inspection for OOXML sub-types (DOCX, XLSX, PPTX)
//! 3. Content sniffing for text formats (HTML, XML, WebVTT, LaTeX, CSV)
//! 4. File extension fallback (from `ruddydoc-core`)

use ruddydoc_core::InputFormat;

// ---------------------------------------------------------------------------
// Magic byte signatures
// ---------------------------------------------------------------------------

/// Check if data starts with the PDF magic bytes `%PDF`.
fn is_pdf(data: &[u8]) -> bool {
    data.starts_with(b"%PDF")
}

/// Check if data starts with the ZIP magic bytes `PK\x03\x04`.
fn is_zip(data: &[u8]) -> bool {
    data.len() >= 4 && data[0] == b'P' && data[1] == b'K' && data[2] == 0x03 && data[3] == 0x04
}

/// Check if data starts with the PNG magic bytes `\x89PNG`.
fn is_png(data: &[u8]) -> bool {
    data.len() >= 4 && data[0] == 0x89 && data[1] == b'P' && data[2] == b'N' && data[3] == b'G'
}

/// Check if data starts with JPEG magic bytes `\xFF\xD8\xFF`.
fn is_jpeg(data: &[u8]) -> bool {
    data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF
}

/// Check if data starts with TIFF magic bytes (`II\x2A\x00` or `MM\x00\x2A`).
fn is_tiff(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    // Little-endian TIFF
    let le = data[0] == b'I' && data[1] == b'I' && data[2] == 0x2A && data[3] == 0x00;
    // Big-endian TIFF
    let be = data[0] == b'M' && data[1] == b'M' && data[2] == 0x00 && data[3] == 0x2A;
    le || be
}

/// Check if data starts with GIF magic bytes `GIF8`.
fn is_gif(data: &[u8]) -> bool {
    data.starts_with(b"GIF8")
}

/// Check if data starts with WebP magic bytes (`RIFF....WEBP`).
fn is_webp(data: &[u8]) -> bool {
    data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP"
}

// ---------------------------------------------------------------------------
// Magic byte detection (binary formats)
// ---------------------------------------------------------------------------

/// Detect format from magic bytes alone (no extension considered).
///
/// Returns `None` if no magic byte signature matches.
pub fn detect_format_from_bytes(data: &[u8]) -> Option<InputFormat> {
    if data.is_empty() {
        return None;
    }

    // Binary formats first (most specific signatures)
    if is_pdf(data) {
        return Some(InputFormat::Pdf);
    }
    if is_png(data) {
        return Some(InputFormat::Image);
    }
    if is_jpeg(data) {
        return Some(InputFormat::Image);
    }
    if is_tiff(data) {
        return Some(InputFormat::Image);
    }
    if is_gif(data) {
        return Some(InputFormat::Image);
    }
    if is_webp(data) {
        return Some(InputFormat::Image);
    }
    if is_zip(data) {
        // ZIP could be DOCX, XLSX, PPTX, EPUB, or generic ZIP
        return detect_ooxml_from_bytes(data);
    }

    // Text-based content sniffing
    detect_from_text_content(data)
}

/// Full format detection combining extension hint with byte-level analysis.
///
/// If `ext_hint` is provided (from file extension), it is used as a fallback
/// when byte-level detection does not produce a result. When byte-level
/// detection does produce a result, it takes priority (since magic bytes
/// are more reliable than extensions).
pub fn detect_format_full(ext_hint: Option<InputFormat>, data: &[u8]) -> Option<InputFormat> {
    // Try magic bytes / content sniffing first
    if let Some(fmt) = detect_format_from_bytes(data) {
        return Some(fmt);
    }

    // Fall back to extension hint
    ext_hint
}

// ---------------------------------------------------------------------------
// ZIP / OOXML inspection
// ---------------------------------------------------------------------------

/// Inspect a ZIP archive to determine if it is a specific OOXML format.
///
/// Checks for `[Content_Types].xml` and then looks for format-specific
/// paths inside the archive.
fn detect_ooxml_from_bytes(data: &[u8]) -> Option<InputFormat> {
    let cursor = std::io::Cursor::new(data);
    let archive = zip::ZipArchive::new(cursor).ok()?;

    // Check for OOXML marker
    let has_content_types = archive.file_names().any(|n| n == "[Content_Types].xml");

    if has_content_types {
        // DOCX
        if archive.file_names().any(|n| n == "word/document.xml") {
            return Some(InputFormat::Docx);
        }
        // XLSX
        if archive.file_names().any(|n| n == "xl/workbook.xml") {
            return Some(InputFormat::Xlsx);
        }
        // PPTX
        if archive.file_names().any(|n| n == "ppt/presentation.xml") {
            return Some(InputFormat::Pptx);
        }
    }

    // Check for EPUB (META-INF/container.xml)
    if archive.file_names().any(|n| n == "META-INF/container.xml") {
        return Some(InputFormat::Epub);
    }

    // Generic ZIP -- not a recognized document format
    None
}

// ---------------------------------------------------------------------------
// Text-based content sniffing
// ---------------------------------------------------------------------------

/// Detect format from text content analysis.
///
/// Attempts to identify text-based formats by looking for characteristic
/// patterns in the first few KB of content.
fn detect_from_text_content(data: &[u8]) -> Option<InputFormat> {
    // Must be valid-ish text (allow UTF-8 with some tolerance)
    let text = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let trimmed = text.trim_start();

    // WebVTT: must start with "WEBVTT"
    if trimmed.starts_with("WEBVTT") {
        return Some(InputFormat::WebVtt);
    }

    // HTML: check for DOCTYPE or <html tag (case-insensitive)
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        return Some(InputFormat::Html);
    }

    // XML: starts with <?xml or a root element
    if trimmed.starts_with("<?xml") {
        // Could be XHTML, XBRL, generic XML
        if lower.contains("<html") {
            return Some(InputFormat::Html);
        }
        if lower.contains("xbrl") {
            return Some(InputFormat::Xbrl);
        }
        return Some(InputFormat::Xml);
    }

    // LaTeX: characteristic commands
    if trimmed.contains("\\documentclass") || trimmed.contains("\\begin{document}") {
        return Some(InputFormat::Latex);
    }

    // CSV heuristic: check for consistent delimiter counts in lines
    if looks_like_csv(trimmed) {
        return Some(InputFormat::Csv);
    }

    None
}

/// Heuristic to detect CSV/TSV content.
///
/// Checks whether the first several lines have a consistent number of
/// delimiters (comma or tab).
fn looks_like_csv(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().take(10).collect();
    if lines.len() < 2 {
        return false;
    }

    // Try comma first
    if consistent_delimiter_count(&lines, ',') {
        return true;
    }

    // Try tab
    if consistent_delimiter_count(&lines, '\t') {
        return true;
    }

    false
}

/// Check that all non-empty lines have the same number of a given delimiter,
/// and that count is at least 1.
fn consistent_delimiter_count(lines: &[&str], delim: char) -> bool {
    let counts: Vec<usize> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().filter(|c| *c == delim).count())
        .collect();

    if counts.is_empty() {
        return false;
    }

    let first = counts[0];
    if first == 0 {
        return false;
    }

    counts.iter().all(|&c| c == first)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // -- Magic byte detection --

    #[test]
    fn detect_pdf_magic() {
        let data = b"%PDF-1.4 some pdf content here...";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Pdf));
    }

    #[test]
    fn detect_png_magic() {
        let mut data = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        data.extend_from_slice(&[0; 20]); // padding
        assert_eq!(detect_format_from_bytes(&data), Some(InputFormat::Image));
    }

    #[test]
    fn detect_jpeg_magic() {
        let data = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(detect_format_from_bytes(&data), Some(InputFormat::Image));
    }

    #[test]
    fn detect_tiff_le_magic() {
        let data = [b'I', b'I', 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        assert_eq!(detect_format_from_bytes(&data), Some(InputFormat::Image));
    }

    #[test]
    fn detect_tiff_be_magic() {
        let data = [b'M', b'M', 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08];
        assert_eq!(detect_format_from_bytes(&data), Some(InputFormat::Image));
    }

    #[test]
    fn detect_gif_magic() {
        let data = b"GIF89a some gif data";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Image));
    }

    #[test]
    fn detect_webp_magic() {
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // file size placeholder
        data.extend_from_slice(b"WEBP");
        data.extend_from_slice(&[0; 20]); // padding
        assert_eq!(detect_format_from_bytes(&data), Some(InputFormat::Image));
    }

    // -- Content sniffing --

    #[test]
    fn detect_html_doctype() {
        let data = b"<!DOCTYPE html><html><body>hi</body></html>";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Html));
    }

    #[test]
    fn detect_html_tag() {
        let data = b"<html lang=\"en\"><head></head><body></body></html>";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Html));
    }

    #[test]
    fn detect_html_case_insensitive() {
        let data = b"<!DOCTYPE HTML>\n<HTML><BODY>test</BODY></HTML>";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Html));
    }

    #[test]
    fn detect_xml_processing_instruction() {
        let data = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root><item/></root>";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Xml));
    }

    #[test]
    fn detect_xml_with_html() {
        let data = b"<?xml version=\"1.0\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\">";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Html));
    }

    #[test]
    fn detect_webvtt_content() {
        let data = b"WEBVTT\n\n00:00:01.000 --> 00:00:04.000\nHello world";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::WebVtt));
    }

    #[test]
    fn detect_latex_documentclass() {
        let data = b"\\documentclass{article}\n\\begin{document}\nHello\n\\end{document}";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Latex));
    }

    #[test]
    fn detect_latex_begin_document() {
        let data = b"% preamble stuff\n\\begin{document}\nSome text\n\\end{document}";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Latex));
    }

    #[test]
    fn detect_csv_comma() {
        let data = b"name,age,city\nAlice,30,NYC\nBob,25,LA\n";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Csv));
    }

    #[test]
    fn detect_csv_tab() {
        let data = b"name\tage\tcity\nAlice\t30\tNYC\nBob\t25\tLA\n";
        assert_eq!(detect_format_from_bytes(data), Some(InputFormat::Csv));
    }

    #[test]
    fn csv_heuristic_rejects_single_line() {
        // A single line should not be detected as CSV
        let data = b"name,age,city\n";
        // Only 1 real data line, needs at least 2
        assert_ne!(detect_format_from_bytes(data), Some(InputFormat::Csv));
    }

    #[test]
    fn csv_heuristic_rejects_inconsistent() {
        let data = b"name,age\nAlice\nBob,25\n";
        // Inconsistent comma counts: 1, 0, 1
        assert_ne!(detect_format_from_bytes(data), Some(InputFormat::Csv));
    }

    // -- Empty / unrecognized --

    #[test]
    fn detect_empty_bytes() {
        assert_eq!(detect_format_from_bytes(b""), None);
    }

    #[test]
    fn detect_random_bytes() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05];
        assert_eq!(detect_format_from_bytes(&data), None);
    }

    #[test]
    fn detect_plain_text_not_matched() {
        // Plain text without any format markers should return None
        let data = b"Just some random text without any structure.";
        assert_eq!(detect_format_from_bytes(data), None);
    }

    // -- Full detection with extension hint --

    #[test]
    fn full_detection_magic_overrides_extension() {
        // Data has PDF magic but extension hint says Markdown
        let data = b"%PDF-1.7 content";
        let result = detect_format_full(Some(InputFormat::Markdown), data);
        assert_eq!(result, Some(InputFormat::Pdf));
    }

    #[test]
    fn full_detection_falls_back_to_extension() {
        // Data is plain text; extension hint says Markdown
        let data = b"# Hello World";
        let result = detect_format_full(Some(InputFormat::Markdown), data);
        assert_eq!(result, Some(InputFormat::Markdown));
    }

    #[test]
    fn full_detection_no_hint_no_match() {
        let data = b"just random text";
        let result = detect_format_full(None, data);
        assert_eq!(result, None);
    }

    // -- ZIP / OOXML detection --

    #[test]
    fn detect_docx_zip() {
        let buf = build_ooxml_zip(&["[Content_Types].xml", "word/document.xml"]);
        assert_eq!(detect_format_from_bytes(&buf), Some(InputFormat::Docx));
    }

    #[test]
    fn detect_xlsx_zip() {
        let buf = build_ooxml_zip(&["[Content_Types].xml", "xl/workbook.xml"]);
        assert_eq!(detect_format_from_bytes(&buf), Some(InputFormat::Xlsx));
    }

    #[test]
    fn detect_pptx_zip() {
        let buf = build_ooxml_zip(&["[Content_Types].xml", "ppt/presentation.xml"]);
        assert_eq!(detect_format_from_bytes(&buf), Some(InputFormat::Pptx));
    }

    #[test]
    fn detect_epub_zip() {
        let buf = build_ooxml_zip(&["META-INF/container.xml", "mimetype"]);
        assert_eq!(detect_format_from_bytes(&buf), Some(InputFormat::Epub));
    }

    #[test]
    fn detect_generic_zip_returns_none() {
        let buf = build_ooxml_zip(&["file.txt", "another.txt"]);
        assert_eq!(detect_format_from_bytes(&buf), None);
    }

    // -- Helper: build a minimal ZIP in memory --

    fn build_ooxml_zip(file_names: &[&str]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut writer = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for name in file_names {
                writer.start_file(name.to_string(), options).unwrap();
                writer.write_all(b"dummy").unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    // -- Internal function tests --

    #[test]
    fn consistent_delimiter_all_same() {
        let lines = vec!["a,b,c", "1,2,3", "4,5,6"];
        assert!(consistent_delimiter_count(&lines, ','));
    }

    #[test]
    fn consistent_delimiter_different_counts() {
        let lines = vec!["a,b,c", "1,2", "4,5,6"];
        assert!(!consistent_delimiter_count(&lines, ','));
    }

    #[test]
    fn consistent_delimiter_zero_delimiters() {
        let lines = vec!["abc", "def", "ghi"];
        assert!(!consistent_delimiter_count(&lines, ','));
    }

    #[test]
    fn consistent_delimiter_empty_input() {
        let lines: Vec<&str> = Vec::new();
        assert!(!consistent_delimiter_count(&lines, ','));
    }
}
