//! ASCII-folding text normalization pipeline stage.
//!
//! This module provides `ascii_fold()` for stripping combining diacritical
//! marks after NFKD decomposition, and a `TextNormalizationStage` that
//! automatically writes `rdoc:normalizedText` (and `rdoc:normalizedCellText`)
//! triples for any text that differs from its folded form.

use std::time::Instant;

use unicode_normalization::UnicodeNormalization;

use ruddydoc_ontology as ont;

use crate::{PipelineContext, PipelineStage, StageResult};

// TODO: Replace these local constants with ontology constants once
// `PROP_NORMALIZED_TEXT` and `PROP_NORMALIZED_CELL_TEXT` are added
// to `ruddydoc-ontology` (Step 1).
const PROP_NORMALIZED_TEXT: &str = "normalizedText";
const PROP_NORMALIZED_CELL_TEXT: &str = "normalizedCellText";

/// ASCII-fold a string: NFKD decompose, strip combining diacritical marks,
/// and collect the remaining characters.
///
/// This transforms accented characters into their ASCII base forms while
/// preserving characters that are not decomposable (e.g., `ß` stays as `ß`).
///
/// # Examples
///
/// ```
/// use ruddydoc_pipeline::ascii_fold;
///
/// assert_eq!(ascii_fold("résumé"), "resume");
/// assert_eq!(ascii_fold("café"), "cafe");
/// assert_eq!(ascii_fold("naïve"), "naive");
/// assert_eq!(ascii_fold("Hello World"), "Hello World");
/// ```
pub fn ascii_fold(s: &str) -> String {
    s.nfkd()
        .filter(|c| {
            !matches!(
                *c,
                '\u{0300}'..='\u{036F}'   // Combining Diacritical Marks
                | '\u{1AB0}'..='\u{1AFF}' // Combining Diacritical Marks Extended
                | '\u{1DC0}'..='\u{1DFF}' // Combining Diacritical Marks Supplement
                | '\u{20D0}'..='\u{20FF}' // Combining Diacritical Marks for Symbols
                | '\u{FE20}'..='\u{FE2F}' // Combining Half Marks
            )
        })
        .collect()
}

/// Extract the plain string value from a SPARQL literal representation.
///
/// Handles forms like:
/// - `"value"^^<http://www.w3.org/2001/XMLSchema#string>` -> `value`
/// - `"value"` -> `value`
/// - `value` -> `value` (bare, unlikely from Oxigraph but handled)
fn strip_literal_value(s: &str) -> String {
    let s = s.trim();
    if let Some(stripped) = s.strip_prefix('"')
        && let Some(end_quote) = stripped.find('"')
    {
        return stripped[..end_quote].to_string();
    }
    s.to_string()
}

/// Strip angle brackets from an IRI string returned by SPARQL.
///
/// Handles `<http://example.com/foo>` -> `http://example.com/foo` and
/// typed literals like `"42"^^<xsd:integer>`.
fn strip_iri(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('<') && s.ends_with('>') {
        s[1..s.len() - 1].to_string()
    } else if let Some(pos) = s.find("\"^^<") {
        s[1..pos].to_string()
    } else {
        s.to_string()
    }
}

/// Pipeline stage that adds ASCII-folded normalized text for each text element.
///
/// For every element with `rdoc:textContent`, this stage computes the
/// ASCII-folded form via [`ascii_fold`]. If the folded form differs from
/// the original, it inserts a `rdoc:normalizedText` triple.
///
/// The same is done for table cells: `rdoc:cellText` is folded into
/// `rdoc:normalizedCellText`.
pub struct TextNormalizationStage;

impl PipelineStage for TextNormalizationStage {
    fn name(&self) -> &str {
        "text_normalization"
    }

