//! ONNX Runtime wrapper for model loading and inference.
//!
//! This module is only available when the `onnx` feature is enabled.
//! It wraps the `ort` crate to provide a simplified interface for
//! loading ONNX models and running inference on image data.

use std::path::Path;

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::{Tensor, ValueType};

use crate::types::{ImageData, ModelTask};

/// A loaded ONNX Runtime model ready for inference.
///
/// Wraps an `ort::Session` with metadata about the model's purpose.
pub struct OnnxModel {
    session: Session,
    task: ModelTask,
    name: String,
    version: String,
}

impl OnnxModel {
    /// Load an ONNX model from a file path.
    ///
    /// The model is loaded with Level3 graph optimization by default.
    pub fn load(
        path: &Path,
        task: ModelTask,
        name: &str,
        version: &str,
    ) -> ruddydoc_core::Result<Self> {
        let session = Session::builder()
            .map_err(|e| -> ruddydoc_core::Error {
                format!("Failed to create session builder: {e}").into()
            })?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| -> ruddydoc_core::Error {
                format!("Failed to set optimization level: {e}").into()
            })?
            .commit_from_file(path)
            .map_err(|e| -> ruddydoc_core::Error {
                format!("Failed to load ONNX model from {}: {e}", path.display()).into()
            })?;

        Ok(Self {
            session,
            task,
            name: name.to_string(),
            version: version.to_string(),
        })
    }

    /// Load an ONNX model from in-memory bytes.
    pub fn load_from_memory(
        bytes: &[u8],
        task: ModelTask,
        name: &str,
        version: &str,
    ) -> ruddydoc_core::Result<Self> {
        let session = Session::builder()
            .map_err(|e| -> ruddydoc_core::Error {
                format!("Failed to create session builder: {e}").into()
            })?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| -> ruddydoc_core::Error {
                format!("Failed to set optimization level: {e}").into()
            })?
            .commit_from_memory(bytes)
            .map_err(|e| -> ruddydoc_core::Error {
                format!("Failed to load ONNX model from memory: {e}").into()
            })?;

        Ok(Self {
            session,
            task,
            name: name.to_string(),
            version: version.to_string(),
        })
    }

    /// Return the task this model is intended for.
    pub fn task(&self) -> ModelTask {
        self.task
    }

    /// Return the model's human-readable name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the model's version string.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Run inference with a single image input.
    ///
    /// The image is converted to an ONNX tensor in NCHW layout
    /// `[1, C, H, W]` and fed to the first input of the session.
    /// Returns the raw output values from the model.
    pub fn infer(&mut self, input: &ImageData) -> ruddydoc_core::Result<Vec<Vec<f32>>> {
        let tensor_data = input.to_tensor();
        let shape = [
            1i64,
            input.channels as i64,
            input.height as i64,
            input.width as i64,
        ];

        let input_tensor =
            Tensor::from_array((shape, tensor_data)).map_err(|e| -> ruddydoc_core::Error {
                format!("Failed to create input tensor: {e}").into()
            })?;

        let outputs = self
            .session
            .run(ort::inputs![input_tensor])
            .map_err(|e| -> ruddydoc_core::Error { format!("Inference failed: {e}").into() })?;

        let mut result = Vec::new();
        for (_name, value) in &outputs {
            match value.try_extract_tensor::<f32>() {
                Ok((_shape, data)) => {
                    result.push(data.to_vec());
                }
                Err(_) => {
                    // Skip non-f32 outputs for now
                    result.push(Vec::new());
                }
            }
        }

        Ok(result)
    }

    /// Get the expected input shape from the model's first input.
    ///
    /// Returns `None` if the model has no inputs or the input is not a tensor.
    /// Dynamic dimensions are represented as `-1`.
    pub fn input_shape(&self) -> Option<Vec<i64>> {
        let input = self.session.inputs().first()?;
        match input.dtype() {
            ValueType::Tensor { shape, .. } => Some(shape.iter().copied().collect()),
            _ => None,
        }
    }

    /// Get the output names from the model.
    pub fn output_names(&self) -> Vec<String> {
        self.session
            .outputs()
            .iter()
            .map(|o| o.name().to_string())
            .collect()
    }

    /// Get the input names from the model.
    pub fn input_names(&self) -> Vec<String> {
        self.session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect()
    }
}

impl crate::types::DocumentModel for OnnxModel {
    fn task(&self) -> ModelTask {
        self.task
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }
}

// Note: Integration tests for OnnxModel require actual ONNX model files,
// so they are not included in unit tests. The struct's API is exercised
// through the public interface once models are available.
