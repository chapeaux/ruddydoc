//! RuddyDoc server: HTTP REST API and MCP tool definitions.
//!
//! This crate provides a combined HTTP REST API (via axum) and MCP tool
//! definitions for AI agent integration with RuddyDoc's document
//! conversion pipeline.
//!
//! # Architecture
//!
//! The server holds an in-memory Sparq store shared by all converted
//! documents. Each document lives in its own named graph. The
//! [`state::ServerState`] struct manages the store, converter, and
//! document registry.
//!
//! - [`http`] -- axum REST API handlers
//! - [`mcp`] -- MCP tool schema definitions (protocol wiring is TODO)
//! - [`state`] -- shared server state and document lifecycle

pub mod http;
pub mod mcp;
pub mod state;

use std::sync::Arc;

use state::ServerState;

/// Start the HTTP REST server on the given port.
///
/// This blocks until the server is shut down.
pub async fn start_http_server(port: u16) -> ruddydoc_core::Result<()> {
    let state = Arc::new(ServerState::new()?);
    let app = http::router(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!(port = port, "RuddyDoc server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Start the MCP server on stdio (stdin/stdout).
///
/// This blocks until stdin is closed.
pub async fn start_mcp_stdio() -> ruddydoc_core::Result<()> {
    mcp::run_stdio().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Helper to create a test app with fresh state.
    fn test_app() -> axum::Router {
        let state = Arc::new(ServerState::new().expect("failed to create server state"));
        http::router(state)
    }

    /// Helper to send a request and get back (status, body_string).
    async fn send_request(app: axum::Router, request: Request<Body>) -> (StatusCode, String) {
        let response = app.oneshot(request).await.expect("request failed");
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes();
        let body_str = String::from_utf8_lossy(&body).to_string();
        (status, body_str)
    }

    // -----------------------------------------------------------------
    // Health endpoint
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = test_app();
        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert!(json.get("version").is_some());
    }

    // -----------------------------------------------------------------
    // Formats endpoint
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_formats_endpoint() {
        let app = test_app();
        let request = Request::builder()
            .uri("/formats")
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json.get("input_formats").is_some());
        assert!(json.get("output_formats").is_some());

        let input = json["input_formats"].as_array().unwrap();
        assert!(!input.is_empty());

        let output = json["output_formats"].as_array().unwrap();
        assert!(!output.is_empty());
    }

    // -----------------------------------------------------------------
    // Convert endpoint
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_convert_markdown_file() {
        // Write a temp markdown file with a unique name to avoid races
        let tmp =
            std::env::temp_dir().join(format!("ruddydoc_test_convert_{}.md", std::process::id()));
        std::fs::write(&tmp, "# Test\n\nHello world.\n").unwrap();

        let app = test_app();
        let body_json = serde_json::json!({ "source": tmp.to_string_lossy() });
        let request = Request::builder()
            .method("POST")
            .uri("/convert")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body_json).unwrap()))
            .unwrap();

        let (status, body) = send_request(app, request).await;

        // Clean up
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(status, StatusCode::CREATED, "body: {body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json.get("id").is_some());
        assert_eq!(json["format"], "Markdown");
    }

    #[tokio::test]
    async fn test_convert_empty_source() {
        let app = test_app();
        let body_json = serde_json::json!({ "source": "" });
        let request = Request::builder()
            .method("POST")
            .uri("/convert")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body_json).unwrap()))
            .unwrap();

        let (status, _body) = send_request(app, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // -----------------------------------------------------------------
    // Document lifecycle: convert -> get -> export -> query -> elements -> chunks
    // -----------------------------------------------------------------

    /// Helper: convert a document and return its ID using a shared state.
    async fn convert_test_doc(state: &Arc<ServerState>) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!(
            "ruddydoc_test_lifecycle_{}_{n}.md",
            std::process::id()
        ));
        std::fs::write(
            &tmp,
            "# Document Title\n\nFirst paragraph.\n\n## Section\n\nSecond paragraph.\n\n- Item A\n- Item B\n",
        )
        .unwrap();

        let record = state
            .convert_file(tmp.to_string_lossy().as_ref())
            .await
            .expect("conversion failed");

        let _ = std::fs::remove_file(&tmp);
        record.id
    }

    #[tokio::test]
    async fn test_list_documents() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let app = http::router(Arc::clone(&state));
        let request = Request::builder()
            .uri("/documents")
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        let docs = json["documents"].as_array().unwrap();
        assert!(!docs.is_empty());
        assert!(docs.iter().any(|d| d["id"] == doc_id));
    }

    #[tokio::test]
    async fn test_get_document() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let app = http::router(Arc::clone(&state));
        let request = Request::builder()
            .uri(format!("/documents/{doc_id}"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["id"], doc_id);
        assert_eq!(json["format"], "Markdown");
    }

    #[tokio::test]
    async fn test_get_document_not_found() {
        let app = test_app();
        let request = Request::builder()
            .uri("/documents/nonexistent-id")
            .body(Body::empty())
            .unwrap();

        let (status, _body) = send_request(app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_export_json() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let app = http::router(Arc::clone(&state));
        let request = Request::builder()
            .uri(format!("/documents/{doc_id}/export?format=json"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["format"], "json");
        assert!(json.get("content").is_some());
    }

    #[tokio::test]
    async fn test_export_turtle() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let app = http::router(Arc::clone(&state));
        let request = Request::builder()
            .uri(format!("/documents/{doc_id}/export?format=turtle"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["format"], "turtle");
        let content = json["content"].as_str().unwrap();
        assert!(content.contains("ruddydoc"));
    }

    #[tokio::test]
    async fn test_query_document() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let app = http::router(Arc::clone(&state));
        let body_json = serde_json::json!({
            "sparql": "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 5"
        });
        let request = Request::builder()
            .method("POST")
            .uri(format!("/documents/{doc_id}/query"))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body_json).unwrap()))
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json.get("results").is_some());
    }

    #[tokio::test]
    async fn test_introspect_list_classes_and_prefixes() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let introspection = state.introspect_document(&doc_id).await.unwrap();
        assert!(introspection["triples"].as_u64().unwrap() > 0);

        let classes = state.list_classes(&doc_id).await.unwrap();
        let classes = classes.as_array().unwrap();
        assert!(!classes.is_empty());
        // The test fixture has headings, paragraphs, and a list -- confirm at
        // least one recognizable rdoc class shows up, not just "some class".
        assert!(
            classes
                .iter()
                .any(|c| c["class"].as_str().unwrap_or("").contains("Paragraph"))
        );

        let prefixes = state.list_prefixes(&doc_id).await.unwrap();
        let prefixes = prefixes.as_array().unwrap();
        assert!(!prefixes.is_empty());
    }

    #[tokio::test]
    async fn test_convert_attaches_and_revalidates_shacl_report() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        // convert_file's initial report, carried on the DocumentRecord.
        let docs = state.documents.read().await;
        let record = docs.get(&doc_id).unwrap().clone();
        drop(docs);
        let initial = record.validation.expect("expected a validation report");
        assert_eq!(initial["conforms"], true, "report: {initial}");

        // validate_document re-runs it on demand and agrees.
        let revalidated = state.validate_document(&doc_id).await.unwrap();
        assert_eq!(revalidated["conforms"], true, "report: {revalidated}");
    }

    #[tokio::test]
    async fn test_search_text() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        // Fixture text includes "First paragraph." / "Second paragraph."
        let hits = state.search_text(&doc_id, "paragraph", 10).await.unwrap();
        let hits = hits.as_array().unwrap();
        assert_eq!(hits.len(), 2, "hits: {hits:?}");
        assert!(hits.iter().all(|h| h["text"].as_str().unwrap_or("").contains("paragraph")));

        let no_hits = state
            .search_text(&doc_id, "nonexistentterm", 10)
            .await
            .unwrap();
        assert!(no_hits.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_canonicalize_document() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let hash = state.canonicalize_document(&doc_id).await.unwrap();
        assert_eq!(hash.len(), 64, "expected a hex SHA-256 digest: {hash}");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

        // Stable across repeated calls against an unchanged graph.
        let hash_again = state.canonicalize_document(&doc_id).await.unwrap();
        assert_eq!(hash, hash_again);
    }

    /// Deterministic fake embedder for tests: no network calls, and
    /// identical text always produces an identical vector (so a query using
    /// a chunk's own exact text should retrieve it with similarity ~1.0).
    struct FakeEmbeddingProvider;

    impl ruddydoc_models::EmbeddingProvider for FakeEmbeddingProvider {
        fn embed(&self, texts: &[String]) -> ruddydoc_core::Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|text| {
                    let mut v = vec![0.0f32; 4];
                    for (i, b) in text.bytes().enumerate() {
                        v[i % 4] += b as f32;
                    }
                    v
                })
                .collect())
        }
    }

    fn state_with_fake_embedder() -> Arc<ServerState> {
        let mut state = ServerState::new().unwrap();
        state.embedding_provider = Some(Arc::new(FakeEmbeddingProvider));
        Arc::new(state)
    }

    #[tokio::test]
    async fn test_embed_document_requires_configured_provider() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;
        // No RUDDYDOC_EMBEDDING_URL set in the test environment.
        let result = state.embed_document(&doc_id, 512).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_embed_and_semantic_search() {
        let state = state_with_fake_embedder();
        let doc_id = convert_test_doc(&state).await;

        let count = state.embed_document(&doc_id, 512).await.unwrap();
        assert!(count > 0);

        // Fetch an actual chunk's exact indexed text via SPARQL, rather than
        // assuming the fixture's exact chunking/heading-breadcrumb shape.
        let ont = ruddydoc_ontology::NAMESPACE;
        let sparql = format!("SELECT ?text WHERE {{ ?c <{ont}chunkText> ?text }} LIMIT 1");
        let rows = state.query_document(&doc_id, &sparql).await.unwrap();
        let raw_text = rows[0]["text"].as_str().unwrap();
        // query_to_json renders literals as `"value"` -- strip the quotes to
        // get back the exact chunk text for the semantic_search query.
        let chunk_text = raw_text.trim_start_matches('"').trim_end_matches('"');

        // Querying with a chunk's own exact text should retrieve that exact
        // chunk first (identical text -> identical fake-embedded vector ->
        // cosine similarity 1.0, the maximum possible).
        let results = state.semantic_search(&doc_id, chunk_text, 3).await.unwrap();
        let results = results.as_array().unwrap();
        assert!(!results.is_empty());
        assert!(results[0]["score"].as_f64().unwrap() > 0.99, "results: {results:?}");
    }

    #[tokio::test]
    async fn test_semantic_search_requires_configured_provider() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;
        let result = state.semantic_search(&doc_id, "anything", 3).await;
        assert!(result.is_err());
    }

    /// Scripted fake LLM for tests: always returns a fixed, valid SPARQL
    /// query -- no network calls, no repair round needed.
    struct FakeLlmProvider;

    impl ruddydoc_models::LlmProvider for FakeLlmProvider {
        fn complete(&self, _prompt: &str) -> ruddydoc_core::Result<String> {
            let ont = ruddydoc_ontology::NAMESPACE;
            Ok(format!("SELECT ?p WHERE {{ ?p a <{ont}Paragraph> }}"))
        }
    }

    fn state_with_fake_llm() -> Arc<ServerState> {
        let mut state = ServerState::new().unwrap();
        state.llm_provider = Some(Arc::new(FakeLlmProvider));
        Arc::new(state)
    }

    #[tokio::test]
    async fn test_ask_document_requires_configured_provider() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;
        let result = state.ask_document(&doc_id, "What paragraphs are there?").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ask_document_happy_path() {
        let state = state_with_fake_llm();
        let doc_id = convert_test_doc(&state).await;

        // Fixture has "First paragraph." and "Second paragraph." (2 rdoc:Paragraph elements).
        let answer = state
            .ask_document(&doc_id, "What paragraphs are there?")
            .await
            .unwrap();
        assert_eq!(answer["repairs"], 0, "answer: {answer}");
        let rows = answer["result"].as_array().expect("expected array");
        assert_eq!(rows.len(), 2, "answer: {answer}");
    }

    #[tokio::test]
    async fn test_list_elements() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let app = http::router(Arc::clone(&state));
        let request = Request::builder()
            .uri(format!("/documents/{doc_id}/elements"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        let elements = json["elements"].as_array().unwrap();
        assert!(!elements.is_empty());
    }

    #[tokio::test]
    async fn test_list_elements_filtered() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let app = http::router(Arc::clone(&state));
        let request = Request::builder()
            .uri(format!("/documents/{doc_id}/elements?type=Paragraph"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        let elements = json["elements"].as_array().unwrap();
        // All returned elements should be paragraphs
        for el in elements {
            let type_str = el["type"].as_str().unwrap_or("");
            assert!(
                type_str.contains("Paragraph"),
                "expected Paragraph type, got: {type_str}"
            );
        }
    }

    #[tokio::test]
    async fn test_chunk_document() {
        let state = Arc::new(ServerState::new().unwrap());
        let doc_id = convert_test_doc(&state).await;

        let app = http::router(Arc::clone(&state));
        let request = Request::builder()
            .uri(format!("/documents/{doc_id}/chunks?max_tokens=512"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, request).await;

        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        let chunks = json["chunks"].as_array().unwrap();
        assert!(!chunks.is_empty());
        assert!(json.get("count").is_some());
        assert_eq!(json["max_tokens"], 512);
    }

    // -----------------------------------------------------------------
    // 404 tests for unknown document IDs
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_export_not_found() {
        let app = test_app();
        let request = Request::builder()
            .uri("/documents/nonexistent/export?format=json")
            .body(Body::empty())
            .unwrap();

        let (status, _body) = send_request(app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_query_not_found() {
        let app = test_app();
        let body_json = serde_json::json!({ "sparql": "SELECT ?s WHERE { ?s ?p ?o }" });
        let request = Request::builder()
            .method("POST")
            .uri("/documents/nonexistent/query")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body_json).unwrap()))
            .unwrap();

        let (status, _body) = send_request(app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_elements_not_found() {
        let app = test_app();
        let request = Request::builder()
            .uri("/documents/nonexistent/elements")
            .body(Body::empty())
            .unwrap();

        let (status, _body) = send_request(app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_chunks_not_found() {
        let app = test_app();
        let request = Request::builder()
            .uri("/documents/nonexistent/chunks")
            .body(Body::empty())
            .unwrap();

        let (status, _body) = send_request(app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // MCP tool schema coverage lives in mcp.rs's own test module
    // (tool_schemas_are_valid_json, tool_schemas_have_expected_tools) --
    // mcp's internals are private to that module, so there's no public
    // entry point left here to duplicate that coverage against.
}
