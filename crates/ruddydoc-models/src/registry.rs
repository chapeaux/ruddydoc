//! Model registry: discovers and manages cached ONNX model files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::types::{ModelInfo, ModelTask};

/// Default subdirectory for model cache under the user's cache directory.
const CACHE_SUBDIR: &str = "ruddydoc/models";

/// Registry of available ML models.
///
/// The registry manages a local cache directory where ONNX model files
/// are stored. It does not download models itself; it provides discovery
/// and metadata for models that have been placed in the cache.
pub struct ModelRegistry {
    cache_dir: PathBuf,
    models: HashMap<ModelTask, ModelInfo>,
}

impl ModelRegistry {
    /// Create a new model registry using the default cache directory.
    ///
    /// The default cache directory is `~/.cache/ruddydoc/models/` on Linux,
    /// `~/Library/Caches/ruddydoc/models/` on macOS, and
    /// `%LOCALAPPDATA%/ruddydoc/models/` on Windows.
    ///
    /// The directory is created if it does not exist.
    pub fn new() -> ruddydoc_core::Result<Self> {
        let cache_dir = default_cache_dir()?;
        Self::with_cache_dir(cache_dir)
    }

    /// Create a new model registry with a custom cache directory.
    ///
    /// The directory is created if it does not exist.
    pub fn with_cache_dir(cache_dir: PathBuf) -> ruddydoc_core::Result<Self> {
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            cache_dir,
            models: HashMap::new(),
        })
    }

    /// Return the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Check if a model for the given task is registered.
    pub fn is_available(&self, task: ModelTask) -> bool {
        self.models.contains_key(&task)
    }

    /// Register a model with the registry.
    ///
    /// If a model for this task was previously registered, it is replaced.
    pub fn register(&mut self, info: ModelInfo) {
        self.models.insert(info.task, info);
    }

    /// Get model info for a specific task.
    pub fn model_info(&self, task: ModelTask) -> Option<&ModelInfo> {
        self.models.get(&task)
    }

    /// List all ONNX model files found in the cache directory.
    ///
    /// This scans the cache directory for `.onnx` files and returns
    /// basic metadata. Note that this does not assign tasks to files;
    /// use `register()` to associate a model file with a specific task.
    pub fn list_cached_models(&self) -> ruddydoc_core::Result<Vec<CachedModelFile>> {
        let mut files = Vec::new();

        if !self.cache_dir.exists() {
            return Ok(files);
        }

        for entry in std::fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "onnx") {
                let metadata = entry.metadata()?;
                files.push(CachedModelFile {
                    file_path: path,
                    file_size: metadata.len(),
                });
            }
        }

        files.sort_by(|a, b| a.file_path.cmp(&b.file_path));
        Ok(files)
    }

    /// Return all registered models.
    pub fn registered_models(&self) -> Vec<&ModelInfo> {
        self.models.values().collect()
    }
}

/// Information about a cached ONNX model file (not yet associated with a task).
#[derive(Debug, Clone)]
pub struct CachedModelFile {
    /// Path to the ONNX file.
    pub file_path: PathBuf,
    /// File size in bytes.
    pub file_size: u64,
}

/// Determine the default cache directory for model storage.
fn default_cache_dir() -> ruddydoc_core::Result<PathBuf> {
    // Try standard cache dirs in order of preference.
    if let Some(cache) = dirs_cache_dir() {
        return Ok(cache.join(CACHE_SUBDIR));
    }

    // Fallback: use home directory.
    if let Ok(home) = std::env::var("HOME") {
        return Ok(PathBuf::from(home).join(".cache").join(CACHE_SUBDIR));
    }

    // Last resort: use current directory.
    let cwd = std::env::current_dir()?;
    Ok(cwd.join(".ruddydoc").join("models"))
}

