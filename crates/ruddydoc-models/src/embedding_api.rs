//! HTTP API embedding client for calling OpenAI-compatible `/v1/embeddings`
//! endpoints.
//!
//! This module is gated behind the `embeddings-api` feature. It provides
//! [`ApiEmbeddingModel`], which sends text to a remote embedding server and
//! returns dense vectors -- used for RAG-style semantic chunk retrieval.
//! Mirrors [`crate::vlm_api`]'s HTTP provider pattern.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// EmbeddingProvider
// ---------------------------------------------------------------------------

/// A provider of text embeddings -- text in, dense vectors out, same order.
///
/// Standalone from [`crate::types::DocumentModel`]: that trait hierarchy is
/// about document-*structure* detection models (layout, table, OCR, VLM,
/// classification); embeddings serve a different concern (retrieval), so
/// they don't share a `task()`/`ModelTask` categorization with those.
pub trait EmbeddingProvider: Send + Sync {
    /// Embeds a batch of texts, returning one vector per input in the same
    /// order. Returns an empty vector for an empty input slice.
    fn embed(&self, texts: &[String]) -> ruddydoc_core::Result<Vec<Vec<f32>>>;
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Options for calling an embedding model via an HTTP API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEmbeddingOptions {
    /// API endpoint URL (e.g., `"http://localhost:8000/v1/embeddings"`).
    pub url: String,
    /// API key (optional, for cloud-hosted models).
    pub api_key: Option<String>,
    /// Model name sent in the API request body.
    pub model_name: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for ApiEmbeddingOptions {
    fn default() -> Self {
        Self {
            url: "http://localhost:8000/v1/embeddings".to_string(),
            api_key: None,
            model_name: "text-embedding-3-small".to_string(),
            timeout_secs: 60,
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response DTOs for the OpenAI-compatible API
// ---------------------------------------------------------------------------

/// The `/v1/embeddings` request body.
#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

/// One embedding entry in the API response.
#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    /// The API doesn't guarantee response order matches request order --
    /// this is the input's original position, used to resort.
    index: usize,
}

/// The full `/v1/embeddings` response body.
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

// ---------------------------------------------------------------------------
// ApiEmbeddingModel
// ---------------------------------------------------------------------------

/// An embedding provider that calls an OpenAI-compatible `/v1/embeddings`
/// endpoint. Uses `reqwest::blocking` for simplicity (RuddyDoc's pipeline is
/// synchronous).
pub struct ApiEmbeddingModel {
    options: ApiEmbeddingOptions,
    client: reqwest::blocking::Client,
}

impl std::fmt::Debug for ApiEmbeddingModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiEmbeddingModel")
            .field("options", &self.options)
            .finish()
    }
}

impl ApiEmbeddingModel {
    /// Create a new API embedding model with the given options.
    pub fn new(options: ApiEmbeddingOptions) -> ruddydoc_core::Result<Self> {
        let timeout = std::time::Duration::from_secs(options.timeout_secs);
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| -> ruddydoc_core::Error {
                format!("failed to build HTTP client: {e}").into()
            })?;
        Ok(Self { options, client })
    }

    /// Build the request body for a given batch of texts.
    ///
    /// Extracted as a public method so it can be tested independently
    /// without making an actual HTTP call.
    pub fn build_request_body(&self, texts: &[String]) -> serde_json::Value {
        let request = EmbeddingRequest {
            model: &self.options.model_name,
            input: texts,
        };
        // Serialize to Value; this cannot fail for this well-formed struct.
        serde_json::to_value(&request).unwrap_or_default()
    }

    /// Parse a raw JSON response string into embedding vectors, resorted by
    /// the response's `index` field to match the original request order.
    ///
    /// Extracted as a public method for testability.
    pub fn parse_response(&self, response_json: &str) -> ruddydoc_core::Result<Vec<Vec<f32>>> {
        let mut response: EmbeddingResponse = serde_json::from_str(response_json)
            .map_err(|e| -> ruddydoc_core::Error {
                format!("failed to parse embedding response: {e}").into()
            })?;
        response.data.sort_by_key(|d| d.index);
        Ok(response.data.into_iter().map(|d| d.embedding).collect())
    }
}

impl EmbeddingProvider for ApiEmbeddingModel {
    fn embed(&self, texts: &[String]) -> ruddydoc_core::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let body = self.build_request_body(texts);