    fn process(&self, ctx: &mut PipelineContext) -> ruddydoc_core::Result<StageResult> {
        let start = Instant::now();
        let mut elements_modified = 0usize;

        // 1. Normalize rdoc:textContent -> rdoc:normalizedText
        elements_modified += normalize_property(ctx, ont::PROP_TEXT_CONTENT, PROP_NORMALIZED_TEXT)?;

        // 2. Normalize rdoc:cellText -> rdoc:normalizedCellText
        elements_modified +=
            normalize_property(ctx, ont::PROP_CELL_TEXT, PROP_NORMALIZED_CELL_TEXT)?;

        Ok(StageResult {
            stage_name: self.name().to_string(),
            elements_added: 0,
            elements_modified,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

/// Query all elements with the given `source_prop`, compute `ascii_fold` on
/// each value, and insert the normalized value as `target_prop` when it
/// differs from the original.
///
/// Returns the number of elements that were modified (i.e., had a normalized
/// value inserted).
fn normalize_property(
    ctx: &mut PipelineContext,
    source_prop: &str,
    target_prop: &str,
) -> ruddydoc_core::Result<usize> {
    let sparql = format!(
        "SELECT ?e ?text WHERE {{ GRAPH <{graph}> {{ ?e <{prop}> ?text }} }}",
        graph = ctx.doc_graph,
        prop = ont::iri(source_prop),
    );

    let results = ctx.store.query_to_json(&sparql)?;
    let rows = results.as_array().unwrap_or(&Vec::new()).clone();

    let target_iri = ont::iri(target_prop);
    let mut modified = 0usize;

    for row in &rows {
        let element_iri = match row["e"].as_str() {
            Some(s) => strip_iri(s),
            None => continue,
        };
        let text = match row["text"].as_str() {
            Some(s) => strip_literal_value(s),
            None => continue,
        };

        let normalized = ascii_fold(&text);
        if normalized != text {
            ctx.store.insert_literal(
                &element_iri,
                &target_iri,
                &normalized,
                "string",
                &ctx.doc_graph,
            )?;
            modified += 1;
        }
    }

    Ok(modified)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_core::{DocumentHash, DocumentMeta, DocumentStore, InputFormat};
    use ruddydoc_graph::OxigraphStore;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::{PipelineContext, PipelineOptions};

    /// Create a test store wrapped in Arc.
    fn test_store() -> Arc<OxigraphStore> {
        Arc::new(OxigraphStore::new().expect("failed to create test store"))
    }

    /// Create a minimal DocumentMeta for testing.
    fn test_doc_meta() -> DocumentMeta {
        DocumentMeta {
            file_path: Some(PathBuf::from("test.pdf")),
            hash: DocumentHash("normtest".to_string()),
            format: InputFormat::Pdf,
            file_size: 1024,
            page_count: Some(1),
            language: None,
        }
    }

    /// Create a PipelineContext with the given store.
    fn test_context(store: Arc<OxigraphStore>) -> PipelineContext {
        PipelineContext {
            store,
            doc_graph: "urn:ruddydoc:doc:normtest".to_string(),
            doc_meta: test_doc_meta(),
            page_images: Vec::new(),
            options: PipelineOptions::default(),
        }
    }

    // -----------------------------------------------------------------------
    // Unit tests for ascii_fold
    // -----------------------------------------------------------------------

    #[test]
    fn fold_resume() {
        assert_eq!(ascii_fold("résumé"), "resume");
    }

    #[test]
    fn fold_cafe() {
        assert_eq!(ascii_fold("café"), "cafe");
    }

    #[test]
    fn fold_naive() {
        assert_eq!(ascii_fold("naïve"), "naive");
    }

    #[test]
    fn fold_no_change() {
        assert_eq!(ascii_fold("Hello World"), "Hello World");
    }

    #[test]
    fn fold_mixed() {
        assert_eq!(ascii_fold("Le résumé du café"), "Le resume du cafe");
    }

    #[test]
    fn fold_empty() {
        assert_eq!(ascii_fold(""), "");
    }

    #[test]
    fn fold_german_umlauts() {
        assert_eq!(ascii_fold("über"), "uber");
        // ß is not a combining mark; NFKD does not decompose it, so it's preserved.
        assert_eq!(ascii_fold("Straße"), "Straße");
    }

    #[test]
    fn fold_spanish() {
        assert_eq!(ascii_fold("niño"), "nino");
        assert_eq!(ascii_fold("señor"), "senor");
    }

    #[test]
    fn fold_preserves_case() {
        assert_eq!(ascii_fold("Résumé"), "Resume");
    }

    #[test]
    fn fold_czech() {
        assert_eq!(ascii_fold("příliš žluťoučký"), "prilis zlutoucky");
    }

    #[test]
    fn fold_ascii_only() {
        let s = "The quick brown fox jumps over the lazy dog.";
        assert_eq!(ascii_fold(s), s);
    }

    #[test]
    fn fold_numbers_and_punctuation() {
        let s = "Price: $42.99 (25% off!)";
        assert_eq!(ascii_fold(s), s);
    }

    // -----------------------------------------------------------------------
    // Unit tests for strip_literal_value
    // -----------------------------------------------------------------------

    #[test]
    fn strip_literal_typed() {
        assert_eq!(
            strip_literal_value("\"hello\"^^<http://www.w3.org/2001/XMLSchema#string>"),
            "hello"
        );
    }

    #[test]
    fn strip_literal_plain_quoted() {
        assert_eq!(strip_literal_value("\"hello\""), "hello");
    }

    #[test]
    fn strip_literal_bare() {
        assert_eq!(strip_literal_value("hello"), "hello");
    }

    #[test]
    fn strip_literal_accented() {
        assert_eq!(
            strip_literal_value("\"résumé\"^^<http://www.w3.org/2001/XMLSchema#string>"),
            "résumé"
        );
    }

    // -----------------------------------------------------------------------
    // Unit tests for strip_iri
    // -----------------------------------------------------------------------

    #[test]
    fn strip_iri_with_brackets() {
        assert_eq!(
            strip_iri("<urn:ruddydoc:doc:test/el-0>"),
            "urn:ruddydoc:doc:test/el-0"
        );
    }

    #[test]
    fn strip_iri_without_brackets() {
        assert_eq!(
            strip_iri("urn:ruddydoc:doc:test/el-0"),
            "urn:ruddydoc:doc:test/el-0"
        );
    }

    // -----------------------------------------------------------------------
    // Integration tests: TextNormalizationStage
    // -----------------------------------------------------------------------

    #[test]
    fn normalization_stage_adds_normalized_text() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:normtest";

        // Insert an element with accented textContent.
        let el_iri = "urn:ruddydoc:doc:normtest/para-0";
        store
            .insert_triple_into(
                el_iri,
                &ont::rdf_iri("type"),
                &ont::iri(ont::CLASS_PARAGRAPH),
                g,
            )
            .expect("insert type");
        store
            .insert_literal(
                el_iri,
                &ont::iri(ont::PROP_TEXT_CONTENT),
                "Le résumé du café",
                "string",
                g,
            )
            .expect("insert textContent");

        let mut ctx = test_context(store);
        let stage = TextNormalizationStage;
        let result = stage.process(&mut ctx).expect("stage should succeed");

        assert_eq!(result.stage_name, "text_normalization");
        assert_eq!(result.elements_modified, 1);
        assert_eq!(result.elements_added, 0);

        // Verify the normalizedText triple was inserted.
        let sparql = format!(
            "SELECT ?norm WHERE {{ GRAPH <{g}> {{ <{el_iri}> <{prop}> ?norm }} }}",
            prop = ont::iri(PROP_NORMALIZED_TEXT),
        );
        let json = ctx
            .store
            .query_to_json(&sparql)
            .expect("query should succeed");
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let norm_val = strip_literal_value(rows[0]["norm"].as_str().expect("expected string"));
        assert_eq!(norm_val, "Le resume du cafe");
    }

    #[test]
    fn normalization_stage_skips_ascii_text() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:normtest";

        // Insert an element with pure ASCII textContent.
        let el_iri = "urn:ruddydoc:doc:normtest/para-ascii";
        store
            .insert_triple_into(
                el_iri,
                &ont::rdf_iri("type"),
                &ont::iri(ont::CLASS_PARAGRAPH),
                g,
            )
            .expect("insert type");
        store
            .insert_literal(
                el_iri,
                &ont::iri(ont::PROP_TEXT_CONTENT),
                "Hello World",
                "string",
                g,
            )
            .expect("insert textContent");

        let mut ctx = test_context(store);
        let stage = TextNormalizationStage;
        let result = stage.process(&mut ctx).expect("stage should succeed");

        // No elements should be modified because normalized == original.
        assert_eq!(result.elements_modified, 0);

        // Verify no normalizedText triple was inserted.
        let sparql = format!(
            "SELECT ?norm WHERE {{ GRAPH <{g}> {{ <{el_iri}> <{prop}> ?norm }} }}",
            prop = ont::iri(PROP_NORMALIZED_TEXT),
        );
        let json = ctx
            .store
            .query_to_json(&sparql)
            .expect("query should succeed");
        let rows = json.as_array().expect("expected array");
        assert!(
            rows.is_empty(),
            "no normalizedText should be inserted for ASCII text"
        );
    }

    #[test]
    fn normalization_stage_handles_table_cells() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:normtest";

        // Insert a table cell with accented cellText.
        let cell_iri = "urn:ruddydoc:doc:normtest/cell-0";
        store
            .insert_triple_into(
                cell_iri,
                &ont::rdf_iri("type"),
                &ont::iri(ont::CLASS_TABLE_CELL),
                g,
            )
            .expect("insert type");
        store
            .insert_literal(
                cell_iri,
                &ont::iri(ont::PROP_CELL_TEXT),
                "Événement",
                "string",
                g,
            )
            .expect("insert cellText");

        let mut ctx = test_context(store);
        let stage = TextNormalizationStage;
        let result = stage.process(&mut ctx).expect("stage should succeed");

        assert_eq!(result.elements_modified, 1);

        // Verify normalizedCellText was inserted.
        let sparql = format!(
            "SELECT ?norm WHERE {{ GRAPH <{g}> {{ <{cell_iri}> <{prop}> ?norm }} }}",
            prop = ont::iri(PROP_NORMALIZED_CELL_TEXT),
        );
        let json = ctx
            .store
            .query_to_json(&sparql)
            .expect("query should succeed");
        let rows = json.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let norm_val = strip_literal_value(rows[0]["norm"].as_str().expect("expected string"));
        assert_eq!(norm_val, "Evenement");
    }

    #[test]
    fn normalization_stage_handles_mixed_elements() {
        let store = test_store();
        let g = "urn:ruddydoc:doc:normtest";

        // Element 1: accented text (should be normalized)
        let el1 = "urn:ruddydoc:doc:normtest/para-1";
        store
            .insert_literal(el1, &ont::iri(ont::PROP_TEXT_CONTENT), "café", "string", g)
            .expect("insert");

        // Element 2: ASCII text (should be skipped)
        let el2 = "urn:ruddydoc:doc:normtest/para-2";
        store
            .insert_literal(el2, &ont::iri(ont::PROP_TEXT_CONTENT), "hello", "string", g)
            .expect("insert");

        // Element 3: accented text (should be normalized)
        let el3 = "urn:ruddydoc:doc:normtest/para-3";
        store
            .insert_literal(el3, &ont::iri(ont::PROP_TEXT_CONTENT), "naïve", "string", g)
            .expect("insert");

        let mut ctx = test_context(store);
        let stage = TextNormalizationStage;
        let result = stage.process(&mut ctx).expect("stage should succeed");

        // Only elements 1 and 3 should be modified.
        assert_eq!(result.elements_modified, 2);
    }

    #[test]
    fn normalization_stage_no_elements() {
        let store = test_store();
        let mut ctx = test_context(store);

        let stage = TextNormalizationStage;
        let result = stage.process(&mut ctx).expect("stage should succeed");

        assert_eq!(result.stage_name, "text_normalization");
        assert_eq!(result.elements_modified, 0);
        assert_eq!(result.elements_added, 0);
    }

    #[test]
    fn normalization_stage_name() {
        let stage = TextNormalizationStage;
        assert_eq!(stage.name(), "text_normalization");
    }
}
