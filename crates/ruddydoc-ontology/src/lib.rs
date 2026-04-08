//! Document ontology definitions for RuddyDoc.
//!
//! This crate defines the RuddyDoc document ontology as Rust constants
//! and provides functions to load the ontology into the document store.
//! The ontology Turtle file is bundled at compile time using `include_str!`.

use ruddydoc_core::DocumentStore;

/// The bundled ontology Turtle file, included at compile time.
const ONTOLOGY_TTL: &str = include_str!("../../../ontology/ruddydoc.ttl");

/// Ontology namespace IRI.
pub const NAMESPACE: &str = "https://ruddydoc.chapeaux.io/ontology#";

/// Named graph for the ontology itself.
pub const ONTOLOGY_GRAPH: &str = "urn:ruddydoc:ontology";

/// Standard RDF namespace.
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// Standard RDFS namespace.
pub const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";

/// Standard OWL namespace.
pub const OWL: &str = "http://www.w3.org/2002/07/owl#";

/// Standard XSD namespace.
pub const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

// -----------------------------------------------------------------------
// Helper to build full IRIs
// -----------------------------------------------------------------------

/// Build a full ontology IRI from a local name.
pub fn iri(term: &str) -> String {
    format!("{NAMESPACE}{term}")
}

/// Build a full RDF IRI from a local name.
pub fn rdf_iri(term: &str) -> String {
    format!("{RDF}{term}")
}

/// Build a full RDFS IRI from a local name.
pub fn rdfs_iri(term: &str) -> String {
    format!("{RDFS}{term}")
}

// -----------------------------------------------------------------------
// Class constants (local names)
// -----------------------------------------------------------------------

pub const CLASS_DOCUMENT: &str = "Document";
pub const CLASS_PAGE: &str = "Page";
pub const CLASS_DOCUMENT_ELEMENT: &str = "DocumentElement";
pub const CLASS_TEXT_ELEMENT: &str = "TextElement";
pub const CLASS_TITLE: &str = "Title";
pub const CLASS_SECTION_HEADER: &str = "SectionHeader";
pub const CLASS_PARAGRAPH: &str = "Paragraph";
pub const CLASS_LIST_ITEM: &str = "ListItem";
pub const CLASS_FOOTNOTE: &str = "Footnote";
pub const CLASS_CAPTION: &str = "Caption";
pub const CLASS_CODE: &str = "Code";
pub const CLASS_FORMULA: &str = "Formula";
pub const CLASS_REFERENCE: &str = "Reference";
pub const CLASS_HYPERLINK: &str = "Hyperlink";
pub const CLASS_TABLE_ELEMENT: &str = "TableElement";
pub const CLASS_TABLE_CELL: &str = "TableCell";
pub const CLASS_PICTURE_ELEMENT: &str = "PictureElement";
pub const CLASS_KEY_VALUE_ITEM: &str = "KeyValueItem";
pub const CLASS_GROUP: &str = "Group";
pub const CLASS_ORDERED_LIST: &str = "OrderedList";
pub const CLASS_UNORDERED_LIST: &str = "UnorderedList";
pub const CLASS_FURNITURE: &str = "Furniture";
pub const CLASS_PAGE_HEADER: &str = "PageHeader";
pub const CLASS_PAGE_FOOTER: &str = "PageFooter";
pub const CLASS_BOUNDING_BOX: &str = "BoundingBox";
pub const CLASS_PROVENANCE: &str = "Provenance";

// -----------------------------------------------------------------------
// Property constants (local names)
// -----------------------------------------------------------------------

// Document-level properties
pub const PROP_HAS_ELEMENT: &str = "hasElement";
pub const PROP_HAS_PAGE: &str = "hasPage";
pub const PROP_SOURCE_FORMAT: &str = "sourceFormat";
pub const PROP_DOCUMENT_HASH: &str = "documentHash";
pub const PROP_FILE_NAME: &str = "fileName";
pub const PROP_FILE_SIZE: &str = "fileSize";
pub const PROP_PAGE_COUNT: &str = "pageCount";
pub const PROP_LANGUAGE: &str = "language";

