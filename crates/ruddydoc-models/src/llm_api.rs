//! HTTP API LLM client for calling OpenAI-compatible chat/completions
//! endpoints.
//!
//! This module is gated behind the `llm-api` feature. It provides
//! [`ApiLlmModel`], which sends a plain text prompt to a remote LLM server
//! and returns its completion -- used for natural-language querying (NL →
//! SPARQL). Mirrors [`crate::vlm_api`]'s HTTP provider pattern (text-only,
//! no image content).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// LlmProvider
// ---------------------------------------------------------------------------

/// A provider of text completions -- prompt in, completion text out.
///
/// Standalone from [`crate::types::DocumentModel`], same reasoning as
/// [`crate::embedding_api::EmbeddingProvider`]: this isn't a document
/// structure-detection model.
pub trait LlmProvider: Send + Sync {
    /// Completes `prompt`, returning the model's response text.
    fn complete(&self, prompt: &str) -> ruddydoc_core::Result<String>;
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Options for calling an LLM via an HTTP API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiLlmOptions {
    /// API endpoint URL (e.g., `"http://localhost:8000/v1/chat/completions"`).
    pub url: String,
    /// API key (optional, for cloud-hosted models).
    pub api_key: Option<String>,
    /// Model name sent in the API request body.
    pub model_name: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Temperature for generation.
    pub temperature: f32,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
}

impl Default for ApiLlmOptions {
    fn default() -> Self {
        Self {
            url: "http://localhost:8000/v1/chat/completions".to_string(),
            api_key: None,
            model_name: "gpt-4o-mini".to_string(),
            timeout_secs: 60,
            temperature: 0.0,
            max_tokens: 1024,
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response DTOs for the OpenAI-compatible API
// ---------------------------------------------------------------------------

/// A chat message sent in the API request.
#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// The full chat/completions request body.
#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
}

/// A single choice in the API response.
#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

/// The message within a choice.
#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

/// The full chat/completions response body.
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

// ---------------------------------------------------------------------------
// ApiLlmModel
// ---------------------------------------------------------------------------

/// An LLM that calls an OpenAI-compatible chat/completions endpoint with a
/// plain text prompt. Uses `reqwest::blocking` for simplicity (RuddyDoc's
/// pipeline is synchronous).
pub struct ApiLlmModel {
    options: ApiLlmOptions,
    client: reqwest::blocking::Client,
}

impl std::fmt::Debug for ApiLlmModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiLlmModel")
            .field("options", &self.options)
            .finish()
    }
}

impl ApiLlmModel {
    /// Create a new API LLM model with the given options.
    pub fn new(options: ApiLlmOptions) -> ruddydoc_core::Result<Self> {
        let timeout = std::time::Duration::from_secs(options.timeout_secs);
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| -> ruddydoc_core::Error {
                format!("failed to build HTTP client: {e}").into()
            })?;
        Ok(Self { options, client })
    }

    /// Build the request body for a given prompt.
    ///
    /// Extracted as a public method so it can be tested independently
    /// without making an actual HTTP call.
    pub fn build_request_body(&self, prompt: &str) -> serde_json::Value {
        let request = ChatCompletionRequest {
            model: self.options.model_name.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            temperature: self.options.temperature,
            max_tokens: self.options.max_tokens,
        };
        // Serialize to Value; this cannot fail for this well-formed struct.
        serde_json::to_value(&request).unwrap_or_default()
    }

    /// Parse a raw JSON response string into the completion text.
    ///
    /// Extracted as a public method for testability.
    pub fn parse_response(&self, response_json: &str) -> ruddydoc_core::Result<String> {
        let response: ChatCompletionResponse =
            serde_json::from_str(response_json).map_err(|e| -> ruddydoc_core::Error {
                format!("failed to parse LLM response: {e}").into()
            })?;

        let choice =
            response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| -> ruddydoc_core::Error {
                    "LLM response contained no choices".into()
                })?;

        Ok(choice.message.content)
    }
}

impl LlmProvider for ApiLlmModel {
    fn complete(&self, prompt: &str) -> ruddydoc_core::Result<String> {
        let body = self.build_request_body(prompt);

        let mut request = self
            .client
            .post(&self.options.url)
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(ref key) = self.options.api_key {
            request = request.header("Authorization", format!("Bearer {key}"));
        }

        let response = request.send().map_err(|e| -> ruddydoc_core::Error {
            format!("LLM API request failed: {e}").into()
        })?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().unwrap_or_default();
            return Err(format!("LLM API returned status {status}: {body_text}").into());
        }

        let response_text = response.text().map_err(|e| -> ruddydoc_core::Error {
            format!("failed to read LLM response body: {e}").into()
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
    fn api_llm_options_default() {
        let opts = ApiLlmOptions::default();
        assert_eq!(opts.url, "http://localhost:8000/v1/chat/completions");
        assert!(opts.api_key.is_none());
        assert_eq!(opts.model_name, "gpt-4o-mini");
        assert_eq!(opts.timeout_secs, 60);
        assert!((opts.temperature - 0.0).abs() < f32::EPSILON);
        assert_eq!(opts.max_tokens, 1024);
    }

    #[test]
    fn build_request_body_structure() {
        let model = ApiLlmModel::new(ApiLlmOptions::default()).unwrap();
        let body = model.build_request_body("What headings does this document have?");

        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["temperature"], 0.0);
        assert_eq!(body["max_tokens"], 1024);

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(
            messages[0]["content"],
            "What headings does this document have?"
        );
    }

    #[test]
    fn parse_response_success() {
        let model = ApiLlmModel::new(ApiLlmOptions::default()).unwrap();
        let response_json = r#"{
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "SELECT ?h WHERE { ?h a <urn:Heading> }"
                    }
                }
            ]
        }"#;

        let completion = model.parse_response(response_json).unwrap();
        assert_eq!(completion, "SELECT ?h WHERE { ?h a <urn:Heading> }");
    }

    #[test]
    fn parse_response_empty_choices_fails() {
        let model = ApiLlmModel::new(ApiLlmOptions::default()).unwrap();
        let result = model.parse_response(r#"{"choices": []}"#);
        assert!(result.is_err());
    }

    #[test]
    fn parse_response_invalid_json_fails() {
        let model = ApiLlmModel::new(ApiLlmOptions::default()).unwrap();
        let result = model.parse_response("not json");
        assert!(result.is_err());
    }
}
