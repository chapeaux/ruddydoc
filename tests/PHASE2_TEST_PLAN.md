# RuddyDoc Phase 2 Test Plan

**Version:** 1.0  
**Date:** 2024-04-07  
**Status:** Ready for Phase 2 Implementation  
**QA Engineer Sign-off:** Pending backend implementation

## Overview

This test plan defines the acceptance criteria, test fixtures, validation queries, and performance baselines for Phase 2 text-based backends: HTML, CSV, LaTeX, WebVTT, AsciiDoc, and XML (JATS, USPTO).

## Test Fixtures

All fixtures are located in `/home/ldary/rh/chapeaux/ruddydoc/tests/fixtures/`.

### Fixture Inventory

| Fixture File | Format | Lines | Description | Key Features Tested |
|-------------|---------|-------|-------------|---------------------|
| `sample.md` | Markdown | 66 | Phase 1 baseline | Headings, paragraphs, lists, code, tables, images, quotes |
| `sample.html` | HTML | 90 | Full HTML page | DOCTYPE, semantic elements, tables with spans, figures, blockquotes |
| `sample.csv` | CSV | 8 | Comma-delimited | Headers, quoted fields, empty cells, mixed data types |
| `tabs.tsv` | TSV | 5 | Tab-delimited | Numeric data, tab separators |
| `semicolons.csv` | CSV | 5 | Semicolon-delimited | European number format (comma decimals), semicolon separator |
| `sample.tex` | LaTeX | 110 | Full LaTeX article | Sections, lists, tables, equations, figures, verbatim, footnotes, refs, comments |
| `sample.vtt` | WebVTT | 35 | WebVTT subtitles | Cues with timestamps, multi-line cues, identifiers, NOTE blocks |
| `sample.adoc` | AsciiDoc | 100 | Full AsciiDoc doc | Sections, lists, tables, code blocks, images, admonitions, quotes |
| `sample_jats.xml` | JATS XML | 130 | Scientific article | Front matter, abstract, sections, tables, figures, references |
| `sample_uspto.xml` | USPTO XML | 160 | Patent document | Bibliographic data, abstract, detailed description, claims |

## Backend Test Matrix

For each backend, verify the following capabilities:

| Backend | Structural Elements | Metadata | Special Features |
|---------|-------------------|----------|------------------|
| **HTML** | Headings (h1-h6), paragraphs, lists (ul/ol), tables (with colspan/rowspan), images, code blocks (pre/code), blockquotes, figures with captions | Title, meta tags, Open Graph | Semantic HTML5 elements (article, section, main, header, footer) |
| **CSV** | Table with cells, header row detection | Delimiter detection (comma/tab/semicolon) | Quoted fields, empty cells, mixed types |
| **LaTeX** | Sections (section/subsection/subsubsection), paragraphs, lists (itemize/enumerate), tables (tabular), figures (figure), equations (equation/math), code (verbatim) | Title, author, date from preamble | Labels/refs, footnotes, comments (ignored), nested lists |
| **WebVTT** | Cues as text elements with timestamps | WEBVTT header | Cue identifiers, multi-line cues, NOTE blocks (ignored) |
| **AsciiDoc** | Sections (=/==/===), paragraphs, lists (*/.), tables, code blocks (with language), images, admonitions, quotes | Document title, author, version | Nested lists, source code with syntax, block delimiters |
| **JATS** | Front matter, abstract, sections, paragraphs, tables (table-wrap), figures (fig) | Authors, affiliations, pub date, DOI, keywords | References (ref-list), citations |
| **USPTO** | Abstract, field of invention, background, summary, detailed description, claims | Inventors, assignees, application number, classifications | Nested claims, hierarchical headings |

## Expected Triple Counts

These are approximate counts to detect gross parsing failures. Exact counts will vary based on implementation details.

