//! ONNX Runtime ML model integration for RuddyDoc.
//!
//! This crate provides access to layout analysis, table structure
//! recognition, OCR, and picture classification models via ONNX Runtime.
//!
//! # Feature gates
//!
//! - **`onnx`** (default): Enables the ONNX Runtime integration via the `ort`
//!   crate. When disabled, only the types, preprocessing, postprocessing, and
//!   registry modules are available. The `OnnxModel` wrapper requires this
//!   feature.
//! - **`vlm-api`**: Enables the HTTP API VLM client via `reqwest`. The
//!   `ApiVlmModel` calls an OpenAI-compatible chat/completions endpoint.
//! - **`embeddings-api`**: Enables the HTTP API embedding client via
//!   `reqwest`. The `ApiEmbeddingModel` calls an OpenAI-compatible
//!   `/v1/embeddings` endpoint, for RAG-style semantic chunk retrieval.
//! - **`llm-api`**: Enables the HTTP API LLM client via `reqwest`. The
//!   `ApiLlmModel` calls an OpenAI-compatible chat/completions endpoint with
//!   a plain text prompt, for natural-language querying (NL -> SPARQL).
//!
//! # Modules
//!
//! - [`types`]: Core model types, traits (e.g., `LayoutModel`, `OcrModel`,
//!   `VlmModel`), and data structures (`ImageData`, `DetectedRegion`, etc.).
//! - [`preprocess`]: Pure-Rust image preprocessing (HWC-to-CHW conversion,
//!   bilinear resize, normalization).
//! - [`postprocess`]: Post-processing utilities (NMS, IoU, confidence
//!   filtering, reading order sorting, ontology class mapping).
//! - [`registry`]: Model file discovery and management.
//! - [`onnx`]: ONNX Runtime model wrapper (feature-gated behind `onnx`).
//! - [`vlm_api`]: HTTP API VLM client (feature-gated behind `vlm-api`).
//! - [`embedding_api`]: HTTP API embedding client (feature-gated behind
//!   `embeddings-api`).
//! - [`llm_api`]: HTTP API LLM client (feature-gated behind `llm-api`).

pub mod postprocess;
pub mod preprocess;
pub mod registry;
pub mod types;

#[cfg(feature = "onnx")]
pub mod onnx;

#[cfg(feature = "vlm-api")]
pub mod vlm_api;

#[cfg(feature = "embeddings-api")]
pub mod embedding_api;

#[cfg(feature = "llm-api")]
pub mod llm_api;

// Re-export commonly used items at the crate root.
pub use registry::ModelRegistry;
pub use types::{
    ClassificationModel, DetectedCell, DetectedRegion, DocumentModel, ImageData, LayoutModel,
    ModelInfo, ModelTask, OcrModel, OcrResult, RegionLabel, TableModel, VlmModel, VlmPrediction,
    VlmResponseFormat,
};

#[cfg(feature = "onnx")]
pub use onnx::OnnxModel;

#[cfg(feature = "vlm-api")]
pub use vlm_api::{ApiVlmModel, ApiVlmOptions};

#[cfg(feature = "embeddings-api")]
pub use embedding_api::{ApiEmbeddingModel, ApiEmbeddingOptions, EmbeddingProvider};

#[cfg(feature = "llm-api")]
pub use llm_api::{ApiLlmModel, ApiLlmOptions, LlmProvider};