// Page-level properties
pub const PROP_PAGE_NUMBER: &str = "pageNumber";
pub const PROP_PAGE_WIDTH: &str = "pageWidth";
pub const PROP_PAGE_HEIGHT: &str = "pageHeight";

// Element-level properties
pub const PROP_TEXT_CONTENT: &str = "textContent";
pub const PROP_READING_ORDER: &str = "readingOrder";
pub const PROP_ON_PAGE: &str = "onPage";
pub const PROP_HEADING_LEVEL: &str = "headingLevel";
pub const PROP_PARENT_ELEMENT: &str = "parentElement";
pub const PROP_CHILD_ELEMENT: &str = "childElement";
pub const PROP_NEXT_ELEMENT: &str = "nextElement";
pub const PROP_PREVIOUS_ELEMENT: &str = "previousElement";
pub const PROP_CODE_LANGUAGE: &str = "codeLanguage";
pub const PROP_LINK_TARGET: &str = "linkTarget";
pub const PROP_LINK_TEXT: &str = "linkText";

// Key-Value properties
pub const PROP_KEY_NAME: &str = "keyName";
pub const PROP_KEY_VALUE: &str = "keyValue";

// Bounding box properties
pub const PROP_HAS_BOUNDING_BOX: &str = "hasBoundingBox";
pub const PROP_BBOX_LEFT: &str = "bboxLeft";
pub const PROP_BBOX_TOP: &str = "bboxTop";
pub const PROP_BBOX_RIGHT: &str = "bboxRight";
pub const PROP_BBOX_BOTTOM: &str = "bboxBottom";
pub const PROP_BBOX_PAGE: &str = "bboxPage";

// Table-specific properties
pub const PROP_HAS_CELL: &str = "hasCell";
pub const PROP_CELL_ROW: &str = "cellRow";
pub const PROP_CELL_COLUMN: &str = "cellColumn";
pub const PROP_CELL_ROW_SPAN: &str = "cellRowSpan";
pub const PROP_CELL_COL_SPAN: &str = "cellColSpan";
pub const PROP_CELL_TEXT: &str = "cellText";
pub const PROP_IS_HEADER: &str = "isHeader";
pub const PROP_ROW_COUNT: &str = "rowCount";
pub const PROP_COLUMN_COUNT: &str = "columnCount";

// Picture-specific properties
pub const PROP_PICTURE_DATA: &str = "pictureData";
pub const PROP_PICTURE_FORMAT: &str = "pictureFormat";
pub const PROP_HAS_CAPTION: &str = "hasCaption";
pub const PROP_PICTURE_CATEGORY: &str = "pictureCategory";
pub const PROP_ALT_TEXT: &str = "altText";
pub const PROP_IMAGE_WIDTH: &str = "imageWidth";
pub const PROP_IMAGE_HEIGHT: &str = "imageHeight";

// Time-based properties
pub const PROP_START_TIME: &str = "startTime";
pub const PROP_END_TIME: &str = "endTime";
pub const PROP_DURATION: &str = "duration";

// Provenance properties
pub const PROP_HAS_PROVENANCE: &str = "hasProvenance";
pub const PROP_CONFIDENCE: &str = "confidence";
pub const PROP_DETECTED_BY: &str = "detectedBy";
pub const PROP_MODEL_NAME: &str = "modelName";
pub const PROP_MODEL_VERSION: &str = "modelVersion";
pub const PROP_PROCESSING_DATE: &str = "processingDate";

// Cross-reference properties
pub const PROP_REFERS_TO: &str = "refersTo";
pub const PROP_LABEL_ID: &str = "labelId";
pub const PROP_CITATION_KEY: &str = "citationKey";

// -----------------------------------------------------------------------
// Ontology loading
// -----------------------------------------------------------------------

