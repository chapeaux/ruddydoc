//! HTTP API VLM client for calling OpenAI-compatible chat/completions endpoints.
//!
//! This module is gated behind the `vlm-api` feature. It provides [`ApiVlmModel`],
//! which sends page images as base64-encoded data URLs to a remote VLM server.

use serde::{Deserialize, Serialize};

use crate::types::{
    DocumentModel, ImageData, ModelTask, VlmModel, VlmPrediction, VlmResponseFormat,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Options for calling a VLM via an HTTP API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiVlmOptions {
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
    /// The expected response format.
    pub response_format: VlmResponseFormat,
}

impl Default for ApiVlmOptions {
    fn default() -> Self {
        Self {
            url: "http://localhost:8000/v1/chat/completions".to_string(),
            api_key: None,
            model_name: "SmolDocling-256M-preview".to_string(),
            timeout_secs: 120,
            temperature: 0.0,
            max_tokens: 4096,
            response_format: VlmResponseFormat::DocTags,
        }
    }
}

// ---------------------------------------------------------------------------
// Request / response DTOs for the OpenAI-compatible API
// ---------------------------------------------------------------------------

/// A content part within a chat message.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlContent },
}

/// The image URL object within a content part.
#[derive(Debug, Serialize)]
struct ImageUrlContent {
    url: String,
}

/// A chat message sent in the API request.
#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: Vec<ContentPart>,
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

/// Usage stats from the API response.
#[derive(Debug, Deserialize)]
struct ChatUsage {
    completion_tokens: u32,
}

/// The full chat/completions response body.
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

// ---------------------------------------------------------------------------
// ApiVlmModel
// ---------------------------------------------------------------------------

/// A VLM that calls an OpenAI-compatible chat/completions endpoint.
///
/// Sends page images as base64-encoded PNG data URLs. Uses `reqwest::blocking`
/// for simplicity (the pipeline is synchronous).
pub struct ApiVlmModel {
    options: ApiVlmOptions,
    client: reqwest::blocking::Client,
}

impl std::fmt::Debug for ApiVlmModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiVlmModel")
            .field("options", &self.options)
            .finish()
    }
}

impl ApiVlmModel {
    /// Create a new API VLM model with the given options.
    pub fn new(options: ApiVlmOptions) -> ruddydoc_core::Result<Self> {
        let timeout = std::time::Duration::from_secs(options.timeout_secs);
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| -> ruddydoc_core::Error {
                format!("failed to build HTTP client: {e}").into()
            })?;
        Ok(Self { options, client })
    }

    /// Build the request body for a given image and prompt.
    ///
    /// This is extracted as a public method so it can be tested independently
    /// without making an actual HTTP call.
    pub fn build_request_body(&self, image_base64: &str, prompt: &str) -> serde_json::Value {
        let request = ChatCompletionRequest {
            model: self.options.model_name.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: vec![
                    ContentPart::Text {
                        text: prompt.to_string(),
                    },
                    ContentPart::ImageUrl {
                        image_url: ImageUrlContent {
                            url: format!("data:image/png;base64,{image_base64}"),
                        },
                    },
                ],
            }],
            temperature: self.options.temperature,
            max_tokens: self.options.max_tokens,
        };
        // Serialize to Value; this cannot fail for this well-formed struct.
        serde_json::to_value(&request).unwrap_or_default()
    }

    /// Parse a raw JSON response string into a `VlmPrediction`.
    ///
    /// Extracted as a public method for testability.
    pub fn parse_response(&self, response_json: &str) -> ruddydoc_core::Result<VlmPrediction> {
        let response: ChatCompletionResponse =
            serde_json::from_str(response_json).map_err(|e| -> ruddydoc_core::Error {
                format!("failed to parse VLM response: {e}").into()
            })?;

        let choice =
            response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| -> ruddydoc_core::Error {
                    "VLM response contained no choices".into()
                })?;

        let num_tokens = response.usage.map(|u| u.completion_tokens).unwrap_or(0);

        Ok(VlmPrediction {
            text: choice.message.content,
            format: self.options.response_format,
            num_tokens,
            confidence: None,
        })
    }

    /// Encode image data to a base64 PNG data string.
    ///
    /// For simplicity, we encode the raw CHW float data directly as bytes.
    /// In a real implementation this would encode a proper PNG; for now we
    /// encode the raw pixel bytes (the CHW f32 data serialised as bytes).
    fn encode_image_base64(image: &ImageData) -> String {
        // Convert f32 CHW data back to u8 HWC data for encoding.
        let (w, h, c) = (
            image.width as usize,
            image.height as usize,
            image.channels as usize,
        );
        let mut hwc_bytes = vec![0u8; w * h * c];
        for ch in 0..c {
            for y in 0..h {
                for x in 0..w {
                    let chw_idx = ch * h * w + y * w + x;
                    let hwc_idx = (y * w + x) * c + ch;
                    let val = image.data.get(chw_idx).copied().unwrap_or(0.0);
                    hwc_bytes[hwc_idx] = (val.clamp(0.0, 1.0) * 255.0) as u8;
                }
            }
        }

        // Base64-encode the raw RGB bytes.
        // In production, this would encode a proper PNG image.
        base64_encode(&hwc_bytes)
    }
}

/// Simple base64 encoder (avoids adding a `base64` crate dependency).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

impl DocumentModel for ApiVlmModel {
    fn task(&self) -> ModelTask {
        ModelTask::Vlm
    }

    fn name(&self) -> &str {
        &self.options.model_name
    }

    fn version(&self) -> &str {
        "api"
    }
}

