//! Core model types, traits, and data structures.

use ruddydoc_core::BoundingBox;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Model task enumeration
// ---------------------------------------------------------------------------

/// Supported model tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTask {
    /// Detect layout regions (titles, paragraphs, tables, etc.) on a page.
    LayoutAnalysis,
    /// Detect table cell structure from a table image.
    TableStructure,
    /// Optical character recognition on a text region.
    Ocr,
    /// Classify a picture region (chart, diagram, photo, etc.).
    PictureClassification,
    /// Visual language model (full-page understanding).
    Vlm,
}

impl std::fmt::Display for ModelTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::LayoutAnalysis => "layout_analysis",
            Self::TableStructure => "table_structure",
            Self::Ocr => "ocr",
            Self::PictureClassification => "picture_classification",
            Self::Vlm => "vlm",
        };
        write!(f, "{name}")
    }
}

// ---------------------------------------------------------------------------
// VLM response format
// ---------------------------------------------------------------------------

/// Response format from a Visual Language Model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VlmResponseFormat {
    /// DocTags format (SmolDocling/GraniteDocling output).
    DocTags,
    /// Markdown (general-purpose VLMs).
    Markdown,
    /// HTML (general-purpose VLMs).
    Html,
}

impl std::fmt::Display for VlmResponseFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::DocTags => "doc_tags",
            Self::Markdown => "markdown",
            Self::Html => "html",
        };
        write!(f, "{name}")
    }
}

// ---------------------------------------------------------------------------
// VLM prediction
// ---------------------------------------------------------------------------

/// A VLM prediction for a single page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlmPrediction {
    /// Raw text output from the model.
    pub text: String,
    /// Response format of the output.
    pub format: VlmResponseFormat,
    /// Number of tokens generated.
    pub num_tokens: u32,
    /// Model confidence (if available).
    pub confidence: Option<f32>,
}

// ---------------------------------------------------------------------------
// Region labels
// ---------------------------------------------------------------------------

/// Region labels from layout analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionLabel {
    Title,
    SectionHeader,
    Paragraph,
    List,
    Table,
    Picture,
    Caption,
    Footnote,
    PageHeader,
    PageFooter,
    Formula,
    Code,
}

impl std::fmt::Display for RegionLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Title => "Title",
            Self::SectionHeader => "SectionHeader",
            Self::Paragraph => "Paragraph",
            Self::List => "List",
            Self::Table => "Table",
            Self::Picture => "Picture",
            Self::Caption => "Caption",
            Self::Footnote => "Footnote",
            Self::PageHeader => "PageHeader",
            Self::PageFooter => "PageFooter",
            Self::Formula => "Formula",
            Self::Code => "Code",
        };
        write!(f, "{name}")
    }
}

// ---------------------------------------------------------------------------
// Detected region
// ---------------------------------------------------------------------------

/// A detected region on a page image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedRegion {
    /// The semantic label of this region.
    pub label: RegionLabel,
    /// Bounding box in page coordinates.
    pub bbox: BoundingBox,
    /// Model confidence score in `[0.0, 1.0]`.
    pub confidence: f32,
}

// ---------------------------------------------------------------------------
// Detected cell
// ---------------------------------------------------------------------------

/// A detected table cell from table structure recognition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedCell {
    /// Row index (0-based).
    pub row: u32,
    /// Column index (0-based).
    pub col: u32,
    /// Number of rows this cell spans.
    pub row_span: u32,
    /// Number of columns this cell spans.
    pub col_span: u32,
    /// Bounding box in table image coordinates.
    pub bbox: BoundingBox,
    /// Whether this cell is a header cell.
    pub is_header: bool,
    /// Model confidence score in `[0.0, 1.0]`.
    pub confidence: f32,
}

// ---------------------------------------------------------------------------
// OCR result
// ---------------------------------------------------------------------------

/// OCR result for a text region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    /// Recognized text.
    pub text: String,
    /// Model confidence score in `[0.0, 1.0]`.
    pub confidence: f32,
    /// Bounding box of the recognized text, if available.
    pub bbox: Option<BoundingBox>,
}

// ---------------------------------------------------------------------------
// Image data
// ---------------------------------------------------------------------------

/// Preprocessed image data ready for model input.
///
/// Pixel data is stored in CHW (channels, height, width) layout with
/// values normalized to `[0.0, 1.0]`.
#[derive(Debug, Clone)]
pub struct ImageData {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Number of channels (3 for RGB).
    pub channels: u32,
    /// Normalized pixel data in CHW layout, values in `[0.0, 1.0]`.
    pub data: Vec<f32>,
}

impl ImageData {
    /// Create from raw RGB bytes in HWC layout with values in `[0, 255]`.
    ///
    /// The `bytes` slice must have length `width * height * 3`.
    pub fn from_rgb(width: u32, height: u32, bytes: &[u8]) -> Self {
        let channels = 3u32;
        let data = crate::preprocess::hwc_to_chw(bytes, width, height, channels);
        Self {
            width,
            height,
            channels,
            data,
        }
    }

