//! Graph (Oxigraph) benchmarks for RuddyDoc.
//!
//! Measures the performance of the underlying Oxigraph store operations:
//! triple insertion, SPARQL queries, serialization, and graph management.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use ruddydoc_bench::{compute_hash, generate_large_markdown};
use ruddydoc_core::{DocumentBackend, DocumentSource, DocumentStore};
use ruddydoc_graph::OxigraphStore;
use ruddydoc_ontology as ont;

// ---------------------------------------------------------------------------
// Triple insertion
// ---------------------------------------------------------------------------

fn bench_triple_insertion(c: &mut Criterion) {
    let mut group = c.benchmark_group("triple_insertion");

    for count in [100, 500, 1000, 5000] {
        group.bench_with_input(BenchmarkId::new("triples", count), &count, |b, &count| {
            b.iter(|| {
                let store = OxigraphStore::new().expect("store");
                let graph = "urn:ruddydoc:doc:bench";
                let rdf_type = ont::rdf_iri("type");
                let class = ont::iri(ont::CLASS_PARAGRAPH);

                for i in 0..count {
                    store
                        .insert_triple_into(
                            &format!("urn:ruddydoc:doc:bench/el-{i}"),
                            &rdf_type,
                            &class,
                            graph,
                        )
                        .expect("insert failed");
                }
            })
        });
    }

    group.finish();
}

fn bench_literal_insertion(c: &mut Criterion) {
    let mut group = c.benchmark_group("literal_insertion");

    for count in [100, 500, 1000] {
        group.bench_with_input(BenchmarkId::new("literals", count), &count, |b, &count| {
            b.iter(|| {
                let store = OxigraphStore::new().expect("store");
                let graph = "urn:ruddydoc:doc:bench";
                let prop = ont::iri(ont::PROP_TEXT_CONTENT);

                for i in 0..count {
                    store
                        .insert_literal(
                            &format!("urn:ruddydoc:doc:bench/el-{i}"),
                            &prop,
                            &format!("Text content for element {i} in the benchmark document"),
                            "string",
                            graph,
                        )
                        .expect("insert failed");
                }
            })
        });
    }

    group.finish();
}

fn bench_mixed_insertion(c: &mut Criterion) {
    c.bench_function("insert_realistic_element_1000", |b| {
        b.iter(|| {
            let store = OxigraphStore::new().expect("store");
            let graph = "urn:ruddydoc:doc:bench";
            let rdf_type = ont::rdf_iri("type");
            let doc_iri = "urn:ruddydoc:doc:bench/doc";

            // Insert document node
            store
                .insert_triple_into(doc_iri, &rdf_type, &ont::iri(ont::CLASS_DOCUMENT), graph)
                .expect("insert failed");

            for i in 0..1000 {
                let el_iri = format!("urn:ruddydoc:doc:bench/el-{i}");

                // Type triple
                store
                    .insert_triple_into(&el_iri, &rdf_type, &ont::iri(ont::CLASS_PARAGRAPH), graph)
                    .expect("insert failed");

                // hasElement link
                store
                    .insert_triple_into(doc_iri, &ont::iri(ont::PROP_HAS_ELEMENT), &el_iri, graph)
                    .expect("insert failed");

                // Text content
                store
                    .insert_literal(
                        &el_iri,
                        &ont::iri(ont::PROP_TEXT_CONTENT),
                        &format!("Paragraph {i} content in the benchmark document"),
                        "string",
                        graph,
                    )
                    .expect("insert failed");

                // Reading order
                store
                    .insert_literal(
                        &el_iri,
                        &ont::iri(ont::PROP_READING_ORDER),
                        &i.to_string(),
                        "integer",
                        graph,
                    )
                    .expect("insert failed");
            }
        })
    });
}

// ---------------------------------------------------------------------------
// SPARQL query
// ---------------------------------------------------------------------------

