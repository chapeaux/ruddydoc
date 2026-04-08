//! Document chunking for RAG (Retrieval Augmented Generation) workflows.
//!
//! Provides structure-aware chunking that operates on the document graph via
//! SPARQL queries. Each chunk captures a portion of the document as text
//! with metadata about its position, source elements, and heading hierarchy.
//!
//! The primary implementation is [`HierarchicalChunker`], which uses the
//! document's structural hierarchy (headings, sections) to produce chunks
//! that respect semantic boundaries.

use serde::{Deserialize, Serialize};

use ruddydoc_core::DocumentStore;
use ruddydoc_ontology as ont;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single chunk of document text with associated metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// The text content of the chunk, optionally prefixed with heading context.
    pub text: String,
    /// Metadata describing the chunk's origin and position.
    pub metadata: ChunkMetadata,
}

/// Metadata for a single chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Zero-based index of this chunk in the output sequence.
    pub chunk_index: usize,
    /// IRIs of the source document elements that contributed to this chunk.
    pub source_elements: Vec<String>,
    /// Heading hierarchy (breadcrumb) active at the start of this chunk.
    /// For example: `["Chapter 1", "Section 1.2"]`.
    pub headings: Vec<String>,
    /// Page numbers this chunk spans (empty for non-paginated documents).
    pub page_numbers: Vec<u32>,
    /// The ontology class local names of elements in this chunk
    /// (e.g., `"Paragraph"`, `"ListItem"`, `"Code"`).
    pub element_types: Vec<String>,
}

