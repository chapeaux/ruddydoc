//! HTTP REST API handlers for the RuddyDoc server.
//!
//! Provides axum route handlers for document conversion, export, querying,
//! element listing, and chunking.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use tracing::error;

use crate::state::ServerState;

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the axum router with all REST endpoints.
pub fn router(state: Arc<ServerState>) -> axum::Router {
    axum::Router::new()
        .route("/health", get(health))
        .route("/formats", get(list_formats))
        .route("/convert", post(convert_document))
        .route("/documents", get(list_documents))
        .route("/documents/{id}", get(get_document))
        .route("/documents/{id}/export", get(export_document))
        .route("/documents/{id}/query", post(query_document))
        .route("/documents/{id}/elements", get(list_elements))
        .route("/documents/{id}/chunks", get(chunk_document))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Request body for POST /convert.
#[derive(Debug, Deserialize)]
pub struct ConvertRequest {
    /// Path to the file to convert.
    pub source: String,
}

/// Query parameters for GET /documents/{id}/export.
#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    /// Output format (default: "json").
    pub format: Option<String>,
}

/// Request body for POST /documents/{id}/query.
#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    /// SPARQL query to execute.
    pub sparql: String,
}

/// Query parameters for GET /documents/{id}/elements.
#[derive(Debug, Deserialize)]
pub struct ElementsQuery {
    /// Filter by element type (e.g., "Paragraph", "SectionHeader").
    #[serde(rename = "type")]
    pub element_type: Option<String>,
}

/// Query parameters for GET /documents/{id}/chunks.
#[derive(Debug, Deserialize)]
pub struct ChunksQuery {
    /// Maximum tokens per chunk (default: 512).
    pub max_tokens: Option<usize>,
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

/// Application-level error type for HTTP responses.
pub struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }

    fn not_found(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: msg.into(),
        }
    }

    fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.message });
        (self.status, Json(body)).into_response()
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /health -- health check endpoint.
async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// GET /formats -- list supported input and output formats.
async fn list_formats() -> impl IntoResponse {
    let input_formats: Vec<serde_json::Value> = ruddydoc_converter::list_supported_formats()
        .iter()
        .map(|f| {
            serde_json::json!({
                "format": f.format.to_string(),
                "extensions": f.extensions,
                "mime_type": f.mime_type,
            })
        })
        .collect();

    let output_formats = vec![
        serde_json::json!({ "format": "JSON", "id": "json" }),
        serde_json::json!({ "format": "Markdown", "id": "markdown" }),
        serde_json::json!({ "format": "HTML", "id": "html" }),
        serde_json::json!({ "format": "Text", "id": "text" }),
        serde_json::json!({ "format": "Turtle", "id": "turtle" }),
        serde_json::json!({ "format": "N-Triples", "id": "ntriples" }),
    ];

    Json(serde_json::json!({
        "input_formats": input_formats,
        "output_formats": output_formats,
    }))
}

/// POST /convert -- convert a document file.
async fn convert_document(
    State(state): State<Arc<ServerState>>,
    Json(body): Json<ConvertRequest>,
) -> std::result::Result<impl IntoResponse, AppError> {
    if body.source.is_empty() {
        return Err(AppError::bad_request("'source' field is required"));
    }

    let record = state.convert_file(&body.source).await.map_err(|e| {
        error!(error = %e, source = %body.source, "conversion failed");
        AppError::internal(e.to_string())
    })?;

    let response = serde_json::json!({
        "id": record.id,
        "format": record.meta.format.to_string(),
        "file_size": record.meta.file_size,
        "page_count": record.meta.page_count,
        "hash": record.meta.hash.0,
        "graph_iri": record.graph_iri,
    });

    Ok((StatusCode::CREATED, Json(response)))
}

/// GET /documents -- list all converted documents.
async fn list_documents(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let docs = state.documents.read().await;
    let documents: Vec<serde_json::Value> = docs
        .values()
        .map(|record| {
            serde_json::json!({
                "id": record.id,
                "format": record.meta.format.to_string(),
                "file_size": record.meta.file_size,
                "page_count": record.meta.page_count,
                "hash": record.meta.hash.0,
                "graph_iri": record.graph_iri,
            })
        })
        .collect();

    Json(serde_json::json!({ "documents": documents }))
}

/// GET /documents/{id} -- get document metadata.
async fn get_document(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, AppError> {
    let docs = state.documents.read().await;
    let record = docs
        .get(&id)
        .ok_or_else(|| AppError::not_found(format!("document '{id}' not found")))?;

    let response = serde_json::json!({
        "id": record.id,
        "format": record.meta.format.to_string(),
        "file_size": record.meta.file_size,
        "page_count": record.meta.page_count,
        "hash": record.meta.hash.0,
        "graph_iri": record.graph_iri,
        "file_path": record.meta.file_path,
    });

    Ok(Json(response))
}

/// GET /documents/{id}/export?format=json -- export document.
async fn export_document(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Query(params): Query<ExportQuery>,
) -> std::result::Result<impl IntoResponse, AppError> {
    let format = params.format.as_deref().unwrap_or("json");

    let exported = state.export_document(&id, format).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") {
            AppError::not_found(msg)
        } else if msg.contains("unsupported") {
            AppError::bad_request(msg)
        } else {
            error!(error = %e, id = %id, format = %format, "export failed");
            AppError::internal(msg)
        }
    })?;

    // For JSON format, parse the string and return as JSON
    if format == "json" {
        let value: serde_json::Value =
            serde_json::from_str(&exported).unwrap_or(serde_json::Value::String(exported));
        Ok(Json(
            serde_json::json!({ "format": format, "content": value }),
        ))
    } else {
        Ok(Json(
            serde_json::json!({ "format": format, "content": exported }),
        ))
    }
}

/// POST /documents/{id}/query -- run SPARQL query.
async fn query_document(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(body): Json<QueryRequest>,
) -> std::result::Result<impl IntoResponse, AppError> {
    if body.sparql.is_empty() {
        return Err(AppError::bad_request("'sparql' field is required"));
    }

    let results = state.query_document(&id, &body.sparql).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") {
            AppError::not_found(msg)
        } else {
            error!(error = %e, id = %id, "query failed");
            AppError::bad_request(msg)
        }
    })?;

    Ok(Json(serde_json::json!({ "results": results })))
}

/// GET /documents/{id}/elements?type=Paragraph -- list elements.
async fn list_elements(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Query(params): Query<ElementsQuery>,
) -> std::result::Result<impl IntoResponse, AppError> {
    let results = state
        .list_elements(&id, params.element_type.as_deref())
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("not found") {
                AppError::not_found(msg)
            } else {
                error!(error = %e, id = %id, "list elements failed");
                AppError::internal(msg)
            }
        })?;

    Ok(Json(serde_json::json!({ "elements": results })))
}

/// GET /documents/{id}/chunks?max_tokens=512 -- chunk document.
async fn chunk_document(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Query(params): Query<ChunksQuery>,
) -> std::result::Result<impl IntoResponse, AppError> {
    let max_tokens = params.max_tokens.unwrap_or(512);

    let chunks = state.chunk_document(&id, max_tokens).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") {
            AppError::not_found(msg)
        } else {
            error!(error = %e, id = %id, "chunking failed");
            AppError::internal(msg)
        }
    })?;

    Ok(Json(serde_json::json!({
        "chunks": chunks,
        "count": chunks.len(),
        "max_tokens": max_tokens,
    })))
}