/// Create a pre-populated store with `n` elements for query benchmarks.
fn populate_store(n: usize) -> (OxigraphStore, String) {
    let store = OxigraphStore::new().expect("store");
    let graph = "urn:ruddydoc:doc:query_bench";
    let rdf_type = ont::rdf_iri("type");
    let doc_iri = "urn:ruddydoc:doc:query_bench/doc";

    store
        .insert_triple_into(doc_iri, &rdf_type, &ont::iri(ont::CLASS_DOCUMENT), graph)
        .expect("insert failed");

    let classes = [
        ont::CLASS_PARAGRAPH,
        ont::CLASS_SECTION_HEADER,
        ont::CLASS_LIST_ITEM,
        ont::CLASS_CODE,
    ];

    for i in 0..n {
        let el_iri = format!("urn:ruddydoc:doc:query_bench/el-{i}");
        let class = classes[i % classes.len()];

        store
            .insert_triple_into(&el_iri, &rdf_type, &ont::iri(class), graph)
            .expect("insert failed");
        store
            .insert_triple_into(doc_iri, &ont::iri(ont::PROP_HAS_ELEMENT), &el_iri, graph)
            .expect("insert failed");
        store
            .insert_literal(
                &el_iri,
                &ont::iri(ont::PROP_TEXT_CONTENT),
                &format!("Content for element {i}"),
                "string",
                graph,
            )
            .expect("insert failed");
        store
            .insert_literal(
                &el_iri,
                &ont::iri(ont::PROP_READING_ORDER),
                &i.to_string(),
                "integer",
                graph,
            )
            .expect("insert failed");

        if class == ont::CLASS_SECTION_HEADER {
            store
                .insert_literal(
                    &el_iri,
                    &ont::iri(ont::PROP_HEADING_LEVEL),
                    &((i % 3 + 1).to_string()),
                    "integer",
                    graph,
                )
                .expect("insert failed");
        }
    }

    (store, graph.to_string())
}

fn bench_sparql_select_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("sparql_select_all");

    for n in [100, 500, 1000] {
        let (store, graph) = populate_store(n);
        let sparql = format!("SELECT ?s ?p ?o WHERE {{ GRAPH <{graph}> {{ ?s ?p ?o }} }}");

        group.bench_with_input(
            BenchmarkId::new("triples_from", n),
            &(store, sparql),
            |b, (store, sparql)| {
                b.iter(|| {
                    store.query_to_json(sparql).expect("query failed");
                })
            },
        );
    }

    group.finish();
}

fn bench_sparql_select_typed(c: &mut Criterion) {
    let (store, graph) = populate_store(1000);

    let classes = [
        ("Paragraph", ont::CLASS_PARAGRAPH),
        ("SectionHeader", ont::CLASS_SECTION_HEADER),
        ("ListItem", ont::CLASS_LIST_ITEM),
        ("Code", ont::CLASS_CODE),
    ];

    let mut group = c.benchmark_group("sparql_select_by_type");

    for (name, class) in &classes {
        let sparql = format!(
            "SELECT ?el ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el a <{class_iri}>. \
                 ?el <{text_prop}> ?text \
               }} \
             }}",
            class_iri = ont::iri(class),
            text_prop = ont::iri(ont::PROP_TEXT_CONTENT),
        );

        group.bench_with_input(
            BenchmarkId::new("class", name),
            &(store.query_to_json("ASK { ?s ?p ?o }"), &sparql),
            |b, (_, sparql)| {
                b.iter(|| {
                    store.query_to_json(sparql).expect("query failed");
                })
            },
        );
    }

    group.finish();
}

fn bench_sparql_ordered_elements(c: &mut Criterion) {
    let (store, graph) = populate_store(1000);

    let sparql = format!(
        "SELECT ?el ?type ?text ?order WHERE {{ \
           GRAPH <{graph}> {{ \
             ?el a ?type. \
             ?el <{text}> ?text. \
             ?el <{order}> ?order \
           }} \
         }} ORDER BY ?order",
        text = ont::iri(ont::PROP_TEXT_CONTENT),
        order = ont::iri(ont::PROP_READING_ORDER),
    );

    c.bench_function("sparql_ordered_1000_elements", |b| {
        b.iter(|| {
            store.query_to_json(&sparql).expect("query failed");
        })
    });
}

