//! Backend registry for RuddyDoc.
//!
//! Maintains a list of all available format backends and provides
//! lookup by `InputFormat`.

use ruddydoc_core::{DocumentBackend, InputFormat};

/// Registry of all available document backends.
///
/// Backends are registered at construction time and looked up by format
/// during conversion.
pub struct BackendRegistry {
    backends: Vec<Box<dyn DocumentBackend>>,
}

impl BackendRegistry {
    /// Create a new registry pre-populated with all available backends.
    pub fn new() -> Self {
        let backends: Vec<Box<dyn DocumentBackend>> = vec![
            Box::new(ruddydoc_backend_md::MarkdownBackend::new()),
            Box::new(ruddydoc_backend_html::HtmlBackend),
            Box::new(ruddydoc_backend_csv::CsvBackend),
            Box::new(ruddydoc_backend_latex::LatexBackend),
            Box::new(ruddydoc_backend_webvtt::WebVttBackend),
            Box::new(ruddydoc_backend_asciidoc::AsciiDocBackend),
            Box::new(ruddydoc_backend_xml::XmlBackend),
            Box::new(ruddydoc_backend_pdf::PdfBackend),
            Box::new(ruddydoc_backend_docx::DocxBackend),
            Box::new(ruddydoc_backend_xlsx::XlsxBackend),
            Box::new(ruddydoc_backend_pptx::PptxBackend),
            Box::new(ruddydoc_backend_image::ImageBackend),
        ];

        Self { backends }
    }

    /// Find the backend that supports the given format.
    ///
    /// Returns the first registered backend whose `supported_formats()`
    /// includes the requested format.
    pub fn backend_for(&self, format: InputFormat) -> Option<&dyn DocumentBackend> {
        self.backends
            .iter()
            .find(|b| b.supported_formats().contains(&format))
            .map(|b| b.as_ref())
    }

    /// Return all registered backends.
    pub fn all_backends(&self) -> &[Box<dyn DocumentBackend>] {
        &self.backends
    }

    /// Return all formats that have a registered backend.
    pub fn supported_formats(&self) -> Vec<InputFormat> {
        let mut formats = Vec::new();
        for backend in &self.backends {
            for fmt in backend.supported_formats() {
                if !formats.contains(fmt) {
                    formats.push(*fmt);
                }
            }
        }
        formats
    }
}

impl Default for BackendRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_markdown_backend() {
        let registry = BackendRegistry::new();
        let backend = registry.backend_for(InputFormat::Markdown);
        assert!(backend.is_some());
        assert!(
            backend
                .unwrap()
                .supported_formats()
                .contains(&InputFormat::Markdown)
        );
    }

    #[test]
    fn registry_has_html_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::Html).is_some());
    }

    #[test]
    fn registry_has_csv_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::Csv).is_some());
    }

    #[test]
    fn registry_has_latex_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::Latex).is_some());
    }

    #[test]
    fn registry_has_webvtt_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::WebVtt).is_some());
    }

    #[test]
    fn registry_has_asciidoc_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::AsciiDoc).is_some());
    }

    #[test]
    fn registry_has_xml_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::Xml).is_some());
    }

    #[test]
    fn registry_has_pdf_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::Pdf).is_some());
    }

    #[test]
    fn registry_has_docx_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::Docx).is_some());
    }

    #[test]
    fn registry_has_xlsx_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::Xlsx).is_some());
    }

    #[test]
    fn registry_has_pptx_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::Pptx).is_some());
    }

    #[test]
    fn registry_has_image_backend() {
        let registry = BackendRegistry::new();
        assert!(registry.backend_for(InputFormat::Image).is_some());
    }

    #[test]
    fn registry_returns_none_for_unregistered() {
        let registry = BackendRegistry::new();
        // Json and Text don't have backends
        assert!(registry.backend_for(InputFormat::Json).is_none());
        assert!(registry.backend_for(InputFormat::Text).is_none());
    }

    #[test]
    fn registry_supported_formats_list() {
        let registry = BackendRegistry::new();
        let formats = registry.supported_formats();
        assert!(formats.contains(&InputFormat::Markdown));
        assert!(formats.contains(&InputFormat::Html));
        assert!(formats.contains(&InputFormat::Csv));
        assert!(formats.contains(&InputFormat::Pdf));
        assert!(formats.len() >= 12);
    }

    #[test]
    fn registry_all_backends_nonempty() {
        let registry = BackendRegistry::new();
        assert!(!registry.all_backends().is_empty());
        assert_eq!(registry.all_backends().len(), 12);
    }

    #[test]
    fn markdown_backend_is_valid() {
        let registry = BackendRegistry::new();
        let backend = registry.backend_for(InputFormat::Markdown).unwrap();
        let source = ruddydoc_core::DocumentSource::File(std::path::PathBuf::from("test.md"));
        assert!(backend.is_valid(&source));
    }

    #[test]
    fn markdown_backend_invalid_extension() {
        let registry = BackendRegistry::new();
        let backend = registry.backend_for(InputFormat::Markdown).unwrap();
        let source = ruddydoc_core::DocumentSource::File(std::path::PathBuf::from("test.pdf"));
        assert!(!backend.is_valid(&source));
    }
}