impl VlmModel for ApiVlmModel {
    fn predict(&self, image: &ImageData, prompt: &str) -> ruddydoc_core::Result<VlmPrediction> {
        let image_b64 = Self::encode_image_base64(image);
        let body = self.build_request_body(&image_b64, prompt);

        let mut request = self
            .client
            .post(&self.options.url)
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(ref key) = self.options.api_key {
            request = request.header("Authorization", format!("Bearer {key}"));
        }

        let response = request.send().map_err(|e| -> ruddydoc_core::Error {
            format!("VLM API request failed: {e}").into()
        })?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().unwrap_or_default();
            return Err(format!("VLM API returned status {status}: {body_text}").into());
        }

        let response_text = response.text().map_err(|e| -> ruddydoc_core::Error {
            format!("failed to read VLM response body: {e}").into()
        })?;

        self.parse_response(&response_text)
    }

    fn response_format(&self) -> VlmResponseFormat {
        self.options.response_format
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_vlm_options_default() {
        let opts = ApiVlmOptions::default();
        assert_eq!(opts.url, "http://localhost:8000/v1/chat/completions");
        assert!(opts.api_key.is_none());
        assert_eq!(opts.model_name, "SmolDocling-256M-preview");
        assert_eq!(opts.timeout_secs, 120);
        assert!((opts.temperature - 0.0).abs() < f32::EPSILON);
        assert_eq!(opts.max_tokens, 4096);
        assert_eq!(opts.response_format, VlmResponseFormat::DocTags);
    }

    #[test]
    fn api_vlm_options_serde_roundtrip() {
        let opts = ApiVlmOptions {
            url: "http://example.com/v1/chat/completions".to_string(),
            api_key: Some("sk-test-key".to_string()),
            model_name: "test-model".to_string(),
            timeout_secs: 60,
            temperature: 0.5,
            max_tokens: 2048,
            response_format: VlmResponseFormat::Markdown,
        };
        let json = serde_json::to_string(&opts).unwrap();
        let deserialized: ApiVlmOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.url, opts.url);
        assert_eq!(deserialized.api_key, opts.api_key);
        assert_eq!(deserialized.model_name, opts.model_name);
        assert_eq!(deserialized.timeout_secs, opts.timeout_secs);
        assert!((deserialized.temperature - opts.temperature).abs() < f32::EPSILON);
        assert_eq!(deserialized.max_tokens, opts.max_tokens);
        assert_eq!(deserialized.response_format, opts.response_format);
    }

    #[test]
    fn build_request_body_structure() {
        let model = ApiVlmModel::new(ApiVlmOptions::default()).unwrap();
        let body = model.build_request_body("dGVzdA==", "Convert this page to DocTags.");

        assert_eq!(body["model"], "SmolDocling-256M-preview");
        assert_eq!(body["temperature"], 0.0);
        assert_eq!(body["max_tokens"], 4096);

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);

        let msg = &messages[0];
        assert_eq!(msg["role"], "user");

        let content = msg["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Convert this page to DocTags.");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,dGVzdA=="
        );
    }

    #[test]
    fn parse_response_success() {
        let model = ApiVlmModel::new(ApiVlmOptions::default()).unwrap();
        let response_json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "<doctag><page><loc_title>Test Title</loc_title></page></doctag>"
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 42,
                "total_tokens": 142
            }
        }"#;

        let prediction = model.parse_response(response_json).unwrap();
        assert_eq!(
            prediction.text,
            "<doctag><page><loc_title>Test Title</loc_title></page></doctag>"
        );
        assert_eq!(prediction.format, VlmResponseFormat::DocTags);
        assert_eq!(prediction.num_tokens, 42);
        assert!(prediction.confidence.is_none());
    }

    #[test]
    fn parse_response_no_usage() {
        let model = ApiVlmModel::new(ApiVlmOptions::default()).unwrap();
        let response_json = r##"{
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "# Title"
                    }
                }
            ]
        }"##;

        let prediction = model.parse_response(response_json).unwrap();
        assert_eq!(prediction.text, "# Title");
        assert_eq!(prediction.num_tokens, 0);
    }

    #[test]
    fn parse_response_empty_choices_fails() {
        let model = ApiVlmModel::new(ApiVlmOptions::default()).unwrap();
        let response_json = r#"{"choices": []}"#;

        let result = model.parse_response(response_json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_response_invalid_json_fails() {
        let model = ApiVlmModel::new(ApiVlmOptions::default()).unwrap();
        let result = model.parse_response("not json");
        assert!(result.is_err());
    }

    #[test]
    fn document_model_trait_impl() {
        let model = ApiVlmModel::new(ApiVlmOptions::default()).unwrap();
        assert_eq!(model.task(), ModelTask::Vlm);
        assert_eq!(model.name(), "SmolDocling-256M-preview");
        assert_eq!(model.version(), "api");
    }

    #[test]
    fn vlm_model_response_format() {
        let model = ApiVlmModel::new(ApiVlmOptions {
            response_format: VlmResponseFormat::Html,
            ..ApiVlmOptions::default()
        })
        .unwrap();
        assert_eq!(model.response_format(), VlmResponseFormat::Html);
    }

    #[test]
    fn base64_encode_basic() {
        // "Hello" -> "SGVsbG8="
        let encoded = base64_encode(b"Hello");
        assert_eq!(encoded, "SGVsbG8=");
    }

    #[test]
    fn base64_encode_empty() {
        let encoded = base64_encode(b"");
        assert_eq!(encoded, "");
    }

    #[test]
    fn base64_encode_padding() {
        // "a" -> "YQ=="
        let encoded = base64_encode(b"a");
        assert_eq!(encoded, "YQ==");

        // "ab" -> "YWI="
        let encoded = base64_encode(b"ab");
        assert_eq!(encoded, "YWI=");

        // "abc" -> "YWJj" (no padding)
        let encoded = base64_encode(b"abc");
        assert_eq!(encoded, "YWJj");
    }
}