        let mut request = self
            .client
            .post(&self.options.url)
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(ref key) = self.options.api_key {
            request = request.header("Authorization", format!("Bearer {key}"));
        }

        let response = request.send().map_err(|e| -> ruddydoc_core::Error {
            format!("embedding API request failed: {e}").into()
        })?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().unwrap_or_default();
            return Err(format!("embedding API returned status {status}: {body_text}").into());
        }

        let response_text = response.text().map_err(|e| -> ruddydoc_core::Error {
            format!("failed to read embedding response body: {e}").into()
        })?;

        self.parse_response(&response_text)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_embedding_options_default() {
        let opts = ApiEmbeddingOptions::default();
        assert_eq!(opts.url, "http://localhost:8000/v1/embeddings");
        assert!(opts.api_key.is_none());
        assert_eq!(opts.model_name, "text-embedding-3-small");
        assert_eq!(opts.timeout_secs, 60);
    }

    #[test]
    fn api_embedding_options_serde_roundtrip() {
        let opts = ApiEmbeddingOptions {
            url: "http://example.com/v1/embeddings".to_string(),
            api_key: Some("sk-test-key".to_string()),
            model_name: "test-model".to_string(),
            timeout_secs: 30,
        };
        let json = serde_json::to_string(&opts).unwrap();
        let deserialized: ApiEmbeddingOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.url, opts.url);
        assert_eq!(deserialized.api_key, opts.api_key);
        assert_eq!(deserialized.model_name, opts.model_name);
        assert_eq!(deserialized.timeout_secs, opts.timeout_secs);
    }

    #[test]
    fn build_request_body_structure() {
        let model = ApiEmbeddingModel::new(ApiEmbeddingOptions::default()).unwrap();
        let texts = vec!["hello".to_string(), "world".to_string()];
        let body = model.build_request_body(&texts);

        assert_eq!(body["model"], "text-embedding-3-small");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0], "hello");
        assert_eq!(input[1], "world");
    }

    #[test]
    fn parse_response_success() {
        let model = ApiEmbeddingModel::new(ApiEmbeddingOptions::default()).unwrap();
        let response_json = r#"{
            "data": [
                {"embedding": [0.1, 0.2], "index": 0},
                {"embedding": [0.3, 0.4], "index": 1}
            ]
        }"#;

        let vectors = model.parse_response(response_json).unwrap();
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0], vec![0.1, 0.2]);
        assert_eq!(vectors[1], vec![0.3, 0.4]);
    }

    #[test]
    fn parse_response_resorts_out_of_order_indices() {
        let model = ApiEmbeddingModel::new(ApiEmbeddingOptions::default()).unwrap();
        // The API doesn't guarantee response order matches request order.
        let response_json = r#"{
            "data": [
                {"embedding": [9.0], "index": 1},
                {"embedding": [1.0], "index": 0}
            ]
        }"#;

        let vectors = model.parse_response(response_json).unwrap();
        assert_eq!(vectors, vec![vec![1.0], vec![9.0]]);
    }

    #[test]
    fn parse_response_invalid_json_fails() {
        let model = ApiEmbeddingModel::new(ApiEmbeddingOptions::default()).unwrap();
        let result = model.parse_response("not json");
        assert!(result.is_err());
    }

    #[test]
    fn embed_empty_input_returns_empty_without_a_request() {
        let model = ApiEmbeddingModel::new(ApiEmbeddingOptions::default()).unwrap();
        // No HTTP call should happen for an empty batch -- if one did, this
        // would fail/hang since there's no server at the default URL.
        let vectors = model.embed(&[]).unwrap();
        assert!(vectors.is_empty());
    }
}
