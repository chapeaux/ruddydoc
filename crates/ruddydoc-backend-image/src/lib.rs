//! Image backend for RuddyDoc.
//!
//! Accepts image files and creates a document with a single picture
//! element. In Phase 3 this backend extracts only image metadata
//! (dimensions, format). Phase 4 will add OCR text extraction via
//! `ruddydoc-models`.

use image::{GenericImageView, ImageFormat, ImageReader};
use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

/// Image document backend.
pub struct ImageBackend;

impl ImageBackend {
    /// Create a new image backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ImageBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a SHA-256 hash of the content bytes, returning a hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode(result)
}

/// Hex-encode bytes without pulling in a separate hex crate.
fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Map an `image::ImageFormat` to a short string name.
fn format_name(fmt: ImageFormat) -> &'static str {
    match fmt {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Tiff => "tiff",
        ImageFormat::Bmp => "bmp",
        ImageFormat::WebP => "webp",
        ImageFormat::Gif => "gif",
        _ => "unknown",
    }
}

impl DocumentBackend for ImageBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Image]
    }

    fn supports_pagination(&self) -> bool {
        false
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("png" | "jpg" | "jpeg" | "tiff" | "tif" | "bmp" | "webp")
                )
            }
            DocumentSource::Stream { name, .. } => {
                let lower = name.to_lowercase();
                lower.ends_with(".png")
                    || lower.ends_with(".jpg")
                    || lower.ends_with(".jpeg")
                    || lower.ends_with(".tiff")
                    || lower.ends_with(".tif")
                    || lower.ends_with(".bmp")
                    || lower.ends_with(".webp")
            }
        }
    }

    fn parse(
        &self,
        source: &DocumentSource,
        store: &dyn DocumentStore,
        doc_graph: &str,
    ) -> ruddydoc_core::Result<DocumentMeta> {
        // Read the image bytes and file name from the source.
        let (data, file_path, file_name) = match source {
            DocumentSource::File(path) => {
                let data = std::fs::read(path)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".to_string());
                (data, Some(path.clone()), name)
            }
            DocumentSource::Stream { name, data } => (data.clone(), None, name.clone()),
        };

        let file_size = data.len() as u64;
        let hash_str = compute_hash(&data);
        let doc_hash = DocumentHash(hash_str.clone());

        // Decode the image to extract dimensions and format.
        let cursor = std::io::Cursor::new(&data);
        let reader = ImageReader::new(cursor).with_guessed_format()?;
        let detected_format = reader.format();
        let img = reader.decode()?;
        let (width, height) = img.dimensions();

        let picture_format = detected_format.map(format_name).unwrap_or("unknown");

        // Create the document node.
        let doc_iri = ruddydoc_core::doc_iri(&hash_str);
        let rdf_type = ont::rdf_iri("type");
        let g = doc_graph;

        store.insert_triple_into(&doc_iri, &rdf_type, &ont::iri(ont::CLASS_DOCUMENT), g)?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_SOURCE_FORMAT),
            "image",
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

        // Create the single PictureElement.
        let picture_iri = ruddydoc_core::element_iri(&hash_str, "picture-0");
        store.insert_triple_into(
            &picture_iri,
            &rdf_type,
            &ont::iri(ont::CLASS_PICTURE_ELEMENT),
            g,
        )?;
        store.insert_literal(
            &picture_iri,
            &ont::iri(ont::PROP_READING_ORDER),
            "0",
            "integer",
            g,
        )?;
        store.insert_literal(
            &picture_iri,
            &ont::iri(ont::PROP_PICTURE_FORMAT),
            picture_format,
            "string",
            g,
        )?;
        store.insert_literal(
            &picture_iri,
            &ont::iri(ont::PROP_IMAGE_WIDTH),
            &width.to_string(),
            "integer",
            g,
        )?;
        store.insert_literal(
            &picture_iri,
            &ont::iri(ont::PROP_IMAGE_HEIGHT),
            &height.to_string(),
            "integer",
            g,
        )?;
        store.insert_literal(
            &picture_iri,
            &ont::iri(ont::PROP_LINK_TARGET),
            &file_name,
            "string",
            g,
        )?;

        // Link document -> picture element.
        store.insert_triple_into(&doc_iri, &ont::iri(ont::PROP_HAS_ELEMENT), &picture_iri, g)?;

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Image,
            file_size,
            page_count: None,
            language: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;
    use ruddydoc_graph::OxigraphStore;

    /// Create a synthetic image in memory and return (bytes, name).
    fn make_image(
        width: u32,
        height: u32,
        fmt: ImageFormat,
        name: &str,
    ) -> ruddydoc_core::Result<(Vec<u8>, String)> {
        let img = RgbImage::new(width, height);
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), fmt)?;
        Ok((buf, name.to_string()))
    }

    /// Helper: parse an image from a stream source.
    fn parse_image(
        data: &[u8],
        name: &str,
    ) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = ImageBackend::new();
        let source = DocumentSource::Stream {
            name: name.to_string(),
            data: data.to_vec(),
        };
        let hash_str = compute_hash(data);
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);
        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    #[test]
    fn parse_png_dimensions_and_format() -> ruddydoc_core::Result<()> {
        let (data, name) = make_image(100, 50, ImageFormat::Png, "test.png")?;
        let (store, _meta, graph) = parse_image(&data, &name)?;

        let sparql = format!(
            "SELECT ?fmt ?w ?h WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?fmt. \
                 ?p <{}> ?w. \
                 ?p <{}> ?h \
               }} \
             }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
            ont::iri(ont::PROP_PICTURE_FORMAT),
            ont::iri(ont::PROP_IMAGE_WIDTH),
            ont::iri(ont::PROP_IMAGE_HEIGHT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("png"), "expected png, got {fmt}");

        let w = rows[0]["w"].as_str().expect("w");
        assert!(w.contains("100"), "expected width 100, got {w}");

        let h = rows[0]["h"].as_str().expect("h");
        assert!(h.contains("50"), "expected height 50, got {h}");

        Ok(())
    }

    #[test]
    fn parse_jpeg_format() -> ruddydoc_core::Result<()> {
        let (data, name) = make_image(80, 60, ImageFormat::Jpeg, "photo.jpg")?;
        let (store, _meta, graph) = parse_image(&data, &name)?;

        let sparql = format!(
            "SELECT ?fmt WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?fmt \
               }} \
             }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
            ont::iri(ont::PROP_PICTURE_FORMAT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("jpeg"), "expected jpeg, got {fmt}");

        Ok(())
    }

    #[test]
    fn document_metadata_is_correct() -> ruddydoc_core::Result<()> {
        let (data, name) = make_image(10, 10, ImageFormat::Png, "meta.png")?;
        let (store, meta, graph) = parse_image(&data, &name)?;

        assert_eq!(meta.format, InputFormat::Image);
        assert!(meta.page_count.is_none());
        assert_eq!(meta.file_size, data.len() as u64);

        // Check document node properties in the store.
        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?fmt ?hash ?fname WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?fmt. \
                 <{doc_iri}> <{}> ?hash. \
                 <{doc_iri}> <{}> ?fname \
               }} \
             }}",
            ont::iri(ont::PROP_SOURCE_FORMAT),
            ont::iri(ont::PROP_DOCUMENT_HASH),
            ont::iri(ont::PROP_FILE_NAME),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("image"), "source format should be 'image'");

        let fname = rows[0]["fname"].as_str().expect("fname");
        assert!(fname.contains("meta.png"), "file name should be 'meta.png'");

        Ok(())
    }

    #[test]
    fn picture_element_properties() -> ruddydoc_core::Result<()> {
        let (data, name) = make_image(200, 150, ImageFormat::Png, "pic.png")?;
        let (store, _meta, graph) = parse_image(&data, &name)?;

        // Check that the picture element has the expected properties.
        let sparql = format!(
            "SELECT ?order ?target ?w ?h ?fmt WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?order. \
                 ?p <{}> ?target. \
                 ?p <{}> ?w. \
                 ?p <{}> ?h. \
                 ?p <{}> ?fmt \
               }} \
             }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
            ont::iri(ont::PROP_READING_ORDER),
            ont::iri(ont::PROP_LINK_TARGET),
            ont::iri(ont::PROP_IMAGE_WIDTH),
            ont::iri(ont::PROP_IMAGE_HEIGHT),
            ont::iri(ont::PROP_PICTURE_FORMAT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let order = rows[0]["order"].as_str().expect("order");
        assert!(order.contains('0'), "reading order should be 0");

        let target = rows[0]["target"].as_str().expect("target");
        assert!(
            target.contains("pic.png"),
            "link target should contain filename"
        );

        let w = rows[0]["w"].as_str().expect("w");
        assert!(w.contains("200"), "width should be 200");

        let h = rows[0]["h"].as_str().expect("h");
        assert!(h.contains("150"), "height should be 150");

        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("png"), "format should be png");

        Ok(())
    }

    #[test]
    fn has_element_link_exists() -> ruddydoc_core::Result<()> {
        let (data, name) = make_image(10, 10, ImageFormat::Png, "linked.png")?;
        let (store, _meta, graph) = parse_image(&data, &name)?;

        let sparql = format!(
            "ASK {{ \
               GRAPH <{graph}> {{ \
                 ?doc a <{}>. \
                 ?doc <{}> ?pic. \
                 ?pic a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_DOCUMENT),
            ont::iri(ont::PROP_HAS_ELEMENT),
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        Ok(())
    }

    #[test]
    fn is_valid_accepts_image_extensions() {
        let backend = ImageBackend::new();

        let valid_extensions = ["png", "jpg", "jpeg", "tiff", "tif", "bmp", "webp"];
        for ext in &valid_extensions {
            let source = DocumentSource::File(std::path::PathBuf::from(format!("image.{ext}")));
            assert!(
                backend.is_valid(&source),
                "is_valid should accept .{ext} files"
            );
        }

        // Stream-based validation
        let stream_source = DocumentSource::Stream {
            name: "photo.PNG".to_string(),
            data: vec![],
        };
        assert!(
            backend.is_valid(&stream_source),
            "is_valid should accept stream with .PNG extension"
        );
    }

    #[test]
    fn is_valid_rejects_non_image_extensions() {
        let backend = ImageBackend::new();

        let invalid_extensions = ["md", "txt", "pdf", "html", "docx"];
        for ext in &invalid_extensions {
            let source = DocumentSource::File(std::path::PathBuf::from(format!("file.{ext}")));
            assert!(
                !backend.is_valid(&source),
                "is_valid should reject .{ext} files"
            );
        }
    }

    #[test]
    fn supports_pagination_returns_false() {
        let backend = ImageBackend::new();
        assert!(!backend.supports_pagination());
    }

    #[test]
    fn supported_formats_returns_image() {
        let backend = ImageBackend::new();
        assert_eq!(backend.supported_formats(), &[InputFormat::Image]);
    }

    #[test]
    fn error_on_invalid_image_data() {
        let backend = ImageBackend::new();
        let source = DocumentSource::Stream {
            name: "bad.png".to_string(),
            data: b"this is not an image".to_vec(),
        };
        let store = OxigraphStore::new().expect("store creation");
        let result = backend.parse(&source, &store, "urn:test:graph");
        assert!(
            result.is_err(),
            "parsing invalid data should produce an error"
        );
    }

    #[test]
    fn bmp_format_detected() -> ruddydoc_core::Result<()> {
        let (data, name) = make_image(32, 32, ImageFormat::Bmp, "icon.bmp")?;
        let (store, _meta, graph) = parse_image(&data, &name)?;

        let sparql = format!(
            "SELECT ?fmt WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?fmt \
               }} \
             }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
            ont::iri(ont::PROP_PICTURE_FORMAT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("bmp"), "expected bmp, got {fmt}");

        Ok(())
    }

    #[test]
    fn default_trait() {
        let _backend = ImageBackend::default();
    }

    #[test]
    fn format_name_helper() {
        assert_eq!(format_name(ImageFormat::Png), "png");
        assert_eq!(format_name(ImageFormat::Jpeg), "jpeg");
        assert_eq!(format_name(ImageFormat::Tiff), "tiff");
        assert_eq!(format_name(ImageFormat::Bmp), "bmp");
        assert_eq!(format_name(ImageFormat::WebP), "webp");
        assert_eq!(format_name(ImageFormat::Gif), "gif");
    }
}