/// Try to get the OS-specific cache directory.
fn dirs_cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join("Library/Caches"))
    }

    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var("XDG_CACHE_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".cache"))
            })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_with_temp_dir() {
        let dir = std::env::temp_dir().join("ruddydoc_test_registry");
        let _ = std::fs::remove_dir_all(&dir);
        let registry = ModelRegistry::with_cache_dir(dir.clone()).unwrap();
        assert!(registry.cache_dir().exists());
        assert!(!registry.is_available(ModelTask::LayoutAnalysis));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_register_and_query() {
        let dir = std::env::temp_dir().join("ruddydoc_test_register");
        let _ = std::fs::remove_dir_all(&dir);
        let mut registry = ModelRegistry::with_cache_dir(dir.clone()).unwrap();

        assert!(!registry.is_available(ModelTask::Ocr));

        registry.register(ModelInfo {
            task: ModelTask::Ocr,
            name: "test-ocr".to_string(),
            version: "1.0".to_string(),
            file_path: dir.join("ocr.onnx"),
            file_size: 1000,
        });

        assert!(registry.is_available(ModelTask::Ocr));
        let info = registry.model_info(ModelTask::Ocr).unwrap();
        assert_eq!(info.name, "test-ocr");
        assert_eq!(info.version, "1.0");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_list_cached_empty() {
        let dir = std::env::temp_dir().join("ruddydoc_test_list_empty");
        let _ = std::fs::remove_dir_all(&dir);
        let registry = ModelRegistry::with_cache_dir(dir.clone()).unwrap();
        let cached = registry.list_cached_models().unwrap();
        assert!(cached.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_list_cached_with_files() {
        let dir = std::env::temp_dir().join("ruddydoc_test_list_files");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Create dummy files
        std::fs::write(dir.join("model_a.onnx"), b"fake model").unwrap();
        std::fs::write(dir.join("model_b.onnx"), b"another fake model").unwrap();
        std::fs::write(dir.join("readme.txt"), b"not a model").unwrap();

        let registry = ModelRegistry::with_cache_dir(dir.clone()).unwrap();
        let cached = registry.list_cached_models().unwrap();

        assert_eq!(cached.len(), 2);
        assert!(cached.iter().any(|f| f.file_path.ends_with("model_a.onnx")));
        assert!(cached.iter().any(|f| f.file_path.ends_with("model_b.onnx")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_register_replaces() {
        let dir = std::env::temp_dir().join("ruddydoc_test_replace");
        let _ = std::fs::remove_dir_all(&dir);
        let mut registry = ModelRegistry::with_cache_dir(dir.clone()).unwrap();

        registry.register(ModelInfo {
            task: ModelTask::LayoutAnalysis,
            name: "layout-v1".to_string(),
            version: "1.0".to_string(),
            file_path: dir.join("layout_v1.onnx"),
            file_size: 5000,
        });

        registry.register(ModelInfo {
            task: ModelTask::LayoutAnalysis,
            name: "layout-v2".to_string(),
            version: "2.0".to_string(),
            file_path: dir.join("layout_v2.onnx"),
            file_size: 6000,
        });

        let info = registry.model_info(ModelTask::LayoutAnalysis).unwrap();
        assert_eq!(info.name, "layout-v2");
        assert_eq!(info.version, "2.0");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_registered_models() {
        let dir = std::env::temp_dir().join("ruddydoc_test_registered");
        let _ = std::fs::remove_dir_all(&dir);
        let mut registry = ModelRegistry::with_cache_dir(dir.clone()).unwrap();

        registry.register(ModelInfo {
            task: ModelTask::Ocr,
            name: "ocr".to_string(),
            version: "1.0".to_string(),
            file_path: dir.join("ocr.onnx"),
            file_size: 1000,
        });
        registry.register(ModelInfo {
            task: ModelTask::TableStructure,
            name: "table".to_string(),
            version: "1.0".to_string(),
            file_path: dir.join("table.onnx"),
            file_size: 2000,
        });

        let models = registry.registered_models();
        assert_eq!(models.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
