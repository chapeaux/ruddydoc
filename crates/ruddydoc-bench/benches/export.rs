//! Export benchmarks for RuddyDoc.
//!
//! Measures the time to export a pre-parsed document to each output format.
//! Also benchmarks end-to-end conversion (parse + export) and chunking.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use ruddydoc_bench::{compute_hash, generate_large_markdown};
use ruddydoc_core::{DocumentBackend, DocumentExporter, DocumentSource, OutputFormat};
use ruddydoc_export::{
    ChunkOptions, DocTagsExporter, HtmlExporter, JsonExporter, JsonLdExporter, MarkdownExporter,
    NTriplesExporter, RdfXmlExporter, TextExporter, TurtleExporter, WebVttExporter, chunk_document,
};
use ruddydoc_graph::OxigraphStore;

/// Parse a Markdown string into a store and return (store, doc_graph).
fn setup_store(md: &str) -> (OxigraphStore, String) {
    let store = OxigraphStore::new().expect("store creation failed");
    ruddydoc_ontology::load_ontology(&store).expect("ontology load failed");
    let backend = ruddydoc_backend_md::MarkdownBackend::new();
    let source = DocumentSource::Stream {
        name: "bench.md".to_string(),
        data: md.as_bytes().to_vec(),
    };
    let hash = compute_hash(md.as_bytes());
    let doc_graph = ruddydoc_core::doc_iri(&hash);
    backend
        .parse(&source, &store, &doc_graph)
        .expect("parse failed");
    (store, doc_graph)
}

// ---------------------------------------------------------------------------
// Individual export format benchmarks
// ---------------------------------------------------------------------------