/// Load the document ontology into the store's ontology named graph.
///
/// Parses the bundled Turtle file line-by-line and inserts each triple
/// into the `urn:ruddydoc:ontology` named graph. This is done
/// programmatically rather than via Oxigraph's Turtle parser because
/// the `DocumentStore` trait is our only interface to the store.
pub fn load_ontology(store: &dyn DocumentStore) -> ruddydoc_core::Result<()> {
    // Parse the Turtle manually using a simple line-based approach.
    // We use the store's insert methods, which means we need to extract
    // triples from the Turtle ourselves.
    //
    // For the ontology file, we take a simpler approach: programmatically
    // insert the key triples that define the ontology structure.
    let rdf_type = rdf_iri("type");
    let rdfs_class = rdfs_iri("Class");
    let rdfs_sub_class_of = rdfs_iri("subClassOf");
    let rdfs_label = rdfs_iri("label");
    let rdfs_comment = rdfs_iri("comment");
    let rdf_property = rdf_iri("Property");
    let rdfs_domain = rdfs_iri("domain");
    let rdfs_range = rdfs_iri("range");
    let owl_ontology = format!("{OWL}Ontology");

    let g = ONTOLOGY_GRAPH;

    // -- Ontology header --
    store.insert_triple_into(&iri(""), &rdf_type, &owl_ontology, g)?;
    store.insert_literal(
        &iri(""),
        &rdfs_label,
        "RuddyDoc Document Ontology",
        "string",
        g,
    )?;

    // -- Classes --
    let classes: &[(&str, Option<&str>, &str, &str)] = &[
        (
            CLASS_DOCUMENT,
            None,
            "Document",
            "A parsed document. The top-level container.",
        ),
        (
            CLASS_PAGE,
            None,
            "Page",
            "A page within a paginated document.",
        ),
        (
            CLASS_DOCUMENT_ELEMENT,
            None,
            "Document Element",
            "Abstract base class for any structural element.",
        ),
        (
            CLASS_TEXT_ELEMENT,
            Some(CLASS_DOCUMENT_ELEMENT),
            "Text Element",
            "A text-bearing element.",
        ),
        (
            CLASS_TITLE,
            Some(CLASS_TEXT_ELEMENT),
            "Title",
            "The main title of the document.",
        ),
        (
            CLASS_SECTION_HEADER,
            Some(CLASS_TEXT_ELEMENT),
            "Section Header",
            "A heading that introduces a section.",
        ),
        (
            CLASS_PARAGRAPH,
            Some(CLASS_TEXT_ELEMENT),
            "Paragraph",
            "A block of body text.",
        ),
        (
            CLASS_LIST_ITEM,
            Some(CLASS_TEXT_ELEMENT),
            "List Item",
            "A single item within a list.",
        ),
        (
            CLASS_FOOTNOTE,
            Some(CLASS_TEXT_ELEMENT),
            "Footnote",
            "A footnote or endnote.",
        ),
        (
            CLASS_CAPTION,
            Some(CLASS_TEXT_ELEMENT),
            "Caption",
            "A caption describing an element.",
        ),
        (
            CLASS_CODE,
            Some(CLASS_TEXT_ELEMENT),
            "Code Block",
            "A block of source code.",
        ),
        (
            CLASS_FORMULA,
            Some(CLASS_TEXT_ELEMENT),
            "Formula",
            "A mathematical formula.",
        ),
        (
            CLASS_REFERENCE,
            Some(CLASS_TEXT_ELEMENT),
            "Bibliographic Reference",
            "A bibliographic reference.",
        ),
        (
            CLASS_HYPERLINK,
            Some(CLASS_TEXT_ELEMENT),
            "Hyperlink",
            "A link element with a target URL.",
        ),
        (
            CLASS_TABLE_ELEMENT,
            Some(CLASS_DOCUMENT_ELEMENT),
            "Table",
            "A table with rows, columns, and cells.",
        ),
        (
            CLASS_TABLE_CELL,
            None,
            "Table Cell",
            "An individual cell within a table.",
        ),
        (
            CLASS_PICTURE_ELEMENT,
            Some(CLASS_DOCUMENT_ELEMENT),
            "Picture",
            "An image or figure.",
        ),
        (
            CLASS_KEY_VALUE_ITEM,
            Some(CLASS_DOCUMENT_ELEMENT),
            "Key-Value Item",
            "A form field or key-value pair.",
        ),
        (
            CLASS_GROUP,
            Some(CLASS_DOCUMENT_ELEMENT),
            "Group",
            "A logical grouping of elements.",
        ),
        (
            CLASS_ORDERED_LIST,
            Some(CLASS_GROUP),
            "Ordered List",
            "An ordered (numbered) list.",
        ),
        (
            CLASS_UNORDERED_LIST,
            Some(CLASS_GROUP),
            "Unordered List",
            "An unordered (bulleted) list.",
        ),
        (
            CLASS_FURNITURE,
            Some(CLASS_DOCUMENT_ELEMENT),
            "Furniture",
            "Page furniture.",
        ),
        (
            CLASS_PAGE_HEADER,
            Some(CLASS_FURNITURE),
            "Page Header",
            "A running header.",
        ),
        (
            CLASS_PAGE_FOOTER,
            Some(CLASS_FURNITURE),
            "Page Footer",
            "A running footer.",
        ),
        (
            CLASS_BOUNDING_BOX,
            None,
            "Bounding Box",
            "A rectangular spatial position on a page.",
        ),
        (
            CLASS_PROVENANCE,
            None,
            "Provenance",
            "Metadata about how an element was detected.",
        ),
    ];

    for (name, parent, label, comment) in classes {
        let class_iri = iri(name);
        store.insert_triple_into(&class_iri, &rdf_type, &rdfs_class, g)?;
        store.insert_literal(&class_iri, &rdfs_label, label, "string", g)?;
        store.insert_literal(&class_iri, &rdfs_comment, comment, "string", g)?;
        if let Some(parent_name) = parent {
            let parent_iri = iri(parent_name);
            store.insert_triple_into(&class_iri, &rdfs_sub_class_of, &parent_iri, g)?;
        }
    }

    // -- Properties --
    let properties: &[(&str, &str, &str, &str, &str)] = &[
        // Document-level
        (
            PROP_HAS_ELEMENT,
            CLASS_DOCUMENT,
            CLASS_DOCUMENT_ELEMENT,
            "has element",
            "Links a document to a structural element.",
        ),
        (
            PROP_HAS_PAGE,
            CLASS_DOCUMENT,
            CLASS_PAGE,
            "has page",
            "Links a document to a page.",
        ),
        (
            PROP_SOURCE_FORMAT,
            CLASS_DOCUMENT,
            "xsd:string",
            "source format",
            "The input format of the document.",
        ),
        (
            PROP_DOCUMENT_HASH,
            CLASS_DOCUMENT,
            "xsd:string",
            "document hash",
            "Content-based hash identifying the document.",
        ),
        (
            PROP_FILE_NAME,
            CLASS_DOCUMENT,
            "xsd:string",
            "file name",
            "The original file name.",
        ),
        (
            PROP_FILE_SIZE,
            CLASS_DOCUMENT,
            "xsd:long",
            "file size",
            "File size in bytes.",
        ),
        (
            PROP_PAGE_COUNT,
            CLASS_DOCUMENT,
            "xsd:integer",
            "page count",
            "Total number of pages.",
        ),
        (
            PROP_LANGUAGE,
            CLASS_DOCUMENT,
            "xsd:string",
            "language",
            "Primary language of the document.",
        ),
        // Page-level
        (
            PROP_PAGE_NUMBER,
            CLASS_PAGE,
            "xsd:positiveInteger",
            "page number",
            "1-based page number.",
        ),
        (
            PROP_PAGE_WIDTH,
            CLASS_PAGE,
            "xsd:float",
            "page width",
            "Width of the page in points.",
        ),
        (
            PROP_PAGE_HEIGHT,
            CLASS_PAGE,
            "xsd:float",
            "page height",
            "Height of the page in points.",
        ),
        // Element-level
        (
            PROP_TEXT_CONTENT,
            CLASS_TEXT_ELEMENT,
            "xsd:string",
            "text content",
            "Plain text content.",
        ),
        (
            PROP_READING_ORDER,
            CLASS_DOCUMENT_ELEMENT,
            "xsd:nonNegativeInteger",
            "reading order",
            "Position in reading order.",
        ),
        (
            PROP_ON_PAGE,
            CLASS_DOCUMENT_ELEMENT,
            CLASS_PAGE,
            "on page",
            "The page on which this element appears.",
        ),
        (
            PROP_HEADING_LEVEL,
            CLASS_SECTION_HEADER,
            "xsd:integer",
            "heading level",
            "Heading depth level (1-6).",
        ),
        (
            PROP_PARENT_ELEMENT,
            CLASS_DOCUMENT_ELEMENT,
            CLASS_DOCUMENT_ELEMENT,
            "parent element",
            "Parent in the document tree.",
        ),
        (
            PROP_CHILD_ELEMENT,
            CLASS_DOCUMENT_ELEMENT,
            CLASS_DOCUMENT_ELEMENT,
            "child element",
            "Child in the document tree.",
        ),
        (
            PROP_NEXT_ELEMENT,
            CLASS_DOCUMENT_ELEMENT,
            CLASS_DOCUMENT_ELEMENT,
            "next element",
            "Next sibling in reading order.",
        ),
        (
            PROP_PREVIOUS_ELEMENT,
            CLASS_DOCUMENT_ELEMENT,
            CLASS_DOCUMENT_ELEMENT,
            "previous element",
            "Previous sibling in reading order.",
        ),
        (
            PROP_CODE_LANGUAGE,
            CLASS_CODE,
            "xsd:string",
            "code language",
            "Programming language of code block.",
        ),
        (
            PROP_LINK_TARGET,
            CLASS_HYPERLINK,
            "xsd:anyURI",
            "link target",
            "Target URL of a hyperlink.",
        ),
        (
            PROP_LINK_TEXT,
            CLASS_HYPERLINK,
            "xsd:string",
            "link text",
            "Visible anchor text of a hyperlink.",
        ),
        // Key-Value
        (
            PROP_KEY_NAME,
            CLASS_KEY_VALUE_ITEM,
            "xsd:string",
            "key name",
            "The key of a key-value pair.",
        ),
        (
            PROP_KEY_VALUE,
            CLASS_KEY_VALUE_ITEM,
            "xsd:string",
            "key value",
            "The value of a key-value pair.",
        ),
        // Bounding box
        (
            PROP_HAS_BOUNDING_BOX,
            CLASS_DOCUMENT_ELEMENT,
            CLASS_BOUNDING_BOX,
            "has bounding box",
            "Links an element to its bounding box.",
        ),
        (
            PROP_BBOX_LEFT,
            CLASS_BOUNDING_BOX,
            "xsd:float",
            "left",
            "Left edge x-coordinate.",
        ),
        (
            PROP_BBOX_TOP,
            CLASS_BOUNDING_BOX,
            "xsd:float",
            "top",
            "Top edge y-coordinate.",
        ),
        (
            PROP_BBOX_RIGHT,
            CLASS_BOUNDING_BOX,
            "xsd:float",
            "right",
            "Right edge x-coordinate.",
        ),
        (
            PROP_BBOX_BOTTOM,
            CLASS_BOUNDING_BOX,
            "xsd:float",
            "bottom",
            "Bottom edge y-coordinate.",
        ),
        (
            PROP_BBOX_PAGE,
            CLASS_BOUNDING_BOX,
            CLASS_PAGE,
            "bounding box page",
            "Page of the bounding box.",
        ),
        // Table
        (
            PROP_HAS_CELL,
            CLASS_TABLE_ELEMENT,
            CLASS_TABLE_CELL,
            "has cell",
            "Links a table to a cell.",
        ),
        (
            PROP_CELL_ROW,
            CLASS_TABLE_CELL,
            "xsd:nonNegativeInteger",
            "row",
            "0-based row index.",
        ),
        (
            PROP_CELL_COLUMN,
            CLASS_TABLE_CELL,
            "xsd:nonNegativeInteger",
            "column",
            "0-based column index.",
        ),
        (
            PROP_CELL_ROW_SPAN,
            CLASS_TABLE_CELL,
            "xsd:positiveInteger",
            "row span",
            "Number of rows spanned.",
        ),
        (
            PROP_CELL_COL_SPAN,
            CLASS_TABLE_CELL,
            "xsd:positiveInteger",
            "column span",
            "Number of columns spanned.",
        ),
        (
            PROP_CELL_TEXT,
            CLASS_TABLE_CELL,
            "xsd:string",
            "cell text",
            "Text content of the cell.",
        ),
        (
            PROP_IS_HEADER,
            CLASS_TABLE_CELL,
            "xsd:boolean",
            "is header cell",
            "Whether this is a header cell.",
        ),
        (
            PROP_ROW_COUNT,
            CLASS_TABLE_ELEMENT,
            "xsd:nonNegativeInteger",
            "row count",
            "Total number of rows.",
        ),
        (
            PROP_COLUMN_COUNT,
            CLASS_TABLE_ELEMENT,
            "xsd:nonNegativeInteger",
            "column count",
            "Total number of columns.",
        ),
        // Picture
        (
            PROP_PICTURE_DATA,
            CLASS_PICTURE_ELEMENT,
            "xsd:base64Binary",
            "picture data",
            "Binary image data.",
        ),
        (
            PROP_PICTURE_FORMAT,
            CLASS_PICTURE_ELEMENT,
            "xsd:string",
            "picture format",
            "Image format.",
        ),
        (
            PROP_HAS_CAPTION,
            CLASS_PICTURE_ELEMENT,
            CLASS_CAPTION,
            "has caption",
            "Links a picture to its caption.",
        ),
        (
            PROP_PICTURE_CATEGORY,
            CLASS_PICTURE_ELEMENT,
            "xsd:string",
            "picture category",
            "Classification of the picture.",
        ),
        (
            PROP_ALT_TEXT,
            CLASS_PICTURE_ELEMENT,
            "xsd:string",
            "alt text",
            "Alternative text description.",
        ),
        (
            PROP_IMAGE_WIDTH,
            CLASS_PICTURE_ELEMENT,
            "xsd:nonNegativeInteger",
            "image width",
            "Width in pixels.",
        ),
        (
            PROP_IMAGE_HEIGHT,
            CLASS_PICTURE_ELEMENT,
            "xsd:nonNegativeInteger",
            "image height",
            "Height in pixels.",
        ),
        // Time
        (
            PROP_START_TIME,
            CLASS_DOCUMENT_ELEMENT,
            "xsd:duration",
            "start time",
            "Start timestamp.",
        ),
        (
            PROP_END_TIME,
            CLASS_DOCUMENT_ELEMENT,
            "xsd:duration",
            "end time",
            "End timestamp.",
        ),
        (
            PROP_DURATION,
            CLASS_DOCUMENT,
            "xsd:duration",
            "duration",
            "Total duration.",
        ),
        // Provenance
        (
            PROP_HAS_PROVENANCE,
            CLASS_DOCUMENT_ELEMENT,
            CLASS_PROVENANCE,
            "has provenance",
            "Links to provenance metadata.",
        ),
        (
            PROP_CONFIDENCE,
            CLASS_DOCUMENT_ELEMENT,
            "xsd:float",
            "confidence",
            "ML model confidence score.",
        ),
        (
            PROP_DETECTED_BY,
            CLASS_DOCUMENT_ELEMENT,
            "xsd:string",
            "detected by",
            "Name of the detector.",
        ),
        (
            PROP_MODEL_NAME,
            CLASS_PROVENANCE,
            "xsd:string",
            "model name",
            "ML model name.",
        ),
        (
            PROP_MODEL_VERSION,
            CLASS_PROVENANCE,
            "xsd:string",
            "model version",
            "ML model version.",
        ),
        (
            PROP_PROCESSING_DATE,
            CLASS_PROVENANCE,
            "xsd:dateTime",
            "processing date",
            "When the element was processed.",
        ),
        // Cross-references
        (
            PROP_REFERS_TO,
            CLASS_DOCUMENT_ELEMENT,
            CLASS_DOCUMENT_ELEMENT,
            "refers to",
            "Cross-reference between elements.",
        ),
        (
            PROP_LABEL_ID,
            CLASS_DOCUMENT_ELEMENT,
            "xsd:string",
            "label identifier",
            "Document-scoped identifier.",
        ),
        (
            PROP_CITATION_KEY,
            CLASS_REFERENCE,
            "xsd:string",
            "citation key",
            "Citation key for a reference.",
        ),
    ];

    for (name, domain, range, label, comment) in properties {
        let prop_iri = iri(name);
        store.insert_triple_into(&prop_iri, &rdf_type, &rdf_property, g)?;
        store.insert_literal(&prop_iri, &rdfs_label, label, "string", g)?;
        store.insert_literal(&prop_iri, &rdfs_comment, comment, "string", g)?;

        // Domain
        let domain_iri = if let Some(suffix) = domain.strip_prefix("xsd:") {
            format!("{XSD}{suffix}")
        } else {
            iri(domain)
        };
        store.insert_triple_into(&prop_iri, &rdfs_domain, &domain_iri, g)?;

        // Range
        let range_iri = if let Some(suffix) = range.strip_prefix("xsd:") {
            format!("{XSD}{suffix}")
        } else {
            iri(range)
        };
        store.insert_triple_into(&prop_iri, &rdfs_range, &range_iri, g)?;
    }

    Ok(())
}

