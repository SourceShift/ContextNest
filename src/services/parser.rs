use crate::error::ContextNestResult;
use crate::{
    config::{Config, ParserConfig},
    error::{ContextNestError, Result},
    models::{Screen, Style, Widget},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;

/// Parser service scaffold (tree-sitter-based, lightweight)
#[derive(Clone)]
pub struct ParserService {
    config: ParserConfig,
    cache: std::sync::Arc<tokio::sync::RwLock<HashMap<String, ParsedFile>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedFile {
    pub path: String,
    pub widgets: Vec<Widget>,
    pub screens: Vec<Screen>,
    pub styles: Vec<Style>,
    pub imports: Vec<String>,
    pub metadata: FileMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub size: usize,
    pub last_modified: chrono::DateTime<chrono::Utc>,
    pub hash: String,
    pub language: String,
}

impl ParserService {
    pub fn new(config: ParserConfig) -> ContextNestResult<Self> {
        let cache = std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new()));

        Ok(Self { config, cache })
    }

    /// Parse a source file and extract symbol information
    pub async fn parse_file(&self, file_path: &Path) -> ContextNestResult<ParsedFile> {
        // Check cache first
        let cache_key = self.create_cache_key(file_path).await?;

        {
            let cache_read = self.cache.read().await;
            if let Some(cached) = cache_read.get(&cache_key) {
                return Ok(cached.clone());
            }
        }

        // Read file content
        let content = tokio::fs::read_to_string(file_path).await?;

        // Check file size limit
        if content.len() > self.config.max_file_size {
            return Err(ContextNestError::Parser(format!(
                "File too large: {} bytes (max: {})",
                content.len(),
                self.config.max_file_size
            )));
        }

        // Parse with simplified logic (tree-sitter integration would be added later)
        let tree_sitter_result = self.parse_with_simple_logic(&content)?;

        // Enhance with Dart analyzer if available
        let enhanced_result = if !self.config.dart_analyzer_path.is_empty() {
            self.enhance_with_dart_analyzer(file_path, tree_sitter_result)
                .await?
        } else {
            tree_sitter_result
        };

        // Create file metadata
        let metadata = self.create_file_metadata(file_path, &content).await?;

        let parsed_file = ParsedFile {
            path: file_path.to_string_lossy().to_string(),
            widgets: enhanced_result.widgets,
            screens: enhanced_result.screens,
            styles: enhanced_result.styles,
            imports: enhanced_result.imports,
            metadata,
        };

        // Cache the result
        {
            let mut cache_write = self.cache.write().await;
            cache_write.insert(cache_key, parsed_file.clone());
        }

        Ok(parsed_file)
    }

    /// Parse multiple files in parallel
    pub async fn parse_files(
        &self,
        file_paths: Vec<PathBuf>,
    ) -> ContextNestResult<Vec<ParsedFile>> {
        use futures::stream::{self, StreamExt};

        const CONCURRENT_LIMIT: usize = 4;

        let results = stream::iter(file_paths)
            .map(|path| async move { self.parse_file(&path).await })
            .buffer_unordered(CONCURRENT_LIMIT)
            .collect::<Vec<_>>()
            .await;

        results.into_iter().collect::<Result<Vec<_>>>()
    }

    /// Parse content using simplified logic (placeholder for tree-sitter)
    fn parse_with_simple_logic(&self, content: &str) -> ContextNestResult<ParseResult> {
        let mut result = ParseResult {
            widgets: Vec::new(),
            screens: Vec::new(),
            styles: Vec::new(),
            imports: Vec::new(),
        };

        // Simple line-by-line parsing for demo purposes
        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Look for widget patterns
            if line.contains("Widget")
                || line.contains("StatelessWidget")
                || line.contains("StatefulWidget")
            {
                if let Some(widget) = self.extract_widget_from_line(line, line_num)? {
                    result.widgets.push(widget);
                }
            }

            // Look for screen patterns
            if line.contains("Screen") || line.contains("Page") {
                if let Some(screen) = self.extract_screen_from_line(line)? {
                    result.screens.push(screen);
                }
            }

            // Look for imports
            if line.starts_with("import") {
                result.imports.push(line.to_string());
            }
        }

        Ok(result)
    }

    /// Extract widget information from a line
    fn extract_widget_from_line(
        &self,
        line: &str,
        line_num: usize,
    ) -> ContextNestResult<Option<Widget>> {
        // Simple widget detection (would need proper parsing)
        if line.len() < 10 || line.len() > 200 {
            return Ok(None);
        }

        // Extract widget type
        let widget_type = if line.contains("Container") {
            "Container"
        } else if line.contains("Text") {
            "Text"
        } else if line.contains("Button") {
            "Button"
        } else if line.contains("StatelessWidget") {
            "StatelessWidget"
        } else if line.contains("StatefulWidget") {
            "StatefulWidget"
        } else if line.contains("Widget") {
            "Widget"
        } else {
            "Unknown"
        }
        .to_string();

        let widget = Widget::new(
            widget_type,
            line.to_string(),
            line_num * 100, // Approximate byte offset
            (line_num + 1) * 100,
        );

        Ok(Some(widget))
    }

    /// Extract screen information from a line
    fn extract_screen_from_line(&self, line: &str) -> ContextNestResult<Option<Screen>> {
        // Simple screen detection
        if let Some(name_start) = line.find("class ") {
            if let Some(name_end) = line[name_start + 6..].find(" ") {
                let class_name = &line[name_start + 6..name_start + 6 + name_end];

                if class_name.contains("Screen") || class_name.contains("Page") {
                    return Ok(Some(Screen {
                        name: class_name.to_string(),
                        route_name: format!("/{}", class_name.to_lowercase()),
                    }));
                }
            }
        }

        Ok(None)
    }

    /// Enhance parsing results with Dart analyzer
    async fn enhance_with_dart_analyzer(
        &self,
        file_path: &Path,
        base_result: ParseResult,
    ) -> ContextNestResult<ParseResult> {
        if self.config.dart_analyzer_path.is_empty() {
            return Err(ContextNestError::Parser(
                "Dart analyzer path not configured".to_string(),
            ));
        }
        let analyzer_path = &self.config.dart_analyzer_path;

        // Call dart analyzer
        let output = Command::new(analyzer_path)
            .args(&["analyze", "--format=json"])
            .arg(file_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            tracing::warn!(
                "Dart analyzer failed for {}: {}",
                file_path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(base_result);
        }

        // Parse analyzer output (simplified)
        let analyzer_output = String::from_utf8_lossy(&output.stdout);
        tracing::debug!("Dart analyzer output: {}", analyzer_output);

        // For now, just return the base result
        // In a real implementation, you would parse the analyzer output
        // and enhance the widget information with semantic details
        Ok(base_result)
    }

    /// Create cache key for file
    async fn create_cache_key(&self, file_path: &Path) -> ContextNestResult<String> {
        let metadata = tokio::fs::metadata(file_path).await?;
        let modified = metadata.modified()?;
        let size = metadata.len();

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        file_path.hash(&mut hasher);
        modified.hash(&mut hasher);
        size.hash(&mut hasher);

        Ok(format!("{}_{}", file_path.display(), hasher.finish()))
    }

    /// Create file metadata
    async fn create_file_metadata(
        &self,
        file_path: &Path,
        content: &str,
    ) -> ContextNestResult<FileMetadata> {
        let metadata = tokio::fs::metadata(file_path).await?;
        let modified = metadata.modified()?;

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        let hash = format!("{:x}", hasher.finish());

        Ok(FileMetadata {
            size: content.len(),
            last_modified: modified.into(),
            hash,
            language: "dart".to_string(),
        })
    }

    /// Clear parser cache
    pub async fn clear_cache(&self) {
        let mut cache_write = self.cache.write().await;
        cache_write.clear();
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> CacheStats {
        let cache_read = self.cache.read().await;
        CacheStats {
            size: cache_read.len(),
            memory_usage: cache_read.len() * std::mem::size_of::<ParsedFile>(),
        }
    }

    /// Health check for parser service
    pub async fn health_check(&self) -> ContextNestResult<bool> {
        // Check if dart analyzer is available
        if !self.config.dart_analyzer_path.is_empty() {
            let analyzer_path = &self.config.dart_analyzer_path;
            match Command::new(analyzer_path)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
            {
                Ok(status) => Ok(status.success()),
                Err(_) => Ok(false),
            }
        } else {
            // Tree-sitter parser is always available
            Ok(true)
        }
    }

    /// Find project root by walking up looking for a manifest
    pub async fn find_project_root(&self, start_path: &Path) -> Option<PathBuf> {
        let mut current = start_path.to_path_buf();

        loop {
            // Check for pubspec.yaml
            if current.join("pubspec.yaml").exists() {
                return Some(current);
            }

            // Check for known project-root markers
            if current.join("lib").exists() && current.join("android").exists() {
                return Some(current);
            }

            // Move up one directory
            if !current.pop() {
                break;
            }
        }

        None
    }

    /// Get all Dart files in a directory
    pub async fn find_dart_files(&self, dir: &Path) -> ContextNestResult<Vec<PathBuf>> {
        Box::pin(self.find_dart_files_impl(dir)).await
    }

    /// Implementation of find_dart_files with proper boxing
    async fn find_dart_files_impl(&self, dir: &Path) -> ContextNestResult<Vec<PathBuf>> {
        let mut dart_files = Vec::new();
        let mut entries = tokio::fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if extension == "dart" {
                        dart_files.push(path);
                    }
                }
            } else if path.is_dir() {
                // Skip common non-source directories
                if let Some(name) = path.file_name() {
                    let name_str = name.to_string_lossy();
                    if !name_str.starts_with('.') && name_str != "build" && name_str != "target" {
                        let mut sub_files = Box::pin(self.find_dart_files_impl(&path)).await?;
                        dart_files.append(&mut sub_files);
                    }
                }
            }
        }

        Ok(dart_files)
    }
}

#[derive(Debug, Clone)]
struct ParseResult {
    widgets: Vec<Widget>,
    screens: Vec<Screen>,
    styles: Vec<Style>,
    imports: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct CacheStats {
    pub size: usize,
    pub memory_usage: usize,
}