/// Options controlling how a document is chunked.
#[derive(Debug, Clone)]
pub struct ChunkOptions {
    /// Approximate maximum tokens per chunk.
    pub max_tokens: usize,
    /// Number of overlap tokens between consecutive chunks when splitting
    /// an oversized element.
    pub overlap_tokens: usize,
    /// When `true`, merge consecutive list items under the same parent
    /// into a single chunk.
    pub merge_list_items: bool,
    /// When `true`, prepend the heading hierarchy (breadcrumb) to each
    /// chunk's text for contextualised embedding.
    pub include_headings: bool,
    /// Approximate characters-per-token ratio used for token estimation.
    /// The default of `4.0` is a reasonable average for English text with
    /// a BPE tokenizer.
    pub chars_per_token: f32,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        Self {
            max_tokens: 512,
            overlap_tokens: 50,
            merge_list_items: true,
            include_headings: true,
            chars_per_token: 4.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Trait for document chunking strategies.
pub trait DocumentChunker: Send + Sync {
    /// Produce chunks from a document stored in the given named graph.
    fn chunk(
        &self,
        store: &dyn DocumentStore,
        doc_graph: &str,
        options: &ChunkOptions,
    ) -> ruddydoc_core::Result<Vec<Chunk>>;
}

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

/// Estimate the number of tokens in `text` using the configured
/// characters-per-token ratio.
fn estimate_tokens(text: &str, chars_per_token: f32) -> usize {
    if chars_per_token <= 0.0 {
        return text.len();
    }
    (text.len() as f32 / chars_per_token).ceil() as usize
}

/// Estimate the maximum number of characters that fit within `max_tokens`.
fn max_chars(max_tokens: usize, chars_per_token: f32) -> usize {
    (max_tokens as f32 * chars_per_token) as usize
}

// ---------------------------------------------------------------------------
// Internal element representation
// ---------------------------------------------------------------------------

/// An element extracted from the graph, ready for chunking.
#[derive(Debug, Clone)]
struct GraphElement {
    /// The element's IRI (e.g., `<urn:ruddydoc:doc:abc/heading-0>`).
    iri: String,
    /// The ontology class local name (e.g., `"SectionHeader"`).
    element_type: String,
    /// Reading order (for sorting). The query uses ORDER BY so elements
    /// arrive sorted, but we keep the value for diagnostics.
    _reading_order: i64,
    /// Text content (empty for non-text elements like tables).
    text: String,
    /// Heading level (only meaningful for `SectionHeader`).
    heading_level: Option<u32>,
    /// Page number, if the element has one.
    page_number: Option<u32>,
    /// IRI of the parent element, if any.
    parent_iri: Option<String>,
}

// ---------------------------------------------------------------------------
// SPARQL literal helpers (duplicated from json_export/markdown_export to
// keep modules self-contained; could be factored out later)
// ---------------------------------------------------------------------------

/// Strip Oxigraph's typed-literal decoration from a SPARQL result value.
fn clean_literal(s: &str) -> String {
    if let Some(idx) = s.find("\"^^<") {
        return s[1..idx].to_string();
    }
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

fn parse_int(s: &str) -> i64 {
    let cleaned = clean_literal(s);
    cleaned.parse().unwrap_or(0)
}

/// Strip angle brackets from an IRI string returned by SPARQL.
fn clean_iri(s: &str) -> String {
    s.trim_start_matches('<').trim_end_matches('>').to_string()
}

/// Extract the ontology local name from a full IRI.
/// E.g. `"https://ruddydoc.chapeaux.io/ontology#Paragraph"` -> `"Paragraph"`.
fn local_name(iri: &str) -> String {
    let cleaned = clean_iri(iri);
    if let Some(pos) = cleaned.rfind('#') {
        cleaned[pos + 1..].to_string()
    } else if let Some(pos) = cleaned.rfind('/') {
        cleaned[pos + 1..].to_string()
    } else {
        cleaned
    }
}

// ---------------------------------------------------------------------------
// HierarchicalChunker
// ---------------------------------------------------------------------------

/// Chunks a document by its structural hierarchy.
///
/// The algorithm:
/// 1. Query all elements in reading order with types, text, heading levels,
///    page numbers, and parent relationships.
/// 2. Track the heading hierarchy (breadcrumb) by updating a stack as
///    section headers are encountered.
/// 3. Each structural element (paragraph, code block, table text) becomes a
///    chunk candidate.
/// 4. Consecutive small elements under the same heading are merged until
///    `max_tokens` would be exceeded.
/// 5. Oversized elements (very long paragraphs) are split with overlap.
pub struct HierarchicalChunker;

impl DocumentChunker for HierarchicalChunker {
    fn chunk(
        &self,
        store: &dyn DocumentStore,
        doc_graph: &str,
        options: &ChunkOptions,
    ) -> ruddydoc_core::Result<Vec<Chunk>> {
        let elements = query_elements(store, doc_graph)?;
        if elements.is_empty() {
            return Ok(Vec::new());
        }

        // Build chunk candidates from elements.
        let candidates = build_candidates(&elements, options);

        // Merge small consecutive candidates that share the same heading context.
        let merged = merge_candidates(candidates, options);

        // Split any candidates that exceed max_tokens.
        let split = split_oversized(merged, options);

        // Number chunks and produce final output.
        let chunks = split
            .into_iter()
            .enumerate()
            .map(|(i, mut c)| {
                c.metadata.chunk_index = i;
                c
            })
            .collect();

        Ok(chunks)
    }
}

// ---------------------------------------------------------------------------
// Graph querying
// ---------------------------------------------------------------------------

/// Query all relevant elements from the document graph in reading order.
fn query_elements(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<Vec<GraphElement>> {
    let sparql = format!(
        "SELECT ?el ?type ?text ?order ?level ?page ?parent WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?el <{reading_order}> ?order. \
             ?el a ?type. \
             OPTIONAL {{ ?el <{text_content}> ?text }} \
             OPTIONAL {{ ?el <{heading_level}> ?level }} \
             OPTIONAL {{ ?el <{on_page}> ?pageNode. ?pageNode <{page_number}> ?page }} \
             OPTIONAL {{ ?el <{parent_element}> ?parent }} \
             FILTER(?type IN ( \
               <{section_header}>, \
               <{paragraph}>, \
               <{list_item}>, \
               <{code}>, \
               <{table}>, \
               <{formula}>, \
               <{title}> \
             )) \
           }} \
         }} ORDER BY ?order",
        reading_order = ont::iri(ont::PROP_READING_ORDER),
        text_content = ont::iri(ont::PROP_TEXT_CONTENT),
        heading_level = ont::iri(ont::PROP_HEADING_LEVEL),
        on_page = ont::iri(ont::PROP_ON_PAGE),
        page_number = ont::iri(ont::PROP_PAGE_NUMBER),
        parent_element = ont::iri(ont::PROP_PARENT_ELEMENT),
        section_header = ont::iri(ont::CLASS_SECTION_HEADER),
        paragraph = ont::iri(ont::CLASS_PARAGRAPH),
        list_item = ont::iri(ont::CLASS_LIST_ITEM),
        code = ont::iri(ont::CLASS_CODE),
        table = ont::iri(ont::CLASS_TABLE_ELEMENT),
        formula = ont::iri(ont::CLASS_FORMULA),
        title = ont::iri(ont::CLASS_TITLE),
    );

    let result = store.query_to_json(&sparql)?;
    let rows = match result.as_array() {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let mut elements = Vec::with_capacity(rows.len());

    for row in rows {
        let iri = row
            .get("el")
            .and_then(|v| v.as_str())
            .map(clean_iri)
            .unwrap_or_default();
        let element_type = row
            .get("type")
            .and_then(|v| v.as_str())
            .map(local_name)
            .unwrap_or_default();
        let reading_order = row
            .get("order")
            .and_then(|v| v.as_str())
            .map(parse_int)
            .unwrap_or(0);
        let text = row
            .get("text")
            .and_then(|v| v.as_str())
            .map(clean_literal)
            .unwrap_or_default();
        let heading_level = row
            .get("level")
            .and_then(|v| v.as_str())
            .map(|s| parse_int(s) as u32);
        let page_number = row
            .get("page")
            .and_then(|v| v.as_str())
            .map(|s| parse_int(s) as u32);
        let parent_iri = row.get("parent").and_then(|v| v.as_str()).map(clean_iri);

        elements.push(GraphElement {
            iri,
            element_type,
            _reading_order: reading_order,
            text,
            heading_level,
            page_number,
            parent_iri,
        });
    }

    Ok(elements)
}

/// For table elements, query the cell text and concatenate it into a
/// readable representation.
fn query_table_text(
    store: &dyn DocumentStore,
    doc_graph: &str,
    table_iri: &str,
) -> ruddydoc_core::Result<String> {
    let sparql = format!(
        "SELECT ?text ?row ?col WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             <{table_iri}> <{has_cell}> ?cell. \
             ?cell <{cell_text}> ?text. \
             ?cell <{cell_row}> ?row. \
             ?cell <{cell_col}> ?col \
           }} \
         }} ORDER BY ?row ?col",
        has_cell = ont::iri(ont::PROP_HAS_CELL),
        cell_text = ont::iri(ont::PROP_CELL_TEXT),
        cell_row = ont::iri(ont::PROP_CELL_ROW),
        cell_col = ont::iri(ont::PROP_CELL_COLUMN),
    );
    let result = store.query_to_json(&sparql)?;

    let rows = match result.as_array() {
        Some(r) => r,
        None => return Ok(String::new()),
    };

    if rows.is_empty() {
        return Ok(String::new());
    }

    // Group cells by row for readable output.
    let mut table: std::collections::BTreeMap<i64, Vec<(i64, String)>> =
        std::collections::BTreeMap::new();

    for row in rows {
        let text = row
            .get("text")
            .and_then(|v| v.as_str())
            .map(clean_literal)
            .unwrap_or_default();
        let r = row
            .get("row")
            .and_then(|v| v.as_str())
            .map(parse_int)
            .unwrap_or(0);
        let c = row
            .get("col")
            .and_then(|v| v.as_str())
            .map(parse_int)
            .unwrap_or(0);
        table.entry(r).or_default().push((c, text));
    }

    for cells in table.values_mut() {
        cells.sort_by_key(|(c, _)| *c);
    }

    let mut output = String::new();
    for cells in table.values() {
        let line: Vec<&str> = cells.iter().map(|(_, t)| t.as_str()).collect();
        output.push_str(&line.join(" | "));
        output.push('\n');
    }

    Ok(output.trim_end().to_string())
}

// ---------------------------------------------------------------------------
// Candidate building
// ---------------------------------------------------------------------------

/// A candidate chunk before merging/splitting.
#[derive(Debug, Clone)]
struct ChunkCandidate {
    text: String,
    source_elements: Vec<String>,
    headings: Vec<String>,
    page_numbers: Vec<u32>,
    element_types: Vec<String>,
}

/// Walk the elements in reading order, track heading breadcrumb, and produce
/// one candidate per structural element (or merged list items).
fn build_candidates(elements: &[GraphElement], options: &ChunkOptions) -> Vec<ChunkCandidate> {
    let mut candidates: Vec<ChunkCandidate> = Vec::new();
    // Heading stack: each entry is (level, text). When we encounter a heading
    // of level N, we pop all headings with level >= N.
    let mut heading_stack: Vec<(u32, String)> = Vec::new();

    // For list-item merging: accumulate consecutive list items that share
    // a parent.
    let mut list_buf: Option<ChunkCandidate> = None;
    let mut list_parent: Option<String> = None;

    for elem in elements {
        // If this element is not a list item (or list-item merging is off),
        // flush any accumulated list buffer.
        let is_list_item = elem.element_type == "ListItem";
        if (!is_list_item || !options.merge_list_items)
            && let Some(buf) = list_buf.take()
        {
            candidates.push(buf);
            list_parent = None;
        }

        match elem.element_type.as_str() {
            "SectionHeader" | "Title" => {
                let level = elem.heading_level.unwrap_or(1);

                // Update heading stack: remove headings at the same or deeper level.
                while heading_stack.last().is_some_and(|(l, _)| *l >= level) {
                    heading_stack.pop();
                }
                heading_stack.push((level, elem.text.clone()));

                // Headings themselves become chunk candidates so that a
                // heading-only section is not lost.
                let headings: Vec<String> = heading_stack.iter().map(|(_, t)| t.clone()).collect();
                candidates.push(ChunkCandidate {
                    text: elem.text.clone(),
                    source_elements: vec![elem.iri.clone()],
                    headings,
                    page_numbers: elem.page_number.into_iter().collect(),
                    element_types: vec![elem.element_type.clone()],
                });
            }
            "ListItem" if options.merge_list_items => {
                let parent = elem.parent_iri.clone();
                let same_parent = list_parent.as_ref() == parent.as_ref() && parent.is_some();

                if same_parent {
                    // Append to existing buffer.
                    if let Some(buf) = list_buf.as_mut() {
                        buf.text.push('\n');
                        buf.text.push_str(&elem.text);
                        buf.source_elements.push(elem.iri.clone());
                        if let Some(p) = elem.page_number
                            && !buf.page_numbers.contains(&p)
                        {
                            buf.page_numbers.push(p);
                        }
                        // element_types already contains "ListItem"
                    }
                } else {
                    // Flush previous list buffer.
                    if let Some(buf) = list_buf.take() {
                        candidates.push(buf);
                    }
                    let headings: Vec<String> =
                        heading_stack.iter().map(|(_, t)| t.clone()).collect();
                    list_buf = Some(ChunkCandidate {
                        text: elem.text.clone(),
                        source_elements: vec![elem.iri.clone()],
                        headings,
                        page_numbers: elem.page_number.into_iter().collect(),
                        element_types: vec!["ListItem".to_string()],
                    });
                    list_parent = parent;
                }
            }
            "TableElement" => {
                // Tables have no textContent directly; we skip them and
                // represent them as "[Table]" placeholder. A more advanced
                // version could call query_table_text, but that requires
                // the store, which is not passed here. Instead we mark
                // them so the caller can enrich later.
                let headings: Vec<String> = heading_stack.iter().map(|(_, t)| t.clone()).collect();
                candidates.push(ChunkCandidate {
                    text: elem.text.clone(), // will be empty; enriched later
                    source_elements: vec![elem.iri.clone()],
                    headings,
                    page_numbers: elem.page_number.into_iter().collect(),
                    element_types: vec![elem.element_type.clone()],
                });
            }
            _ => {
                // Paragraph, Code, Formula, or ListItem with merge off.
                if !elem.text.is_empty() {
                    let headings: Vec<String> =
                        heading_stack.iter().map(|(_, t)| t.clone()).collect();
                    candidates.push(ChunkCandidate {
                        text: elem.text.clone(),
                        source_elements: vec![elem.iri.clone()],
                        headings,
                        page_numbers: elem.page_number.into_iter().collect(),
                        element_types: vec![elem.element_type.clone()],
                    });
                }
            }
        }
    }

    // Flush any remaining list buffer.
    if let Some(buf) = list_buf.take() {
        candidates.push(buf);
    }

    candidates
}

/// Enrich table candidates with their cell text.
fn enrich_table_candidates(
    candidates: &mut [ChunkCandidate],
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<()> {
    for candidate in candidates.iter_mut() {
        if candidate.element_types.iter().any(|t| t == "TableElement")
            && candidate.text.is_empty()
            && let Some(table_iri) = candidate.source_elements.first()
        {
            let table_text = query_table_text(store, doc_graph, table_iri)?;
            if !table_text.is_empty() {
                candidate.text = table_text;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Merging
// ---------------------------------------------------------------------------

/// Merge consecutive small candidates that share the same heading context.
fn merge_candidates(candidates: Vec<ChunkCandidate>, options: &ChunkOptions) -> Vec<Chunk> {
    let mut result: Vec<Chunk> = Vec::new();

    for candidate in candidates {
        let candidate_text = contextualize_text(&candidate.text, &candidate.headings, options);
        let candidate_tokens = estimate_tokens(&candidate_text, options.chars_per_token);

        let candidate_is_list_item = candidate.element_types.iter().any(|t| t == "ListItem");

        let should_merge = if let Some(last) = result.last() {
            // Merge if same heading context and combined size fits.
            let same_headings = last.metadata.headings == candidate.headings;
            let combined_text = format!("{}\n\n{}", last.text, candidate_text);
            let combined_tokens = estimate_tokens(&combined_text, options.chars_per_token);

            let last_is_list_item = last.metadata.element_types.iter().any(|t| t == "ListItem");

            // When merge_list_items is disabled, do not merge list items
            // with other list items or with non-list content.
            let list_merge_blocked =
                !options.merge_list_items && (candidate_is_list_item || last_is_list_item);

            same_headings && combined_tokens <= options.max_tokens
                && !list_merge_blocked
                // Don't merge headings with content that follows them.
                && !last
                    .metadata
                    .element_types
                    .iter()
                    .any(|t| t == "SectionHeader" || t == "Title")
                && !candidate
                    .element_types
                    .iter()
                    .any(|t| t == "SectionHeader" || t == "Title")
        } else {
            false
        };

        if should_merge {
            let last = result.last_mut().expect("checked above");
            last.text = format!("{}\n\n{}", last.text, candidate_text);
            last.metadata
                .source_elements
                .extend(candidate.source_elements);
            for p in &candidate.page_numbers {
                if !last.metadata.page_numbers.contains(p) {
                    last.metadata.page_numbers.push(*p);
                }
            }
            for t in &candidate.element_types {
                if !last.metadata.element_types.contains(t) {
                    last.metadata.element_types.push(t.clone());
                }
            }
        } else {
            // If the candidate itself is a heading and the next element
            // will provide content, we still emit it as a standalone chunk
            // for now; the heading text carries value.
            if candidate_tokens == 0 {
                continue;
            }
            result.push(Chunk {
                text: candidate_text,
                metadata: ChunkMetadata {
                    chunk_index: 0, // will be set later
                    source_elements: candidate.source_elements,
                    headings: candidate.headings,
                    page_numbers: candidate.page_numbers,
                    element_types: candidate.element_types,
                },
            });
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Splitting
// ---------------------------------------------------------------------------

/// Split chunks that exceed `max_tokens` into multiple chunks with overlap.
fn split_oversized(chunks: Vec<Chunk>, options: &ChunkOptions) -> Vec<Chunk> {
    let mut result = Vec::new();

    for chunk in chunks {
        let tokens = estimate_tokens(&chunk.text, options.chars_per_token);
        if tokens <= options.max_tokens {
            result.push(chunk);
            continue;
        }

        // Split the text at approximately max_tokens boundaries with overlap.
        let sub_chunks = split_text_with_overlap(
            &chunk.text,
            options.max_tokens,
            options.overlap_tokens,
            options.chars_per_token,
        );

        for sub_text in sub_chunks {
            result.push(Chunk {
                text: sub_text,
                metadata: chunk.metadata.clone(),
            });
        }
    }

    result
}

/// Split `text` into pieces of approximately `max_tokens` tokens with
/// `overlap_tokens` overlap. Splits preferably at whitespace boundaries.
fn split_text_with_overlap(
    text: &str,
    max_tokens: usize,
    overlap_tokens: usize,
    chars_per_token: f32,
) -> Vec<String> {
    let max_c = max_chars(max_tokens, chars_per_token);
    let overlap_c = max_chars(overlap_tokens, chars_per_token);

    if text.len() <= max_c || max_c == 0 {
        return vec![text.to_string()];
    }

    let mut pieces = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = (start + max_c).min(text.len());

        // Try to break at a whitespace boundary before the hard limit.
        let actual_end = if end < text.len() {
            find_break_point(text, start, end)
        } else {
            end
        };

        pieces.push(text[start..actual_end].to_string());

        if actual_end >= text.len() {
            break;
        }

        // Advance start, applying overlap.
        let advance = if actual_end > start + overlap_c {
            actual_end - start - overlap_c
        } else {
            // If the piece is shorter than the overlap, just advance past it.
            actual_end - start
        };
        start += advance;
    }

    pieces
}

/// Find the best position to break text between `start` and `end`.
/// Prefers breaking at newlines, then spaces, falling back to `end`.
fn find_break_point(text: &str, start: usize, end: usize) -> usize {
    let segment = &text[start..end];

    // Prefer breaking at the last newline.
    if let Some(pos) = segment.rfind('\n') {
        return start + pos + 1;
    }
    // Fall back to the last space.
    if let Some(pos) = segment.rfind(' ') {
        return start + pos + 1;
    }
    // No good break point; hard-break at end.
    end
}

// ---------------------------------------------------------------------------
// Contextualisation
// ---------------------------------------------------------------------------

/// Produce the final chunk text, optionally prepending heading context.
fn contextualize_text(text: &str, headings: &[String], options: &ChunkOptions) -> String {
    if !options.include_headings || headings.is_empty() {
        return text.to_string();
    }

    let mut output = String::new();
    for (i, heading) in headings.iter().enumerate() {
        let prefix = "#".repeat(i + 1);
        output.push_str(&format!("{prefix} {heading}\n"));
    }
    output.push('\n');
    output.push_str(text);
    output
}

// ---------------------------------------------------------------------------
// Public convenience function
// ---------------------------------------------------------------------------

/// Chunk a document using the [`HierarchicalChunker`] with the given options.
///
/// This is a convenience wrapper around constructing a `HierarchicalChunker`
/// and calling its `chunk` method, with the additional step of enriching
/// table elements with their cell text.
pub fn chunk_document(
    store: &dyn DocumentStore,
    doc_graph: &str,
    options: &ChunkOptions,
) -> ruddydoc_core::Result<Vec<Chunk>> {
    let elements = query_elements(store, doc_graph)?;
    if elements.is_empty() {
        return Ok(Vec::new());
    }

    let mut candidates = build_candidates(&elements, options);
    enrich_table_candidates(&mut candidates, store, doc_graph)?;

    let merged = merge_candidates(candidates, options);
    let split = split_oversized(merged, options);

    let chunks = split
        .into_iter()
        .enumerate()
        .map(|(i, mut c)| {
            c.metadata.chunk_index = i;
            c
        })
        .collect();

    Ok(chunks)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_core::{DocumentBackend, DocumentSource};
    use ruddydoc_graph::OxigraphStore;

    /// Parse markdown into a store and return the store and graph IRI.
    fn parse_md(md: &str) -> ruddydoc_core::Result<(OxigraphStore, String)> {
        let store = OxigraphStore::new()?;
        let backend = ruddydoc_backend_md::MarkdownBackend::new();
        let source = DocumentSource::Stream {
            name: "test.md".to_string(),
            data: md.as_bytes().to_vec(),
        };
        let hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(md.as_bytes());
            let result = hasher.finalize();
            result.iter().fold(String::new(), |mut s, b| {
                use std::fmt::Write;
                let _ = write!(s, "{b:02x}");
                s
            })
        };
        let doc_graph = ruddydoc_core::doc_iri(&hash);
        backend.parse(&source, &store, &doc_graph)?;
        Ok((store, doc_graph))
    }

    // -------------------------------------------------------------------
    // Basic functionality
    // -------------------------------------------------------------------

    #[test]
    fn chunk_simple_document() -> ruddydoc_core::Result<()> {
        let md = "# Section One\n\nFirst paragraph.\n\n# Section Two\n\nSecond paragraph.\n\nThird paragraph.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        // Should have at least the two headings and 3 paragraphs (some may merge).
        assert!(!chunks.is_empty(), "expected non-empty chunks");

        // Verify chunk indices are sequential.
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.metadata.chunk_index, i);
        }

        Ok(())
    }

    #[test]
    fn chunk_empty_document() -> ruddydoc_core::Result<()> {
        // A document with only whitespace produces no structural elements.
        let md = "\n\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        assert!(
            chunks.is_empty(),
            "expected empty chunks for empty document"
        );
        Ok(())
    }

    #[test]
    fn chunk_single_paragraph() -> ruddydoc_core::Result<()> {
        let md = "Just one paragraph.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        assert_eq!(chunks.len(), 1, "expected exactly one chunk");
        assert!(chunks[0].text.contains("Just one paragraph."));
        assert_eq!(chunks[0].metadata.chunk_index, 0);
        assert!(!chunks[0].metadata.source_elements.is_empty());
        Ok(())
    }

    // -------------------------------------------------------------------
    // Heading hierarchy
    // -------------------------------------------------------------------

    #[test]
    fn heading_hierarchy_in_metadata() -> ruddydoc_core::Result<()> {
        let md = "# Chapter\n\n## Section\n\nParagraph text.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions {
            include_headings: true,
            ..Default::default()
        };
        let chunks = chunk_document(&store, &graph, &options)?;

        // Find the chunk containing "Paragraph text."
        let para_chunk = chunks
            .iter()
            .find(|c| c.text.contains("Paragraph text."))
            .expect("should find paragraph chunk");

        assert_eq!(
            para_chunk.metadata.headings,
            vec!["Chapter", "Section"],
            "heading hierarchy should be [Chapter, Section]"
        );
        Ok(())
    }

    #[test]
    fn include_headings_prepends_context() -> ruddydoc_core::Result<()> {
        let md = "# Top\n\n## Sub\n\nContent here.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions {
            include_headings: true,
            ..Default::default()
        };
        let chunks = chunk_document(&store, &graph, &options)?;

        let content_chunk = chunks
            .iter()
            .find(|c| c.text.contains("Content here."))
            .expect("should find content chunk");

        // The text should start with heading context.
        assert!(
            content_chunk.text.contains("# Top"),
            "chunk text should contain heading prefix"
        );
        assert!(
            content_chunk.text.contains("## Sub"),
            "chunk text should contain sub-heading prefix"
        );
        Ok(())
    }

    #[test]
    fn disable_include_headings() -> ruddydoc_core::Result<()> {
        let md = "# Top\n\nContent.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions {
            include_headings: false,
            ..Default::default()
        };
        let chunks = chunk_document(&store, &graph, &options)?;

        let content_chunk = chunks
            .iter()
            .find(|c| c.text.contains("Content."))
            .expect("should find content chunk");

        // Should NOT have heading prefix in text.
        assert!(
            !content_chunk.text.contains("# Top"),
            "chunk text should not contain heading prefix when disabled"
        );
        Ok(())
    }

    // -------------------------------------------------------------------
    // Source elements
    // -------------------------------------------------------------------

    #[test]
    fn source_elements_contain_iris() -> ruddydoc_core::Result<()> {
        let md = "# Heading\n\nSome text.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        for chunk in &chunks {
            for iri in &chunk.metadata.source_elements {
                assert!(
                    iri.starts_with("urn:ruddydoc:doc:"),
                    "source element IRI should start with urn:ruddydoc:doc:"
                );
            }
        }
        Ok(())
    }

    // -------------------------------------------------------------------
    // List item merging
    // -------------------------------------------------------------------

    #[test]
    fn merge_list_items() -> ruddydoc_core::Result<()> {
        let md = "- Apple\n- Banana\n- Cherry\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions {
            merge_list_items: true,
            ..Default::default()
        };
        let chunks = chunk_document(&store, &graph, &options)?;

        // With merging enabled, the three list items under the same parent
        // should be merged into a single chunk.
        let list_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| c.metadata.element_types.iter().any(|t| t == "ListItem"))
            .collect();

        assert_eq!(
            list_chunks.len(),
            1,
            "merged list items should produce one chunk"
        );
        assert!(list_chunks[0].text.contains("Apple"));
        assert!(list_chunks[0].text.contains("Banana"));
        assert!(list_chunks[0].text.contains("Cherry"));
        assert_eq!(
            list_chunks[0].metadata.source_elements.len(),
            3,
            "should reference all three list items"
        );
        Ok(())
    }

    #[test]
    fn no_merge_list_items() -> ruddydoc_core::Result<()> {
        let md = "- Alpha\n- Beta\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions {
            merge_list_items: false,
            ..Default::default()
        };
        let chunks = chunk_document(&store, &graph, &options)?;

        let list_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| c.metadata.element_types.iter().any(|t| t == "ListItem"))
            .collect();

        assert!(
            list_chunks.len() >= 2,
            "without merging, each list item should be a separate chunk (or merged with adjacent non-list content)"
        );
        Ok(())
    }

