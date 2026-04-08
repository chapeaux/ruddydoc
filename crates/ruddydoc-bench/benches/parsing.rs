//! Parsing benchmarks for RuddyDoc backends.
//!
//! Measures the time to parse documents of various formats and sizes
//! through each backend into the Oxigraph store.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use ruddydoc_bench::{
    compute_hash, generate_large_csv, generate_large_html, generate_large_latex,
    generate_large_markdown,
};
use ruddydoc_core::{DocumentBackend, DocumentSource};
use ruddydoc_graph::OxigraphStore;

// ---------------------------------------------------------------------------
// Markdown parsing
// ---------------------------------------------------------------------------

fn bench_markdown_parsing(c: &mut Criterion) {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/sample.md"
    ))
    .expect("failed to read sample.md fixture");

    c.bench_function("parse_markdown_fixture", |b| {
        b.iter(|| {
            let store = OxigraphStore::new().expect("store");
            let backend = ruddydoc_backend_md::MarkdownBackend::new();
            let source = DocumentSource::Stream {
                name: "bench.md".to_string(),
                data: fixture.as_bytes().to_vec(),
            };
            let hash = compute_hash(fixture.as_bytes());
            let doc_graph = ruddydoc_core::doc_iri(&hash);
            backend
                .parse(&source, &store, &doc_graph)
                .expect("parse failed");
        })
    });
}

fn bench_markdown_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown_scaling");

    for size in [100, 500, 1000, 5000, 10000] {
        let content = generate_large_markdown(size);
        let hash = compute_hash(content.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash);

        group.bench_with_input(
            BenchmarkId::new("lines", size),
            &(content, doc_graph),
            |b, (content, doc_graph)| {
                b.iter(|| {
                    let store = OxigraphStore::new().expect("store");
                    let backend = ruddydoc_backend_md::MarkdownBackend::new();
                    let source = DocumentSource::Stream {
                        name: "bench.md".to_string(),
                        data: content.as_bytes().to_vec(),
                    };
                    backend
                        .parse(&source, &store, doc_graph)
                        .expect("parse failed");
                })
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// HTML parsing
// ---------------------------------------------------------------------------

fn bench_html_parsing(c: &mut Criterion) {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/sample.html"
    ))
    .expect("failed to read sample.html fixture");

    c.bench_function("parse_html_fixture", |b| {
        b.iter(|| {
            let store = OxigraphStore::new().expect("store");
            let backend = ruddydoc_backend_html::HtmlBackend;
            let source = DocumentSource::Stream {
                name: "bench.html".to_string(),
                data: fixture.as_bytes().to_vec(),
            };
            let hash = compute_hash(fixture.as_bytes());
            let doc_graph = ruddydoc_core::doc_iri(&hash);
            backend
                .parse(&source, &store, &doc_graph)
                .expect("parse failed");
        })
    });
}

fn bench_html_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_html_scaling");

    for n_elements in [50, 100, 250, 500] {
        let content = generate_large_html(n_elements);
        let hash = compute_hash(content.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash);

        group.bench_with_input(
            BenchmarkId::new("elements", n_elements),
            &(content, doc_graph),
            |b, (content, doc_graph)| {
                b.iter(|| {
                    let store = OxigraphStore::new().expect("store");
                    let backend = ruddydoc_backend_html::HtmlBackend;
                    let source = DocumentSource::Stream {
                        name: "bench.html".to_string(),
                        data: content.as_bytes().to_vec(),
                    };
                    backend
                        .parse(&source, &store, doc_graph)
                        .expect("parse failed");
                })
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// CSV parsing
// ---------------------------------------------------------------------------

fn bench_csv_parsing(c: &mut Criterion) {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/sample.csv"
    ))
    .expect("failed to read sample.csv fixture");

    c.bench_function("parse_csv_fixture", |b| {
        b.iter(|| {
            let store = OxigraphStore::new().expect("store");
            let backend = ruddydoc_backend_csv::CsvBackend;
            let source = DocumentSource::Stream {
                name: "bench.csv".to_string(),
                data: fixture.as_bytes().to_vec(),
            };
            let hash = compute_hash(fixture.as_bytes());
            let doc_graph = ruddydoc_core::doc_iri(&hash);
            backend
                .parse(&source, &store, &doc_graph)
                .expect("parse failed");
        })
    });
}

fn bench_csv_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_csv_scaling");

    for n_rows in [50, 100, 500, 1000] {
        let content = generate_large_csv(n_rows);
        let hash = compute_hash(content.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash);

        group.bench_with_input(
            BenchmarkId::new("rows", n_rows),
            &(content, doc_graph),
            |b, (content, doc_graph)| {
                b.iter(|| {
                    let store = OxigraphStore::new().expect("store");
                    let backend = ruddydoc_backend_csv::CsvBackend;
                    let source = DocumentSource::Stream {
                        name: "bench.csv".to_string(),
                        data: content.as_bytes().to_vec(),
                    };
                    backend
                        .parse(&source, &store, doc_graph)
                        .expect("parse failed");
                })
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// LaTeX parsing
// ---------------------------------------------------------------------------

fn bench_latex_parsing(c: &mut Criterion) {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/sample.tex"
    ))
    .expect("failed to read sample.tex fixture");

    c.bench_function("parse_latex_fixture", |b| {
        b.iter(|| {
            let store = OxigraphStore::new().expect("store");
            let backend = ruddydoc_backend_latex::LatexBackend;
            let source = DocumentSource::Stream {
                name: "bench.tex".to_string(),
                data: fixture.as_bytes().to_vec(),
            };
            let hash = compute_hash(fixture.as_bytes());
            let doc_graph = ruddydoc_core::doc_iri(&hash);
            backend
                .parse(&source, &store, &doc_graph)
                .expect("parse failed");
        })
    });
}

fn bench_latex_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_latex_scaling");

    for size in [100, 500, 1000] {
        let content = generate_large_latex(size);
        let hash = compute_hash(content.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash);

        group.bench_with_input(
            BenchmarkId::new("lines", size),
            &(content, doc_graph),
            |b, (content, doc_graph)| {
                b.iter(|| {
                    let store = OxigraphStore::new().expect("store");
                    let backend = ruddydoc_backend_latex::LatexBackend;
                    let source = DocumentSource::Stream {
                        name: "bench.tex".to_string(),
                        data: content.as_bytes().to_vec(),
                    };
                    backend
                        .parse(&source, &store, doc_graph)
                        .expect("parse failed");
                })
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_markdown_parsing,
    bench_markdown_scaling,
    bench_html_parsing,
    bench_html_scaling,
    bench_csv_parsing,
    bench_csv_scaling,
    bench_latex_parsing,
    bench_latex_scaling,
);
criterion_main!(benches);