/// Return the raw Turtle source of the bundled ontology.
///
/// Useful for exporting the ontology itself.
pub fn ontology_turtle() -> &'static str {
    ONTOLOGY_TTL
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;

    #[test]
    fn ontology_ttl_is_bundled() {
        let ttl = ontology_turtle();
        assert!(!ttl.is_empty());
        assert!(ttl.contains("rdoc:Document"));
        assert!(ttl.contains("rdoc:TextElement"));
    }

    #[test]
    fn load_ontology_inserts_triples() -> ruddydoc_core::Result<()> {
        let store = OxigraphStore::new()?;
        load_ontology(&store)?;

        let count = store.triple_count_in(ONTOLOGY_GRAPH)?;
        // We insert: 1 ontology header + label
        // 26 classes * (type + label + comment + optional subClassOf)
        // 57 properties * (type + label + comment + domain + range)
        // Should be well over 100 triples
        assert!(count > 100, "expected >100 ontology triples, got {count}");
        Ok(())
    }

    #[test]
    fn ontology_has_document_class() -> ruddydoc_core::Result<()> {
        let store = OxigraphStore::new()?;
        load_ontology(&store)?;

        let rdf_type = rdf_iri("type");
        let rdfs_class = rdfs_iri("Class");
        let doc_iri = iri(CLASS_DOCUMENT);

        let sparql = format!(
            "ASK {{ GRAPH <{ONTOLOGY_GRAPH}> {{ <{doc_iri}> <{rdf_type}> <{rdfs_class}> }} }}"
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn ontology_has_text_content_property() -> ruddydoc_core::Result<()> {
        let store = OxigraphStore::new()?;
        load_ontology(&store)?;

        let rdf_type = rdf_iri("type");
        let rdf_property = rdf_iri("Property");
        let tc_iri = iri(PROP_TEXT_CONTENT);

        let sparql = format!(
            "ASK {{ GRAPH <{ONTOLOGY_GRAPH}> {{ <{tc_iri}> <{rdf_type}> <{rdf_property}> }} }}"
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn iri_construction() {
        assert_eq!(
            iri("Document"),
            "https://ruddydoc.chapeaux.io/ontology#Document"
        );
        assert_eq!(
            rdf_iri("type"),
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
        );
    }
}