fn bench_sparql_ask(c: &mut Criterion) {
    let (store, graph) = populate_store(1000);

    let sparql_exists = format!(
        "ASK {{ GRAPH <{graph}> {{ ?el a <{}> }} }}",
        ont::iri(ont::CLASS_PARAGRAPH),
    );
    let sparql_not_exists = format!(
        "ASK {{ GRAPH <{graph}> {{ ?el a <{}> }} }}",
        ont::iri(ont::CLASS_FOOTNOTE),
    );

    let mut group = c.benchmark_group("sparql_ask");

    group.bench_function("exists", |b| {
        b.iter(|| {
            store.query_to_json(&sparql_exists).expect("query failed");
        })
    });

    group.bench_function("not_exists", |b| {
        b.iter(|| {
            store
                .query_to_json(&sparql_not_exists)
                .expect("query failed");
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Graph serialization
// ---------------------------------------------------------------------------

fn bench_serialize_turtle(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialize_turtle");

    for n in [100, 500, 1000] {
        let (store, graph) = populate_store(n);

        group.bench_with_input(
            BenchmarkId::new("elements", n),
            &(store, graph),
            |b, (store, graph)| {
                b.iter(|| {
                    store
                        .serialize_graph(graph, "turtle")
                        .expect("serialize failed");
                })
            },
        );
    }

    group.finish();
}

fn bench_serialize_ntriples(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialize_ntriples");

    for n in [100, 500, 1000] {
        let (store, graph) = populate_store(n);

        group.bench_with_input(
            BenchmarkId::new("elements", n),
            &(store, graph),
            |b, (store, graph)| {
                b.iter(|| {
                    store
                        .serialize_graph(graph, "ntriples")
                        .expect("serialize failed");
                })
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Graph management
// ---------------------------------------------------------------------------

fn bench_triple_count(c: &mut Criterion) {
    let (store, graph) = populate_store(1000);

    let mut group = c.benchmark_group("graph_management");

    group.bench_function("triple_count_total", |b| {
        b.iter(|| {
            store.triple_count().expect("count failed");
        })
    });

    group.bench_function("triple_count_in_graph", |b| {
        b.iter(|| {
            store.triple_count_in(&graph).expect("count failed");
        })
    });

    group.finish();
}

fn bench_clear_graph(c: &mut Criterion) {
    c.bench_function("clear_graph_1000_elements", |b| {
        b.iter_with_setup(
            || populate_store(1000),
            |(store, graph)| {
                store.clear_graph(&graph).expect("clear failed");
            },
        );
    });
}

// ---------------------------------------------------------------------------
// End-to-end: realistic document through graph
// ---------------------------------------------------------------------------

fn bench_full_graph_pipeline(c: &mut Criterion) {
    let md = generate_large_markdown(500);

    c.bench_function("full_parse_query_serialize_500_lines", |b| {
        b.iter(|| {
            let store = OxigraphStore::new().expect("store");
            let backend = ruddydoc_backend_md::MarkdownBackend::new();
            let source = DocumentSource::Stream {
                name: "bench.md".to_string(),
                data: md.as_bytes().to_vec(),
            };
            let hash = compute_hash(md.as_bytes());
            let doc_graph = ruddydoc_core::doc_iri(&hash);

            // Parse
            backend
                .parse(&source, &store, &doc_graph)
                .expect("parse failed");

            // Query all elements
            let sparql =
                format!("SELECT ?el ?type WHERE {{ GRAPH <{doc_graph}> {{ ?el a ?type }} }}");
            store.query_to_json(&sparql).expect("query failed");

            // Serialize
            store
                .serialize_graph(&doc_graph, "turtle")
                .expect("serialize failed");
        })
    });
}

// ---------------------------------------------------------------------------
// Criterion harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_triple_insertion,
    bench_literal_insertion,
    bench_mixed_insertion,
    bench_sparql_select_all,
    bench_sparql_select_typed,
    bench_sparql_ordered_elements,
    bench_sparql_ask,
    bench_serialize_turtle,
    bench_serialize_ntriples,
    bench_triple_count,
    bench_clear_graph,
    bench_full_graph_pipeline,
);
criterion_main!(benches);