| Fixture | Format | Approx. Triples | Key Classes Expected |
|---------|--------|----------------|----------------------|
| `sample.md` | Markdown | 50-100 | Document, SectionHeader (9), Paragraph (3+), ListItem (10+), Code (2), TableElement (1), PictureElement (1) |
| `sample.html` | HTML | 80-150 | Document, SectionHeader (5+), Paragraph (10+), ListItem (7), TableElement (1), PictureElement (2), Code (1) |
| `sample.csv` | CSV | 30-60 | Document, TableElement (1), TableCell (35: 7 rows × 5 cols) |
| `tabs.tsv` | TSV | 20-40 | Document, TableElement (1), TableCell (20: 5 rows × 4 cols) |
| `semicolons.csv` | CSV | 20-40 | Document, TableElement (1), TableCell (20: 5 rows × 4 cols) |
| `sample.tex` | LaTeX | 120-200 | Document, SectionHeader (7+), Paragraph (15+), ListItem (10+), TableElement (1), Formula (2+), Code (1), Footnote (1) |
| `sample.vtt` | WebVTT | 30-60 | Document, TextElement (8 cues), timestamps (rdoc:startTime, rdoc:endTime) |
| `sample.adoc` | AsciiDoc | 100-180 | Document, SectionHeader (6+), Paragraph (12+), ListItem (10+), TableElement (1), Code (2), PictureElement (1) |
| `sample_jats.xml` | JATS | 100-180 | Document, SectionHeader (5+), Paragraph (10+), TableElement (1), PictureElement (1), metadata triples (authors, title, abstract) |
| `sample_uspto.xml` | USPTO | 120-200 | Document, SectionHeader (5+), Paragraph (10+), metadata triples (inventor, assignee, claims) |

## SPARQL Validation Queries

### Universal Queries (all backends)

These queries should work for every backend:

```sparql
# Query 1: Document exists
SELECT ?doc WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?doc a <https://ruddydoc.chapeaux.io/ontology#Document> .
  }
}
# Expected: 1 result

# Query 2: Count all elements
SELECT (COUNT(?elem) AS ?count) WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?elem a ?type .
    FILTER(STRSTARTS(STR(?type), "https://ruddydoc.chapeaux.io/ontology#"))
  }
}
# Expected: count matches approximate triple count above

# Query 3: Verify reading order is sequential
SELECT ?elem ?order WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?elem <https://ruddydoc.chapeaux.io/ontology#readingOrder> ?order .
  }
} ORDER BY ?order
# Expected: ?order values are 0, 1, 2, 3, ... (no gaps, no duplicates)

# Query 4: All text elements have content
SELECT ?elem WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?elem a ?type .
    ?type rdfs:subClassOf* <https://ruddydoc.chapeaux.io/ontology#TextElement> .
    FILTER NOT EXISTS {
      ?elem <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
    }
  }
}
# Expected: 0 results (every TextElement has textContent)
```

### Backend-Specific Validation Queries

#### HTML Backend (`sample.html`)

```sparql
# Verify h1, h2, h3 headings
SELECT ?text ?level WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?h a <https://ruddydoc.chapeaux.io/ontology#SectionHeader> .
    ?h <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
    ?h <https://ruddydoc.chapeaux.io/ontology#headingLevel> ?level .
  }
} ORDER BY ?level
# Expected: 5+ headings with levels 1, 2, 3

# Verify table with colspan/rowspan
SELECT ?row ?col ?rowspan ?colspan WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?cell a <https://ruddydoc.chapeaux.io/ontology#TableCell> .
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellRow> ?row .
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellColumn> ?col .
    OPTIONAL { ?cell <https://ruddydoc.chapeaux.io/ontology#cellRowSpan> ?rowspan . }
    OPTIONAL { ?cell <https://ruddydoc.chapeaux.io/ontology#cellColSpan> ?colspan . }
  }
}
# Expected: at least one cell with rowspan=2, one with colspan=2

# Verify images with alt text
SELECT ?alt WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?img a <https://ruddydoc.chapeaux.io/ontology#PictureElement> .
    ?img <https://ruddydoc.chapeaux.io/ontology#altText> ?alt .
  }
}
# Expected: 2 results (diagram.png and star.svg)
```

#### CSV Backends (`sample.csv`, `tabs.tsv`, `semicolons.csv`)

