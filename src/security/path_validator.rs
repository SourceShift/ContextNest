//! Path Validator - CRITICAL security component
//! Prevents path traversal attacks by validating file paths against allowed directories.
//! This is the first line of defense against malicious plugins attempting to access
//! sensitive files outside their sandbox.

use crate::error::ContextNestResult;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathValidationError {
    #[error("Path canonicalization failed: {0}")]
    CanonicalizationFailed(String),

    #[error("Access denied: path not in allowed list: {0}")]
    NotInAllowedList(String),

    #[error("Access to hidden files not allowed: {0}")]
    HiddenFileAccess(String),

    #[error("Path traversal pattern detected: {0}")]
    TraversalPattern(String),

    #[error("Symlink access not allowed: {0} -> {1}")]
    SymlinkAccess(String, String),
}

/// PathValidator prevents directory traversal attacks and enforces path restrictions
pub struct PathValidator {
    /// Paths that plugins are allowed to access
    allowed_paths: Vec<PathBuf>,

    /// Paths that are read-only
    readonly_paths: Vec<PathBuf>,

    /// Whether to block symlink access (recommended: true)
    enable_symlink_protection: bool,
}

impl PathValidator {
    /// Create a new PathValidator with specified allowed paths.
    /// Canonicalizes every entry in `allowed_paths` and `readonly_paths` at
    /// construction time so they line up with the canonicalized form
    /// `validate_path` produces for incoming requests. Without this, allow-list
    /// comparisons fail on any platform where the configured root passes
    /// through a symlink — notably macOS, where `/var/folders` (the
    /// `TempDir` root) canonicalizes to `/private/var/folders`. Entries that
    /// don't currently exist on disk are kept as-given (canonicalize would
    /// fail otherwise); they'll match exact-string requests but won't survive
    /// symlink resolution. Best practice: only configure paths that exist.
    pub fn new(
        allowed_paths: Vec<PathBuf>,
        readonly_paths: Vec<PathBuf>,
        enable_symlink_protection: bool,
    ) -> Self {
        let allowed_paths = allowed_paths
            .into_iter()
            .map(|p| p.canonicalize().unwrap_or(p))
            .collect();
        let readonly_paths = readonly_paths
            .into_iter()
            .map(|p| p.canonicalize().unwrap_or(p))
            .collect();
        Self {
            allowed_paths,
            readonly_paths,
            enable_symlink_protection,
        }
    }

    /// Validate a path against security policies
    /// This performs multiple checks:
    /// 1. Canonicalize path (resolve symlinks, relative paths)
    /// 2. Check if path is within allowed directories
    /// 3. Detect symlink attacks
    /// 4. Block hidden files
    /// 5. Prevent path traversal patterns
    pub fn validate_path(
        &self,
        requested_path: &Path,
    ) -> std::result::Result<PathBuf, PathValidationError> {
        // Step 1: Canonicalize path (resolve symlinks, relative paths)
        let canonical_path = requested_path
            .canonicalize()
            .map_err(|e| PathValidationError::CanonicalizationFailed(e.to_string()))?;

        // Step 2: Check if path is within allowed directories
        let is_allowed = self
            .allowed_paths
            .iter()
            .any(|allowed| canonical_path.starts_with(allowed));

        if !is_allowed {
            tracing::error!(
                "🔒 Blocked file access outside allowed paths: {:?}",
                canonical_path
            );
            return Err(PathValidationError::NotInAllowedList(
                canonical_path.display().to_string(),
            ));
        }

        // Step 3: Detect symlink attacks.
        // Only fire on a symlink at the requested leaf — not on any
        // canonicalization-rewrites-the-prefix case, which would falsely
        // trip on system symlinks (e.g. on macOS, `/var/folders` ->
        // `/private/var/folders`) for every request. Step 2 already enforces
        // that the canonicalized path stays inside an allowed root, so the
        // only remaining symlink risk is a user-controlled symlink as the
        // leaf itself.
        if self.enable_symlink_protection {
            if let Ok(meta) = requested_path.symlink_metadata() {
                if meta.file_type().is_symlink() {
                    tracing::warn!(
                        "⚠️  Symlink detected at requested leaf: {:?} -> {:?}",
                        requested_path,
                        canonical_path
                    );
                    return Err(PathValidationError::SymlinkAccess(
                        requested_path.display().to_string(),
                        canonical_path.display().to_string(),
                    ));
                }
            }
        }

        // Step 4: Check for hidden files (security risk)
        if let Some(file_name) = canonical_path.file_name() {
            if let Some(name_str) = file_name.to_str() {
                if name_str.starts_with('.') && name_str != "." && name_str != ".." {
                    return Err(PathValidationError::HiddenFileAccess(
                        canonical_path.display().to_string(),
                    ));
                }
            }
        }

        // Step 5: Prevent path traversal patterns (belt and suspenders)
        let path_str = canonical_path.to_string_lossy();
        if path_str.contains("../") || path_str.contains("..\\") {
            return Err(PathValidationError::TraversalPattern(path_str.to_string()));
        }

        Ok(canonical_path)
    }

