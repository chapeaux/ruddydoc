//! RuddyDoc server: HTTP REST API and MCP tool definitions.
//!
//! This crate provides a combined HTTP REST API (via axum) and MCP tool
//! definitions for AI agent integration with RuddyDoc's document
//! conversion pipeline.
//!
//! # Architecture
//!
//! The server holds an in-memory Oxigraph store shared by all converted
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
        // Write a temp markdown file
        let tmp = std::env::temp_dir().join("ruddydoc_test_convert.md");
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
        let tmp = std::env::temp_dir().join("ruddydoc_test_lifecycle.md");
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

    // -----------------------------------------------------------------
    // MCP tool schemas
    // -----------------------------------------------------------------

    #[test]
    fn test_mcp_tool_schemas_valid() {
        let schemas = mcp::McpToolDefinitions::tool_schemas();
        assert_eq!(schemas.len(), 7);
        for schema in &schemas {
            assert!(schema.get("name").is_some());
            assert!(schema.get("description").is_some());
            assert!(schema.get("inputSchema").is_some());
        }
    }
}