    /// Resize this image to target dimensions using bilinear interpolation.
    pub fn resize(&self, target_width: u32, target_height: u32) -> Self {
        let data = crate::preprocess::resize_bilinear(
            &self.data,
            self.width,
            self.height,
            target_width,
            target_height,
            self.channels,
        );
        Self {
            width: target_width,
            height: target_height,
            channels: self.channels,
            data,
        }
    }

    /// Normalize pixel values with the given mean and standard deviation per channel.
    ///
    /// Applies: `(pixel - mean) / std` for each channel.
    pub fn normalize(&self, mean: [f32; 3], std: [f32; 3]) -> Self {
        let mut data = self.data.clone();
        crate::preprocess::normalize(&mut data, self.channels, mean, std);
        Self {
            width: self.width,
            height: self.height,
            channels: self.channels,
            data,
        }
    }

    /// Convert to a flat tensor in NCHW layout `[1, C, H, W]`.
    ///
    /// The data is already in CHW layout, so this simply returns
    /// a clone of the internal data (the batch dimension is implicit).
    pub fn to_tensor(&self) -> Vec<f32> {
        self.data.clone()
    }
}

// ---------------------------------------------------------------------------
// Model info
// ---------------------------------------------------------------------------

/// Information about a cached model file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// The task this model performs.
    pub task: ModelTask,
    /// Human-readable model name.
    pub name: String,
    /// Model version string.
    pub version: String,
    /// Path to the ONNX model file.
    pub file_path: std::path::PathBuf,
    /// Size of the model file in bytes.
    pub file_size: u64,
}

// ---------------------------------------------------------------------------
// Model traits
// ---------------------------------------------------------------------------

/// Trait for all ML models.
pub trait DocumentModel: Send + Sync {
    /// The task this model performs.
    fn task(&self) -> ModelTask;
    /// Human-readable name of this model.
    fn name(&self) -> &str;
    /// Version string.
    fn version(&self) -> &str;
}

/// Layout analysis model trait.
pub trait LayoutModel: DocumentModel {
    /// Detect layout regions in an image.
    fn detect_layout(&self, image: &ImageData) -> ruddydoc_core::Result<Vec<DetectedRegion>>;
}

/// Table structure recognition model trait.
pub trait TableModel: DocumentModel {
    /// Detect table cells in a table image.
    fn detect_cells(&self, image: &ImageData) -> ruddydoc_core::Result<Vec<DetectedCell>>;
}

/// OCR model trait.
pub trait OcrModel: DocumentModel {
    /// Recognize text in an image region.
    fn recognize_text(&self, image: &ImageData) -> ruddydoc_core::Result<Vec<OcrResult>>;
}

/// Picture classification model trait.
pub trait ClassificationModel: DocumentModel {
    /// Classify a picture, returning `(label, confidence)`.
    fn classify(&self, image: &ImageData) -> ruddydoc_core::Result<(String, f32)>;
}

/// Visual Language Model trait.
///
/// VLMs take a page image and produce structured document output in a
/// single call, combining layout analysis, OCR, and table extraction.
pub trait VlmModel: DocumentModel {
    /// Process a page image with the given prompt and produce structured text output.
    fn predict(&self, image: &ImageData, prompt: &str) -> ruddydoc_core::Result<VlmPrediction>;

    /// The response format this model produces.
    fn response_format(&self) -> VlmResponseFormat;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_task_display() {
        assert_eq!(ModelTask::LayoutAnalysis.to_string(), "layout_analysis");
        assert_eq!(ModelTask::Ocr.to_string(), "ocr");
    }

    #[test]
    fn region_label_display() {
        assert_eq!(RegionLabel::Title.to_string(), "Title");
        assert_eq!(RegionLabel::SectionHeader.to_string(), "SectionHeader");
    }

    #[test]
    fn model_task_serde_roundtrip() {
        let task = ModelTask::TableStructure;
        let json = serde_json::to_string(&task).unwrap();
        let deserialized: ModelTask = serde_json::from_str(&json).unwrap();
        assert_eq!(task, deserialized);
    }

    #[test]
    fn region_label_serde_roundtrip() {
        let label = RegionLabel::Formula;
        let json = serde_json::to_string(&label).unwrap();
        let deserialized: RegionLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(label, deserialized);
    }

    #[test]
    fn model_info_serialization() {
        let info = ModelInfo {
            task: ModelTask::LayoutAnalysis,
            name: "test-layout".to_string(),
            version: "1.0.0".to_string(),
            file_path: std::path::PathBuf::from("/tmp/model.onnx"),
            file_size: 42_000_000,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ModelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task, ModelTask::LayoutAnalysis);
        assert_eq!(deserialized.name, "test-layout");
        assert_eq!(deserialized.version, "1.0.0");
        assert_eq!(deserialized.file_size, 42_000_000);
    }