    /// Check if a path is read-only.
    /// Canonicalizes the input so the comparison stays consistent with the
    /// canonicalized entries stored at construction.
    pub fn is_readonly(&self, path: &Path) -> bool {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.readonly_paths
            .iter()
            .any(|ro_path| canonical.starts_with(ro_path))
    }

    /// Add an allowed path at runtime
    pub fn add_allowed_path(&mut self, path: PathBuf) {
        if !self.allowed_paths.contains(&path) {
            self.allowed_paths.push(path);
        }
    }

    /// Add a readonly path at runtime
    pub fn add_readonly_path(&mut self, path: PathBuf) {
        if !self.readonly_paths.contains(&path) {
            self.readonly_paths.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_validates_allowed_path() {
        let temp_dir = TempDir::new().unwrap();
        let allowed_path = temp_dir.path().to_path_buf();

        let validator = PathValidator::new(vec![allowed_path.clone()], vec![], true);

        let test_file = allowed_path.join("test.txt");
        fs::write(&test_file, "test").unwrap();

        let result = validator.validate_path(&test_file);
        assert!(result.is_ok());
    }

    #[test]
    fn test_blocks_path_outside_allowed() {
        let temp_dir = TempDir::new().unwrap();
        let allowed_path = temp_dir.path().join("allowed");
        fs::create_dir_all(&allowed_path).unwrap();

        let validator = PathValidator::new(vec![allowed_path], vec![], true);

        // Try to access parent directory
        let blocked_path = temp_dir.path().join("blocked.txt");
        fs::write(&blocked_path, "test").unwrap();

        let result = validator.validate_path(&blocked_path);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(PathValidationError::NotInAllowedList(_))
        ));
    }

    #[test]
    fn test_blocks_hidden_files() {
        let temp_dir = TempDir::new().unwrap();
        let allowed_path = temp_dir.path().to_path_buf();

        let validator = PathValidator::new(vec![allowed_path.clone()], vec![], true);

        let hidden_file = allowed_path.join(".hidden");
        fs::write(&hidden_file, "secret").unwrap();

        let result = validator.validate_path(&hidden_file);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(PathValidationError::HiddenFileAccess(_))
        ));
    }

    #[test]
    fn test_identifies_readonly_paths() {
        let temp_dir = TempDir::new().unwrap();
        let allowed_path = temp_dir.path().to_path_buf();
        let readonly_path = allowed_path.join("readonly");
        fs::create_dir_all(&readonly_path).unwrap();

        let validator = PathValidator::new(
            vec![allowed_path.clone()],
            vec![readonly_path.clone()],
            true,
        );

        let readonly_file = readonly_path.join("config.txt");
        fs::write(&readonly_file, "config").unwrap();

        assert!(validator.is_readonly(&readonly_file));

        let writable_file = allowed_path.join("data.txt");
        assert!(!validator.is_readonly(&writable_file));
    }
}