```sparql
# Verify header row detection
SELECT ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?cell a <https://ruddydoc.chapeaux.io/ontology#TableCell> .
    ?cell <https://ruddydoc.chapeaux.io/ontology#isHeader> true .
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellText> ?text .
  }
} ORDER BY ?text
# Expected for sample.csv: Age, Department, Location, Name, Salary

# Verify quoted field handling (sample.csv)
SELECT ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellText> ?text .
    FILTER(CONTAINS(?text, "Sales, International"))
  }
}
# Expected: 1 result (David Lee's department)

# Verify empty cell handling (sample.csv)
SELECT ?row ?col WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?cell a <https://ruddydoc.chapeaux.io/ontology#TableCell> .
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellRow> ?row .
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellColumn> ?col .
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellText> "" .
  }
}
# Expected: 1 result (Emma Wilson's salary)
```

#### LaTeX Backend (`sample.tex`)

```sparql
# Verify section hierarchy
SELECT ?text ?level WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?h a <https://ruddydoc.chapeaux.io/ontology#SectionHeader> .
    ?h <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
    ?h <https://ruddydoc.chapeaux.io/ontology#headingLevel> ?level .
  }
} ORDER BY ?level
# Expected: section=1, subsection=2, subsubsection=3

# Verify equations
SELECT ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?eq a <https://ruddydoc.chapeaux.io/ontology#Formula> .
    ?eq <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
  }
}
# Expected: at least 2 (inline quadratic formula, E=mc^2)

# Verify footnotes
SELECT ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?fn a <https://ruddydoc.chapeaux.io/ontology#Footnote> .
    ?fn <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
  }
}
# Expected: 1 (footnote explaining something)

# Verify comments are NOT parsed
SELECT ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?elem <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
    FILTER(CONTAINS(?text, "This is a comment"))
  }
}
# Expected: 0 results
```

#### WebVTT Backend (`sample.vtt`)

```sparql
# Verify cues with timestamps
SELECT ?text ?start ?end WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?cue a <https://ruddydoc.chapeaux.io/ontology#TextElement> .
    ?cue <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
    ?cue <https://ruddydoc.chapeaux.io/ontology#startTime> ?start .
    ?cue <https://ruddydoc.chapeaux.io/ontology#endTime> ?end .
  }
} ORDER BY ?start
# Expected: 8 cues with sequential timestamps

# Verify multi-line cues
SELECT ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?cue <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
    FILTER(CONTAINS(?text, "\n"))
  }
}
# Expected: at least 5 (all multi-line cues)

# Verify NOTE blocks are ignored
SELECT ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?elem <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
    FILTER(CONTAINS(?text, "NOTE"))
  }
}
# Expected: 0 results (NOTE blocks are comments)
```

#### AsciiDoc Backend (`sample.adoc`)

```sparql
# Verify heading levels (=, ==, ===, ====)
SELECT ?text ?level WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?h a <https://ruddydoc.chapeaux.io/ontology#SectionHeader> .
    ?h <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
    ?h <https://ruddydoc.chapeaux.io/ontology#headingLevel> ?level .
  }
} ORDER BY ?level
# Expected: level 1-4 headings

# Verify code blocks with language
SELECT ?lang ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?code a <https://ruddydoc.chapeaux.io/ontology#Code> .
    ?code <https://ruddydoc.chapeaux.io/ontology#codeLanguage> ?lang .
    ?code <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
  }
}
# Expected: python, rust

# Verify admonitions
SELECT ?type ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?admon a <https://ruddydoc.chapeaux.io/ontology#Paragraph> .
    ?admon <https://ruddydoc.chapeaux.io/ontology#admonitionType> ?type .
    ?admon <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
  }
}
# Expected: NOTE, WARNING
```

#### JATS Backend (`sample_jats.xml`)