fn bench_export_json(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = JsonExporter;

    c.bench_function("export_json_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

fn bench_export_markdown(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = MarkdownExporter;

    c.bench_function("export_markdown_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

fn bench_export_html(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = HtmlExporter;

    c.bench_function("export_html_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

fn bench_export_text(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = TextExporter;

    c.bench_function("export_text_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

fn bench_export_turtle(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = TurtleExporter;

    c.bench_function("export_turtle_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

fn bench_export_ntriples(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = NTriplesExporter;

    c.bench_function("export_ntriples_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

fn bench_export_jsonld(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = JsonLdExporter;

    c.bench_function("export_jsonld_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

fn bench_export_rdfxml(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = RdfXmlExporter;

    c.bench_function("export_rdfxml_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

fn bench_export_doctags(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = DocTagsExporter;

    c.bench_function("export_doctags_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

fn bench_export_webvtt(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);
    let exporter = WebVttExporter;

    c.bench_function("export_webvtt_500_lines", |b| {
        b.iter(|| {
            exporter.export(&store, &doc_graph).expect("export failed");
        })
    });
}

// ---------------------------------------------------------------------------
// Export format comparison (all formats on the same document)
// ---------------------------------------------------------------------------

fn bench_export_format_comparison(c: &mut Criterion) {
    let md = generate_large_markdown(200);
    let (store, doc_graph) = setup_store(&md);

    let formats: Vec<(&str, Box<dyn DocumentExporter>)> = vec![
        ("json", Box::new(JsonExporter)),
        ("markdown", Box::new(MarkdownExporter)),
        ("html", Box::new(HtmlExporter)),
        ("text", Box::new(TextExporter)),
        ("turtle", Box::new(TurtleExporter)),
        ("ntriples", Box::new(NTriplesExporter)),
        ("jsonld", Box::new(JsonLdExporter)),
        ("rdfxml", Box::new(RdfXmlExporter)),
        ("doctags", Box::new(DocTagsExporter)),
        ("webvtt", Box::new(WebVttExporter)),
    ];

    let mut group = c.benchmark_group("export_format_comparison");

    for (name, exporter) in &formats {
        group.bench_with_input(BenchmarkId::new("format", name), &(), |b, _| {
            b.iter(|| {
                exporter.export(&store, &doc_graph).expect("export failed");
            })
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Chunking benchmarks
// ---------------------------------------------------------------------------

fn bench_chunking(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);

    c.bench_function("chunk_500_lines_default", |b| {
        let options = ChunkOptions::default();
        b.iter(|| {
            chunk_document(&store, &doc_graph, &options).expect("chunking failed");
        })
    });
}

fn bench_chunking_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunk_scaling");

    for size in [100, 500, 1000] {
        let md = generate_large_markdown(size);
        let (store, doc_graph) = setup_store(&md);
        let options = ChunkOptions::default();

        group.bench_with_input(
            BenchmarkId::new("lines", size),
            &(store, doc_graph, options),
            |b, (store, doc_graph, options)| {
                b.iter(|| {
                    chunk_document(store, doc_graph, options).expect("chunking failed");
                })
            },
        );
    }

    group.finish();
}

fn bench_chunking_small_tokens(c: &mut Criterion) {
    let md = generate_large_markdown(500);
    let (store, doc_graph) = setup_store(&md);

    c.bench_function("chunk_500_lines_128_tokens", |b| {
        let options = ChunkOptions {
            max_tokens: 128,
            ..Default::default()
        };
        b.iter(|| {
            chunk_document(&store, &doc_graph, &options).expect("chunking failed");
        })
    });
}

// ---------------------------------------------------------------------------
// End-to-end benchmarks (parse + export)
// ---------------------------------------------------------------------------

fn bench_end_to_end_markdown_to_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_markdown_to_json");

    for size in [100, 500, 1000] {
        let content = generate_large_markdown(size);

        group.bench_with_input(BenchmarkId::new("lines", size), &content, |b, content| {
            b.iter(|| {
                let converter = ruddydoc_converter::DocumentConverter::new(Default::default());
                let source = DocumentSource::Stream {
                    name: "bench.md".to_string(),
                    data: content.as_bytes().to_vec(),
                };
                let result = converter.convert(source).expect("conversion failed");
                let exporter = JsonExporter;
                exporter
                    .export(result.store.as_ref(), &result.doc_graph)
                    .expect("export failed");
            })
        });
    }

    group.finish();
}

fn bench_end_to_end_format_sweep(c: &mut Criterion) {
    let content = generate_large_markdown(200);
    let converter = ruddydoc_converter::DocumentConverter::new(Default::default());

    // Pre-convert once to get the store
    let source = DocumentSource::Stream {
        name: "bench.md".to_string(),
        data: content.as_bytes().to_vec(),
    };
    let result = converter.convert(source).expect("conversion failed");

    let formats: &[(&str, OutputFormat)] = &[
        ("json", OutputFormat::Json),
        ("markdown", OutputFormat::Markdown),
        ("html", OutputFormat::Html),
        ("text", OutputFormat::Text),
        ("turtle", OutputFormat::Turtle),
        ("ntriples", OutputFormat::NTriples),
        ("jsonld", OutputFormat::JsonLd),
        ("doctags", OutputFormat::DocTags),
    ];

    let mut group = c.benchmark_group("e2e_export_format_sweep");

    for (name, format) in formats {
        let exporter = ruddydoc_export::exporter_for(*format).expect("exporter");
        group.bench_with_input(BenchmarkId::new("format", name), &(), |b, _| {
            b.iter(|| {
                exporter
                    .export(result.store.as_ref(), &result.doc_graph)
                    .expect("export failed");
            })
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_export_json,
    bench_export_markdown,
    bench_export_html,
    bench_export_text,
    bench_export_turtle,
    bench_export_ntriples,
    bench_export_jsonld,
    bench_export_rdfxml,
    bench_export_doctags,
    bench_export_webvtt,
    bench_export_format_comparison,
    bench_chunking,
    bench_chunking_scaling,
    bench_chunking_small_tokens,
    bench_end_to_end_markdown_to_json,
    bench_end_to_end_format_sweep,
);
criterion_main!(benches);