    #[test]
    fn image_data_from_rgb() {
        // 2x2 red image
        let bytes = vec![
            255, 0, 0, 255, 0, 0, // row 0
            255, 0, 0, 255, 0, 0, // row 1
        ];
        let img = ImageData::from_rgb(2, 2, &bytes);
        assert_eq!(img.width, 2);
        assert_eq!(img.height, 2);
        assert_eq!(img.channels, 3);
        // CHW layout: first 4 values are channel 0 (R), all 1.0
        assert_eq!(img.data.len(), 12);
        // R channel
        assert!((img.data[0] - 1.0).abs() < 1e-6);
        assert!((img.data[1] - 1.0).abs() < 1e-6);
        assert!((img.data[2] - 1.0).abs() < 1e-6);
        assert!((img.data[3] - 1.0).abs() < 1e-6);
        // G channel
        assert!((img.data[4]).abs() < 1e-6);
        // B channel
        assert!((img.data[8]).abs() < 1e-6);
    }

    #[test]
    fn image_data_to_tensor() {
        let bytes = vec![128, 64, 32, 0, 0, 0];
        let img = ImageData::from_rgb(2, 1, &bytes);
        let tensor = img.to_tensor();
        assert_eq!(tensor.len(), 6); // 3 channels * 2 * 1
    }

    #[test]
    fn detected_region_serde() {
        let region = DetectedRegion {
            label: RegionLabel::Paragraph,
            bbox: BoundingBox {
                left: 10.0,
                top: 20.0,
                right: 100.0,
                bottom: 50.0,
            },
            confidence: 0.95,
        };
        let json = serde_json::to_string(&region).unwrap();
        let deserialized: DetectedRegion = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.label, RegionLabel::Paragraph);
        assert!((deserialized.confidence - 0.95).abs() < 1e-6);
    }

    #[test]
    fn detected_cell_serde() {
        let cell = DetectedCell {
            row: 0,
            col: 1,
            row_span: 2,
            col_span: 1,
            bbox: BoundingBox {
                left: 10.0,
                top: 20.0,
                right: 100.0,
                bottom: 50.0,
            },
            is_header: true,
            confidence: 0.88,
        };
        let json = serde_json::to_string(&cell).unwrap();
        let deserialized: DetectedCell = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_header);
        assert_eq!(deserialized.row_span, 2);
    }

    #[test]
    fn model_task_vlm_display() {
        assert_eq!(ModelTask::Vlm.to_string(), "vlm");
    }

    #[test]
    fn model_task_vlm_serde_roundtrip() {
        let task = ModelTask::Vlm;
        let json = serde_json::to_string(&task).unwrap();
        assert_eq!(json, "\"vlm\"");
        let deserialized: ModelTask = serde_json::from_str(&json).unwrap();
        assert_eq!(task, deserialized);
    }

    #[test]
    fn vlm_response_format_serde_roundtrip() {
        for fmt in [
            VlmResponseFormat::DocTags,
            VlmResponseFormat::Markdown,
            VlmResponseFormat::Html,
        ] {
            let json = serde_json::to_string(&fmt).unwrap();
            let deserialized: VlmResponseFormat = serde_json::from_str(&json).unwrap();
            assert_eq!(fmt, deserialized);
        }
    }

    #[test]
    fn vlm_response_format_display() {
        assert_eq!(VlmResponseFormat::DocTags.to_string(), "doc_tags");
        assert_eq!(VlmResponseFormat::Markdown.to_string(), "markdown");
        assert_eq!(VlmResponseFormat::Html.to_string(), "html");
    }

    #[test]
    fn vlm_prediction_creation_and_serde() {
        let pred = VlmPrediction {
            text: "<doctag><page><loc_title>Hello</loc_title></page></doctag>".to_string(),
            format: VlmResponseFormat::DocTags,
            num_tokens: 42,
            confidence: Some(0.95),
        };
        let json = serde_json::to_string(&pred).unwrap();
        let deserialized: VlmPrediction = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.text, pred.text);
        assert_eq!(deserialized.format, VlmResponseFormat::DocTags);
        assert_eq!(deserialized.num_tokens, 42);
        assert!((deserialized.confidence.unwrap() - 0.95).abs() < 1e-6);
    }

    #[test]
    fn vlm_prediction_no_confidence() {
        let pred = VlmPrediction {
            text: "# Title".to_string(),
            format: VlmResponseFormat::Markdown,
            num_tokens: 10,
            confidence: None,
        };
        let json = serde_json::to_string(&pred).unwrap();
        let deserialized: VlmPrediction = serde_json::from_str(&json).unwrap();
        assert!(deserialized.confidence.is_none());
    }

    #[test]
    fn ocr_result_serde() {
        let result = OcrResult {
            text: "Hello world".to_string(),
            confidence: 0.99,
            bbox: Some(BoundingBox {
                left: 5.0,
                top: 10.0,
                right: 200.0,
                bottom: 30.0,
            }),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: OcrResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.text, "Hello world");
        assert!(deserialized.bbox.is_some());
    }
}