```sparql
# Verify metadata extraction
SELECT ?title ?author WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?doc a <https://ruddydoc.chapeaux.io/ontology#Document> .
    ?doc <http://purl.org/dc/terms/title> ?title .
    ?doc <http://purl.org/dc/terms/creator> ?author .
  }
}
# Expected: title="Semantic Document Parsing...", authors (Smith, Johnson)

# Verify abstract
SELECT ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?abs a <https://ruddydoc.chapeaux.io/ontology#Abstract> .
    ?abs <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
  }
}
# Expected: 1 result (abstract paragraph)

# Verify table in section
SELECT ?caption ?row ?col ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?table a <https://ruddydoc.chapeaux.io/ontology#TableElement> .
    ?table <https://ruddydoc.chapeaux.io/ontology#hasCaption> ?cap .
    ?cap <https://ruddydoc.chapeaux.io/ontology#textContent> ?caption .
    ?table <https://ruddydoc.chapeaux.io/ontology#hasCell> ?cell .
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellRow> ?row .
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellColumn> ?col .
    ?cell <https://ruddydoc.chapeaux.io/ontology#cellText> ?text .
  }
}
# Expected: caption="Performance metrics...", cells with Format, Speed, Accuracy
```

#### USPTO Backend (`sample_uspto.xml`)

```sparql
# Verify patent metadata
SELECT ?title ?inventor ?assignee WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?doc a <https://ruddydoc.chapeaux.io/ontology#Document> .
    ?doc <http://purl.org/dc/terms/title> ?title .
    ?doc <https://ruddydoc.chapeaux.io/ontology#inventor> ?inventor .
    ?doc <https://ruddydoc.chapeaux.io/ontology#assignee> ?assignee .
  }
}
# Expected: title="System and Method...", inventor="Jane Inventor", assignee="RuddyDoc Corporation"

# Verify claims
SELECT ?num ?text WHERE {
  GRAPH <urn:ruddydoc:doc:{hash}> {
    ?claim a <https://ruddydoc.chapeaux.io/ontology#Claim> .
    ?claim <https://ruddydoc.chapeaux.io/ontology#claimNumber> ?num .
    ?claim <https://ruddydoc.chapeaux.io/ontology#textContent> ?text .
  }
} ORDER BY ?num
# Expected: 8 claims numbered 1-8
```

## Round-Trip Tests

For each backend, verify that parsing and re-exporting preserves structural integrity:

### Test Procedure

1. Parse the fixture into the RDF graph
2. Export to JSON (docling-compatible format)
3. Compare the JSON structure against expected elements
4. For text-based formats (Markdown, HTML, LaTeX, AsciiDoc), also export back to the original format
5. Parse the exported document again
6. Compare triple counts and key structural elements

### Acceptance Criteria

- **JSON export**: All major structural elements present (headings, paragraphs, lists, tables, images)
- **Reading order**: Sequential, no gaps or duplicates
- **Text content**: No loss of textual data (whitespace normalization is acceptable)
- **Table structure**: Row/column counts match, spans preserved
- **Metadata**: Title, authors, dates extracted correctly (for formats that have metadata)

### Expected JSON Structure (all backends)

```json
{
  "name": "sample.{ext}",
  "source_format": "{format}",
  "texts": [
    {
      "type": "section_header",
      "heading_level": 1,
      "text": "...",
      "reading_order": 0
    },
    {
      "type": "paragraph",
      "text": "...",
      "reading_order": 1
    }
  ],
  "tables": [
    {
      "cells": [
        {
          "row": 0,
          "col": 0,
          "text": "...",
          "is_header": true
        }
      ]
    }
  ],
  "pictures": [
    {
      "alt_text": "...",
      "reading_order": 5
    }
  ]
}
```

## Performance Baselines

These are targets for Phase 2. Actual performance may vary based on implementation.

| Backend | Fixture | Target Parse Time | Target Memory | Notes |
|---------|---------|------------------|---------------|-------|
| HTML | `sample.html` | <10ms | <2MB | DOM parsing is fast |
| CSV | `sample.csv` | <5ms | <1MB | Simplest format |
| CSV | Large 10K rows | <100ms | <10MB | Streaming parser |
| LaTeX | `sample.tex` | <50ms | <5MB | Custom parser, complex |
| WebVTT | `sample.vtt` | <10ms | <2MB | Simple format |
| AsciiDoc | `sample.adoc` | <30ms | <3MB | Moderate complexity |
| JATS | `sample_jats.xml` | <20ms | <3MB | XML parsing |
| USPTO | `sample_uspto.xml` | <30ms | <4MB | XML parsing, metadata heavy |

### SPARQL Query Performance