    // -------------------------------------------------------------------
    // Token limits and splitting
    // -------------------------------------------------------------------

    #[test]
    fn chunk_respects_max_tokens() -> ruddydoc_core::Result<()> {
        let md = "# Heading\n\nParagraph one.\n\nParagraph two.\n\nParagraph three.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions {
            max_tokens: 512,
            ..Default::default()
        };
        let chunks = chunk_document(&store, &graph, &options)?;

        for chunk in &chunks {
            let tokens = estimate_tokens(&chunk.text, options.chars_per_token);
            // Allow a small margin because contextualisation can add heading text.
            assert!(
                tokens <= options.max_tokens + 20,
                "chunk has {} tokens, exceeds max {}",
                tokens,
                options.max_tokens
            );
        }
        Ok(())
    }

    #[test]
    fn split_long_paragraph() -> ruddydoc_core::Result<()> {
        // Create a paragraph that is much larger than 50 tokens.
        let long_text = "word ".repeat(500); // ~500 words = ~500 tokens at 4 chars/token approx
        let md = format!("{long_text}\n");
        let (store, graph) = parse_md(&md)?;

        let options = ChunkOptions {
            max_tokens: 50,
            overlap_tokens: 10,
            chars_per_token: 4.0,
            ..Default::default()
        };
        let chunks = chunk_document(&store, &graph, &options)?;

        assert!(
            chunks.len() > 1,
            "a very long paragraph should be split into multiple chunks, got {}",
            chunks.len()
        );

        // Each chunk should be approximately within limits (with some
        // tolerance for contextualisation).
        for chunk in &chunks {
            let tokens = estimate_tokens(&chunk.text, options.chars_per_token);
            assert!(
                tokens <= options.max_tokens + 20,
                "split chunk has {} tokens, expected <= {}",
                tokens,
                options.max_tokens + 20
            );
        }
        Ok(())
    }

    // -------------------------------------------------------------------
    // Element types
    // -------------------------------------------------------------------

    #[test]
    fn element_types_populated() -> ruddydoc_core::Result<()> {
        let md = "# Title\n\nA paragraph.\n\n```python\nprint(1)\n```\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        let all_types: Vec<String> = chunks
            .iter()
            .flat_map(|c| c.metadata.element_types.clone())
            .collect();

        assert!(
            all_types.iter().any(|t| t == "SectionHeader"),
            "should contain SectionHeader"
        );
        assert!(
            all_types.iter().any(|t| t == "Paragraph"),
            "should contain Paragraph"
        );
        assert!(all_types.iter().any(|t| t == "Code"), "should contain Code");
        Ok(())
    }

    // -------------------------------------------------------------------
    // Token estimation unit tests
    // -------------------------------------------------------------------

    #[test]
    fn estimate_tokens_basic() {
        // 20 chars / 4.0 = 5 tokens
        assert_eq!(estimate_tokens("hello world 1234567!", 4.0), 5);
    }

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens("", 4.0), 0);
    }

    #[test]
    fn split_text_no_split_needed() {
        let text = "short text";
        let pieces = split_text_with_overlap(text, 100, 10, 4.0);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0], "short text");
    }

    #[test]
    fn split_text_produces_overlap() {
        let text = "a ".repeat(200); // 400 chars = ~100 tokens at 4 c/t
        let pieces = split_text_with_overlap(text.trim(), 30, 5, 4.0);
        assert!(pieces.len() > 1, "should split into multiple pieces");

        // Verify overlap: the end of piece N should overlap with the start of piece N+1.
        for i in 0..pieces.len() - 1 {
            let end_of_current = &pieces[i];
            let start_of_next = &pieces[i + 1];
            // With overlap, the last portion of piece i should appear at
            // the beginning of piece i+1.
            let overlap_region = &end_of_current[end_of_current.len().saturating_sub(10)..];
            assert!(
                start_of_next.contains(overlap_region.trim()),
                "pieces should overlap"
            );
        }
    }

    // -------------------------------------------------------------------
    // Contextualise unit tests
    // -------------------------------------------------------------------

    #[test]
    fn contextualize_with_headings() {
        let options = ChunkOptions {
            include_headings: true,
            ..Default::default()
        };
        let headings = vec!["Chapter".to_string(), "Section".to_string()];
        let result = contextualize_text("Body text.", &headings, &options);
        assert!(result.starts_with("# Chapter\n## Section\n"));
        assert!(result.ends_with("Body text."));
    }

    #[test]
    fn contextualize_without_headings() {
        let options = ChunkOptions {
            include_headings: false,
            ..Default::default()
        };
        let headings = vec!["Chapter".to_string()];
        let result = contextualize_text("Body text.", &headings, &options);
        assert_eq!(result, "Body text.");
    }

    #[test]
    fn contextualize_empty_headings() {
        let options = ChunkOptions {
            include_headings: true,
            ..Default::default()
        };
        let result = contextualize_text("Body text.", &[], &options);
        assert_eq!(result, "Body text.");
    }

    // -------------------------------------------------------------------
    // Heading stack management
    // -------------------------------------------------------------------

    #[test]
    fn heading_stack_resets_on_same_level() -> ruddydoc_core::Result<()> {
        let md = "## A\n\nText A.\n\n## B\n\nText B.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        let text_b = chunks
            .iter()
            .find(|c| c.text.contains("Text B."))
            .expect("should find Text B chunk");

        // When heading B (level 2) appears, heading A should be replaced.
        assert!(
            text_b.metadata.headings.contains(&"B".to_string()),
            "headings should contain B"
        );
        assert!(
            !text_b.metadata.headings.contains(&"A".to_string()),
            "headings should NOT contain A"
        );
        Ok(())
    }

    #[test]
    fn nested_heading_hierarchy() -> ruddydoc_core::Result<()> {
        let md = "# Ch1\n\n## Sec1\n\n### Sub1\n\nDeep content.\n\n## Sec2\n\nShallow content.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        // "Deep content" should have headings [Ch1, Sec1, Sub1]
        let deep = chunks
            .iter()
            .find(|c| c.text.contains("Deep content."))
            .expect("should find deep chunk");
        assert_eq!(deep.metadata.headings.len(), 3);

        // "Shallow content" should have headings [Ch1, Sec2] (Sub1 popped)
        let shallow = chunks
            .iter()
            .find(|c| c.text.contains("Shallow content."))
            .expect("should find shallow chunk");
        assert_eq!(shallow.metadata.headings.len(), 2);
        assert_eq!(shallow.metadata.headings[1], "Sec2");
        Ok(())
    }

    // -------------------------------------------------------------------
    // DocumentChunker trait
    // -------------------------------------------------------------------

    #[test]
    fn hierarchical_chunker_trait_impl() -> ruddydoc_core::Result<()> {
        let md = "# Hello\n\nWorld.\n";
        let (store, graph) = parse_md(md)?;

        let chunker = HierarchicalChunker;
        let options = ChunkOptions::default();
        let chunks = chunker.chunk(&store, &graph, &options)?;

        assert!(!chunks.is_empty());
        Ok(())
    }

    // -------------------------------------------------------------------
    // Table chunking
    // -------------------------------------------------------------------

    #[test]
    fn chunk_document_with_table() -> ruddydoc_core::Result<()> {
        let md = "# Data\n\n| Name | Age |\n|------|-----|\n| Alice | 30 |\n\nAfter table.\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        // Should have chunks for heading, table, and paragraph.
        assert!(
            chunks.len() >= 2,
            "expected multiple chunks, got {}",
            chunks.len()
        );

        // Find the table chunk.
        let table_chunk = chunks
            .iter()
            .find(|c| c.metadata.element_types.iter().any(|t| t == "TableElement"));

        // The table chunk should have table cell text.
        if let Some(tc) = table_chunk {
            assert!(
                tc.text.contains("Alice") || tc.text.contains("Name"),
                "table chunk should contain cell text"
            );
        }

        Ok(())
    }

    // -------------------------------------------------------------------
    // Edge cases
    // -------------------------------------------------------------------

    #[test]
    fn chunk_heading_only_document() -> ruddydoc_core::Result<()> {
        let md = "# Only a Heading\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        assert_eq!(
            chunks.len(),
            1,
            "heading-only document should produce one chunk"
        );
        assert!(chunks[0].text.contains("Only a Heading"));
        Ok(())
    }

    #[test]
    fn chunk_code_block() -> ruddydoc_core::Result<()> {
        let md = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n";
        let (store, graph) = parse_md(md)?;

        let options = ChunkOptions::default();
        let chunks = chunk_document(&store, &graph, &options)?;

        assert!(!chunks.is_empty());
        let code_chunk = chunks
            .iter()
            .find(|c| c.metadata.element_types.contains(&"Code".to_string()))
            .expect("should find code chunk");
        assert!(code_chunk.text.contains("fn main()"));
        Ok(())
    }

    #[test]
    fn local_name_extraction() {
        assert_eq!(
            local_name("https://ruddydoc.chapeaux.io/ontology#Paragraph"),
            "Paragraph"
        );
        assert_eq!(
            local_name("<https://ruddydoc.chapeaux.io/ontology#Code>"),
            "Code"
        );
        assert_eq!(local_name("SomeName"), "SomeName");
    }
}