All SPARQL queries listed above should complete in <10ms on a standard development machine (no GPU required).

## Test Coverage Requirements

Each backend must achieve:

- **Unit tests**: >80% line coverage for backend-specific code
- **Integration tests**: Round-trip test (parse → export → parse) passes
- **Edge case tests**:
  - Empty document (zero elements)
  - Single element (minimal valid document)
  - Large document (1000+ elements)
  - Malformed input (graceful error handling)
  - Unicode content (emoji, non-Latin scripts)

## Error Handling Tests

For each backend, verify graceful handling of:

| Error Condition | Expected Behavior |
|----------------|-------------------|
| Malformed syntax | Return `ConversionStatus::PartialSuccess` with error details |
| Empty file | Return `ConversionStatus::Success` with zero elements |
| Invalid encoding | Attempt UTF-8 decode, fall back to Latin-1, report encoding issues |
| Circular references (LaTeX \ref) | Detect and break cycles, log warning |
| Missing images/files | Parse document, create PictureElement with placeholder, log warning |
| Extremely large file (>100MB) | Refuse to parse, return `ConversionStatus::Skipped` with size limit error |

## Implementation Checklist

For each backend, the Rust Engineer must:

- [ ] Implement `DocumentBackend` trait
- [ ] Write unit tests for parser components
- [ ] Write integration test using the fixture
- [ ] Verify all SPARQL queries return expected results
- [ ] Achieve >80% code coverage
- [ ] Benchmark parsing time and memory usage
- [ ] Test error conditions and edge cases
- [ ] Document any known limitations in backend crate README

For the QA Engineer (this role):

- [ ] Run all integration tests
- [ ] Verify SPARQL queries against parsed fixtures
- [ ] Validate JSON export structure
- [ ] Test round-trip parsing (where applicable)
- [ ] Verify performance meets baselines
- [ ] Test error handling
- [ ] Sign off on backend before merge

## Phase 2 Acceptance Criteria

Phase 2 is complete when:

1. All 8 backends (HTML, CSV×3, LaTeX, WebVTT, AsciiDoc, JATS, USPTO) pass their integration tests
2. Format auto-detection correctly identifies each format from file extension and content sniffing
3. All SPARQL validation queries return expected results for each fixture
4. Round-trip tests pass (where applicable)
5. Each backend achieves >80% line coverage
6. Performance meets or exceeds baselines
7. Error handling tests pass for all backends
8. CLI command `ruddydoc convert {fixture}` works for all fixtures
9. QA Engineer signs off on this test plan

## Known Limitations and Future Work

### Phase 2 Scope Exclusions

The following are explicitly OUT OF SCOPE for Phase 2:

- **Binary formats**: DOCX, XLSX, PPTX, PDF, EPUB (Phase 3)
- **ML-enhanced parsing**: Layout analysis, OCR, table structure recognition (Phase 4)
- **Advanced export formats**: DocTags, JSON-LD with full schema.org mapping (Phase 5)
- **Performance optimization**: Parallel parsing, streaming large files (Phase 6)

### LaTeX Limitations

- Full macro expansion is not supported; only common macros (\textbf, \emph, etc.)
- Complex math (multi-line align, matrices) may have limited support
- Custom document classes may not parse correctly

### CSV Limitations

- Delimiter auto-detection may fail on ambiguous files (use explicit format option)
- Very large CSV files (>1M rows) may cause memory issues (streaming parser in Phase 6)

### XML Limitations

- Only JATS and USPTO schemas are supported; generic XML is not parsed
- XBRL (financial reporting XML) is deferred to Phase 3

## Revision History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2024-04-07 | QA Engineer | Initial test plan for Phase 2 |

---

**QA Engineer Certification:**

I certify that this test plan comprehensively covers Phase 2 acceptance criteria as defined in `/home/ldary/rh/chapeaux/ruddydoc/INITIAL_PLAN.md`. The test fixtures exercise all key structural elements for each format. The SPARQL validation queries verify correct ontology mapping. Performance baselines are achievable with well-optimized parsers.

This test plan is ready for Phase 2 implementation.

**Status:** ✅ Ready for Rust Engineers  
**Sign-off:** Pending Phase 2 completion
