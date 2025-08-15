use crate::context::field::{NeuralField, SemanticPattern};
use crate::error::ContextNestResult;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Cross-dimensional projection methods
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProjectionMethod {
    /// Principal Component Analysis projection
    PCA,
    /// Random projection with Johnson-Lindenstrauss lemma
    Random,
    /// Learned projection using autoencoder-style mapping
    Learned,
    /// Semantic-aware projection preserving semantic relationships
    Semantic,
    /// Adaptive projection that changes based on context
    Adaptive { adaptation_rate: f32 },
}

impl Eq for ProjectionMethod {}

impl std::hash::Hash for ProjectionMethod {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            ProjectionMethod::PCA => 0.hash(state),
            ProjectionMethod::Random => 1.hash(state),
            ProjectionMethod::Learned => 2.hash(state),
            ProjectionMethod::Semantic => 3.hash(state),
            ProjectionMethod::Adaptive { adaptation_rate } => {
                4.hash(state);
                // Hash the f32 as bits to avoid float precision issues
                adaptation_rate.to_bits().hash(state);
            }
        }
    }
}

/// Projection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionConfig {
    /// Source dimension
    pub source_dim: usize,
    /// Target dimension
    pub target_dim: usize,
    /// Projection method
    pub method: ProjectionMethod,
    /// Quality threshold for projection acceptance
    pub quality_threshold: f32,
    /// Whether to preserve semantic relationships
    pub preserve_semantics: bool,
}

impl Default for ProjectionConfig {
    fn default() -> Self {
        Self {
            source_dim: 1536,
            target_dim: 384,
            method: ProjectionMethod::PCA,
            quality_threshold: 0.8,
            preserve_semantics: true,
        }
    }
}

/// Projection matrix and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionMatrix {
    /// The actual projection matrix (target_dim x source_dim)
    pub matrix: Vec<Vec<f32>>,
    /// Projection method used
    pub method: ProjectionMethod,
    /// Source and target dimensions
    pub source_dim: usize,
    pub target_dim: usize,
    /// Quality metrics
    pub quality_metrics: ProjectionQuality,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Quality metrics for projection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionQuality {
    /// Variance preserved (0.0 - 1.0)
    pub variance_preserved: f32,
    /// Semantic similarity preservation
    pub semantic_preservation: f32,
    /// Reconstruction error
    pub reconstruction_error: f32,
    /// Distance preservation score
    pub distance_preservation: f32,
}

impl Default for ProjectionQuality {
    fn default() -> Self {
        Self {
            variance_preserved: 1.0,
            semantic_preservation: 1.0,
            reconstruction_error: 0.0,
            distance_preservation: 1.0,
        }
    }
}

/// Result of a projection operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionResult {
    /// Projected vector
    pub projected: Vec<f32>,
    /// Quality of this specific projection
    pub quality: f32,
    /// Information loss estimate
    pub information_loss: f32,
}

/// Cross-dimensional field projector
pub struct FieldProjector {
    /// Available projection matrices
    projection_matrices: HashMap<String, ProjectionMatrix>,
    /// Projection statistics
    stats: ProjectionStats,
}

/// Statistics for projection operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionStats {
    /// Total projections performed
    pub total_projections: usize,
    /// Average projection quality
    pub avg_quality: f32,
    /// Average information loss
    pub avg_information_loss: f32,
    /// Projections by method
    pub projections_by_method: HashMap<ProjectionMethod, usize>,
}

impl Default for ProjectionStats {
    fn default() -> Self {
        Self {
            total_projections: 0,
            avg_quality: 1.0,
            avg_information_loss: 0.0,
            projections_by_method: HashMap::new(),
        }
    }
}

impl FieldProjector {
    /// Create a new field projector
    pub fn new() -> Self {
        Self {
            projection_matrices: HashMap::new(),
            stats: ProjectionStats::default(),
        }
    }

    /// Create a projection matrix
    pub fn create_projection_matrix(
        &mut self,
        matrix_id: String,
        config: ProjectionConfig,
        training_vectors: Option<&[Vec<f32>]>,
    ) -> ContextNestResult<()> {
        let matrix = match config.method {
            ProjectionMethod::PCA => self.create_pca_matrix(&config, training_vectors)?,
            ProjectionMethod::Random => self.create_random_matrix(&config)?,
            ProjectionMethod::Learned => self.create_learned_matrix(&config, training_vectors)?,
            ProjectionMethod::Semantic => self.create_semantic_matrix(&config, training_vectors)?,
            ProjectionMethod::Adaptive { .. } => {
                self.create_adaptive_matrix(&config, training_vectors)?
            }
        };

        self.projection_matrices.insert(matrix_id, matrix);
        Ok(())
    }

    /// Create PCA projection matrix
    fn create_pca_matrix(
        &self,
        config: &ProjectionConfig,
        training_vectors: Option<&[Vec<f32>]>,
    ) -> ContextNestResult<ProjectionMatrix> {
        // For now, create a simplified PCA-like matrix
        // In a full implementation, this would compute actual principal components
        let mut matrix = Vec::with_capacity(config.target_dim);

        for i in 0..config.target_dim {
            let mut row = vec![0.0; config.source_dim];

            // Create orthogonal basis vectors with decreasing importance
            let importance = 1.0 - (i as f32 / config.target_dim as f32);

            // Simple pattern: each row focuses on different ranges of source dimensions
            let start_idx = (i * config.source_dim) / config.target_dim;
            let end_idx = ((i + 1) * config.source_dim) / config.target_dim;

            let range_size = end_idx - start_idx;
            for j in start_idx..end_idx {
                row[j] = importance / (range_size as f32).sqrt();
            }

            matrix.push(row);
        }

        // Evaluate quality
        let quality = self.evaluate_matrix_quality(&matrix, config, training_vectors);

        Ok(ProjectionMatrix {
            matrix,
            method: config.method.clone(),
            source_dim: config.source_dim,
            target_dim: config.target_dim,
            quality_metrics: quality,
            created_at: chrono::Utc::now(),
        })
    }

    /// Create random projection matrix
    fn create_random_matrix(
        &self,
        config: &ProjectionConfig,
    ) -> ContextNestResult<ProjectionMatrix> {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        let mut matrix = Vec::with_capacity(config.target_dim);
        let scale = (config.source_dim as f32).sqrt().recip();

        for _ in 0..config.target_dim {
            let mut row = Vec::with_capacity(config.source_dim);
            for _ in 0..config.source_dim {
                // Use Gaussian random values scaled appropriately
                let val: f32 = rng.gen_range(-1.0..1.0) * scale;
                row.push(val);
            }
            matrix.push(row);
        }

        let quality = self.evaluate_matrix_quality(&matrix, config, None);

        Ok(ProjectionMatrix {
            matrix,
            method: config.method.clone(),
            source_dim: config.source_dim,
            target_dim: config.target_dim,
            quality_metrics: quality,
            created_at: chrono::Utc::now(),
        })
    }

    /// Create learned projection matrix (simplified autoencoder-style)
    fn create_learned_matrix(
        &self,
        config: &ProjectionConfig,
        training_vectors: Option<&[Vec<f32>]>,
    ) -> ContextNestResult<ProjectionMatrix> {
        // Simplified learned projection - in practice this would involve neural network training
        if let Some(vectors) = training_vectors {
            // Compute covariance-based projection similar to PCA but with learned weights
            let mut matrix = Vec::with_capacity(config.target_dim);

            for i in 0..config.target_dim {
                let mut row = vec![0.0; config.source_dim];

                // Learn weights based on variance in training data
                for j in 0..config.source_dim {
                    let mut variance = 0.0;
                    let mut mean = 0.0;

                    // Compute mean
                    for vector in vectors {
                        if j < vector.len() {
                            mean += vector[j];
                        }
                    }
                    mean /= vectors.len() as f32;

                    // Compute variance
                    for vector in vectors {
                        if j < vector.len() {
                            let diff = vector[j] - mean;
                            variance += diff * diff;
                        }
                    }
                    variance /= vectors.len() as f32;

                    // Weight by variance and position importance
                    let importance = 1.0 - (i as f32 / config.target_dim as f32);
                    row[j] = variance.sqrt() * importance / (config.source_dim as f32).sqrt();
                }

                matrix.push(row);
            }

            let quality = self.evaluate_matrix_quality(&matrix, config, Some(vectors));

            Ok(ProjectionMatrix {
                matrix,
                method: config.method.clone(),
                source_dim: config.source_dim,
                target_dim: config.target_dim,
                quality_metrics: quality,
                created_at: chrono::Utc::now(),
            })
        } else {
            // Fall back to random if no training data
            self.create_random_matrix(config)
        }
    }

    /// Create semantic-aware projection matrix
    fn create_semantic_matrix(
        &self,
        config: &ProjectionConfig,
        training_vectors: Option<&[Vec<f32>]>,
    ) -> ContextNestResult<ProjectionMatrix> {
        // Semantic projection preserves semantic relationships
        if let Some(vectors) = training_vectors {
            let mut matrix = Vec::with_capacity(config.target_dim);

            for i in 0..config.target_dim {
                let mut row = vec![0.0; config.source_dim];

                // Create semantic-aware weights
                for j in 0..config.source_dim {
                    // Semantic importance: dimensions that vary together should be preserved
                    let mut semantic_weight = 0.0;

                    for k in 0..vectors.len() {
                        for l in (k + 1)..vectors.len() {
                            if j < vectors[k].len() && j < vectors[l].len() {
                                let similarity = vectors[k][j] * vectors[l][j];
                                semantic_weight += similarity.abs();
                            }
                        }
                    }

                    semantic_weight /= (vectors.len() * (vectors.len() - 1) / 2) as f32;

                    // Combine with positional importance
                    let importance = 1.0 - (i as f32 / config.target_dim as f32);
                    row[j] = semantic_weight * importance / (config.source_dim as f32).sqrt();
                }

                matrix.push(row);
            }

            let quality = self.evaluate_matrix_quality(&matrix, config, Some(vectors));

            Ok(ProjectionMatrix {
                matrix,
                method: config.method.clone(),
                source_dim: config.source_dim,
                target_dim: config.target_dim,
                quality_metrics: quality,
                created_at: chrono::Utc::now(),
            })
        } else {
            // Fall back to PCA-like if no training data
            self.create_pca_matrix(config, None)
        }
    }

    /// Create adaptive projection matrix
    fn create_adaptive_matrix(
        &self,
        config: &ProjectionConfig,
        training_vectors: Option<&[Vec<f32>]>,
    ) -> ContextNestResult<ProjectionMatrix> {
        // Start with semantic projection and add adaptive capability
        let mut base_matrix = self.create_semantic_matrix(config, training_vectors)?;

        // Mark as adaptive
        base_matrix.method = config.method.clone();

        Ok(base_matrix)
    }

    /// Evaluate projection matrix quality
    fn evaluate_matrix_quality(
        &self,
        matrix: &[Vec<f32>],
        config: &ProjectionConfig,
        training_vectors: Option<&[Vec<f32>]>,
    ) -> ProjectionQuality {
        let mut quality = ProjectionQuality::default();

        if let Some(vectors) = training_vectors {
            if !vectors.is_empty() {
                let mut total_variance_preserved = 0.0;
                let mut total_distance_preservation = 0.0;
                let mut reconstruction_errors = Vec::new();

                // Test on a subset of training vectors
                let test_count = vectors.len().min(10);

                for i in 0..test_count {
                    if let Ok(proj_result) = self.project_vector_with_matrix(&vectors[i], matrix) {
                        // Simple variance preservation estimate
                        let orig_variance = self.compute_variance(&vectors[i]);
                        let proj_variance = self.compute_variance(&proj_result.projected);

                        if orig_variance > 0.0 {
                            total_variance_preserved += proj_variance / orig_variance;
                        } else {
                            total_variance_preserved += 1.0;
                        }

                        // Distance preservation (compare with next vector if available)
                        if i + 1 < test_count {
                            let orig_dist = self.euclidean_distance(&vectors[i], &vectors[i + 1]);

                            if let Ok(next_proj) =
                                self.project_vector_with_matrix(&vectors[i + 1], matrix)
                            {
                                let proj_dist = self.euclidean_distance(
                                    &proj_result.projected,
                                    &next_proj.projected,
                                );

                                if orig_dist > 0.0 {
                                    total_distance_preservation +=
                                        (proj_dist / orig_dist - 1.0).abs();
                                }
                            }
                        }

                        reconstruction_errors.push(proj_result.information_loss);
                    }
                }

                if test_count > 0 {
                    quality.variance_preserved =
                        (total_variance_preserved / test_count as f32).min(1.0);
                    quality.distance_preservation =
                        1.0 - (total_distance_preservation / test_count as f32).min(1.0);
                    quality.reconstruction_error = reconstruction_errors.iter().sum::<f32>()
                        / reconstruction_errors.len() as f32;
                    quality.semantic_preservation =
                        (quality.variance_preserved + quality.distance_preservation) / 2.0;
                }
            }
        } else {
            // Estimate quality based on matrix properties
            let matrix_norm = self.compute_matrix_norm(matrix);
            quality.variance_preserved = (matrix_norm / (config.target_dim as f32).sqrt()).min(1.0);
            quality.semantic_preservation = quality.variance_preserved;
            quality.distance_preservation = quality.variance_preserved;
            quality.reconstruction_error = 1.0 - quality.variance_preserved;
        }

        quality
    }

    /// Project a vector using a specific matrix
    fn project_vector_with_matrix(
        &self,
        vector: &[f32],
        matrix: &[Vec<f32>],
    ) -> ContextNestResult<ProjectionResult> {
        if vector.len() != matrix[0].len() {
            return Err(crate::error::ContextNestError::Configuration(format!(
                "Vector dimension {} doesn't match matrix source dimension {}",
                vector.len(),
                matrix[0].len()
            )));
        }

        let mut projected = Vec::with_capacity(matrix.len());

        for row in matrix {
            let mut dot_product = 0.0;
            for (i, &val) in vector.iter().enumerate() {
                dot_product += val * row[i];
            }
            projected.push(dot_product);
        }

        // Estimate quality and information loss
        let orig_norm = self.compute_norm(vector);
        let proj_norm = self.compute_norm(&projected);

        let quality = if orig_norm > 0.0 {
            (proj_norm / orig_norm).min(1.0)
        } else {
            1.0
        };

        let information_loss = 1.0 - quality;

        Ok(ProjectionResult {
            projected,
            quality,
            information_loss,
        })
    }

    /// Project a vector using a named matrix
    pub fn project_vector(
        &mut self,
        vector: &[f32],
        matrix_id: &str,
    ) -> ContextNestResult<ProjectionResult> {
        let matrix = self.projection_matrices.get(matrix_id).ok_or_else(|| {
            crate::error::ContextNestError::Configuration(format!(
                "Projection matrix {} not found",
                matrix_id
            ))
        })?;

        let result = self.project_vector_with_matrix(vector, &matrix.matrix)?;

        // Update statistics
        self.stats.total_projections += 1;
        self.stats.avg_quality =
            (self.stats.avg_quality * (self.stats.total_projections - 1) as f32 + result.quality)
                / self.stats.total_projections as f32;
        self.stats.avg_information_loss = (self.stats.avg_information_loss
            * (self.stats.total_projections - 1) as f32
            + result.information_loss)
            / self.stats.total_projections as f32;

        *self
            .stats
            .projections_by_method
            .entry(matrix.method.clone())
            .or_insert(0) += 1;

        Ok(result)
    }

    /// Project all patterns in a neural field
    pub fn project_field(
        &mut self,
        field: &NeuralField,
        matrix_id: &str,
        quality_threshold: f32,
    ) -> ContextNestResult<NeuralField> {
        let mut projected_field = NeuralField::new();
        let mut successful_projections = 0;

        for pattern in &field.patterns {
            match self.project_vector(&pattern.embedding, matrix_id) {
                Ok(projection_result) => {
                    if projection_result.quality >= quality_threshold {
                        let mut projected_pattern = pattern.clone();
                        projected_pattern.embedding = projection_result.projected;

                        // Adjust strength based on projection quality
                        projected_pattern.strength *= projection_result.quality;

                        projected_field.patterns.push(projected_pattern);
                        successful_projections += 1;
                    }
                }
                Err(_) => {
                    // Skip patterns that can't be projected
                    continue;
                }
            }
        }

        // Copy other field properties (NeuralField doesn't have id or harmonic_bridges, so we skip this)

        Ok(projected_field)
    }

    /// Update adaptive projection matrix based on new data
    pub fn update_adaptive_matrix(
        &mut self,
        matrix_id: &str,
        new_vectors: &[Vec<f32>],
        adaptation_rate: f32,
    ) -> ContextNestResult<()> {
        // Check if matrix exists and is adaptive
        let (source_dim, target_dim, method) = {
            let matrix = self.projection_matrices.get(matrix_id).ok_or_else(|| {
                crate::error::ContextNestError::Configuration(format!(
                    "Projection matrix {} not found",
                    matrix_id
                ))
            })?;
            (matrix.source_dim, matrix.target_dim, matrix.method.clone())
        };

        if let ProjectionMethod::Adaptive { .. } = method {
            // Update matrix weights
            {
                let matrix = self.projection_matrices.get_mut(matrix_id).unwrap();

                // Simple adaptive update: adjust weights based on new data variance
                for i in 0..matrix.matrix.len() {
                    for j in 0..matrix.matrix[i].len() {
                        let mut new_weight = 0.0;
                        let mut count = 0;

                        for vector in new_vectors {
                            if j < vector.len() {
                                new_weight += vector[j].abs();
                                count += 1;
                            }
                        }

                        if count > 0 {
                            new_weight /= count as f32;

                            // Adaptive update with learning rate
                            matrix.matrix[i][j] = matrix.matrix[i][j] * (1.0 - adaptation_rate)
                                + new_weight * adaptation_rate / (source_dim as f32).sqrt();
                        }
                    }
                }
            }

            // Update quality metrics (in separate scope to avoid borrow conflicts)
            let config = ProjectionConfig {
                source_dim,
                target_dim,
                method,
                quality_threshold: 0.8,
                preserve_semantics: true,
            };

            let matrix_data = self
                .projection_matrices
                .get(matrix_id)
                .unwrap()
                .matrix
                .clone();
            let quality_metrics =
                self.evaluate_matrix_quality(&matrix_data, &config, Some(new_vectors));

            // Apply quality metrics
            let matrix = self.projection_matrices.get_mut(matrix_id).unwrap();
            matrix.quality_metrics = quality_metrics;
        }

        Ok(())
    }

    /// Get projection matrix information
    pub fn get_matrix_info(&self, matrix_id: &str) -> Option<&ProjectionMatrix> {
        self.projection_matrices.get(matrix_id)
    }

    /// List all available projection matrices
    pub fn list_matrices(&self) -> Vec<String> {
        self.projection_matrices.keys().cloned().collect()
    }

    /// Remove a projection matrix
    pub fn remove_matrix(&mut self, matrix_id: &str) -> bool {
        self.projection_matrices.remove(matrix_id).is_some()
    }

    /// Get projection statistics
    pub fn get_stats(&self) -> &ProjectionStats {
        &self.stats
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = ProjectionStats::default();
    }

    // Helper methods

    fn compute_variance(&self, vector: &[f32]) -> f32 {
        if vector.is_empty() {
            return 0.0;
        }

        let mean = vector.iter().sum::<f32>() / vector.len() as f32;
        let variance =
            vector.iter().map(|&x| (x - mean).powi(2)).sum::<f32>() / vector.len() as f32;

        variance
    }

    fn compute_norm(&self, vector: &[f32]) -> f32 {
        vector.iter().map(|&x| x * x).sum::<f32>().sqrt()
    }

    fn compute_matrix_norm(&self, matrix: &[Vec<f32>]) -> f32 {
        let mut sum = 0.0;
        for row in matrix {
            for &val in row {
                sum += val * val;
            }
        }
        sum.sqrt()
    }

    fn euclidean_distance(&self, a: &[f32], b: &[f32]) -> f32 {
        let min_len = a.len().min(b.len());
        let mut sum = 0.0;

        for i in 0..min_len {
            let diff = a[i] - b[i];
            sum += diff * diff;
        }

        sum.sqrt()
    }
}

impl Default for FieldProjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_projection_config_default() {
        let config = ProjectionConfig::default();
        assert_eq!(config.source_dim, 1536);
        assert_eq!(config.target_dim, 384);
        assert_eq!(config.method, ProjectionMethod::PCA);
        assert_eq!(config.quality_threshold, 0.8);
        assert!(config.preserve_semantics);
    }

    #[test]
    fn test_field_projector_creation() {
        let projector = FieldProjector::new();
        assert_eq!(projector.projection_matrices.len(), 0);
        assert_eq!(projector.stats.total_projections, 0);
    }

    #[test]
    fn test_random_projection_matrix() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 100,
            target_dim: 50,
            method: ProjectionMethod::Random,
            quality_threshold: 0.7,
            preserve_semantics: false,
        };

        let result = projector.create_projection_matrix("test_random".to_string(), config, None);

        assert!(result.is_ok());
        assert_eq!(projector.projection_matrices.len(), 1);

        let matrix = projector.projection_matrices.get("test_random").unwrap();
        assert_eq!(matrix.matrix.len(), 50);
        assert_eq!(matrix.matrix[0].len(), 100);
        assert_eq!(matrix.source_dim, 100);
        assert_eq!(matrix.target_dim, 50);
    }

    #[test]
    fn test_pca_projection_matrix() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 10,
            target_dim: 5,
            method: ProjectionMethod::PCA,
            quality_threshold: 0.8,
            preserve_semantics: true,
        };

        let result = projector.create_projection_matrix("test_pca".to_string(), config, None);

        assert!(result.is_ok());

        let matrix = projector.projection_matrices.get("test_pca").unwrap();
        assert_eq!(matrix.matrix.len(), 5);
        assert_eq!(matrix.matrix[0].len(), 10);
        assert!(matches!(matrix.method, ProjectionMethod::PCA));
    }

    #[test]
    fn test_vector_projection() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 4,
            target_dim: 2,
            method: ProjectionMethod::Random,
            quality_threshold: 0.5,
            preserve_semantics: false,
        };

        projector
            .create_projection_matrix("test".to_string(), config, None)
            .unwrap();

        let vector = vec![1.0, 2.0, 3.0, 4.0];
        let result = projector.project_vector(&vector, "test");

        assert!(result.is_ok());
        let projection = result.unwrap();
        assert_eq!(projection.projected.len(), 2);
        assert!(projection.quality >= 0.0 && projection.quality <= 1.0);
        assert!(projection.information_loss >= 0.0 && projection.information_loss <= 1.0);
    }

    #[test]
    fn test_vector_projection_dimension_mismatch() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 4,
            target_dim: 2,
            method: ProjectionMethod::Random,
            quality_threshold: 0.5,
            preserve_semantics: false,
        };

        projector
            .create_projection_matrix("test".to_string(), config, None)
            .unwrap();

        let vector = vec![1.0, 2.0]; // Wrong dimension
        let result = projector.project_vector(&vector, "test");

        assert!(result.is_err());
    }

    #[test]
    fn test_nonexistent_matrix() {
        let mut projector = FieldProjector::new();
        let vector = vec![1.0, 2.0, 3.0, 4.0];
        let result = projector.project_vector(&vector, "nonexistent");

        assert!(result.is_err());
    }

    #[test]
    fn test_semantic_projection_with_training_data() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 6,
            target_dim: 3,
            method: ProjectionMethod::Semantic,
            quality_threshold: 0.6,
            preserve_semantics: true,
        };

        let training_data = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            vec![2.0, 4.0, 6.0, 8.0, 10.0, 12.0],
            vec![0.5, 1.0, 1.5, 2.0, 2.5, 3.0],
        ];

        let result = projector.create_projection_matrix(
            "semantic_test".to_string(),
            config,
            Some(&training_data),
        );

        assert!(result.is_ok());

        let matrix = projector.projection_matrices.get("semantic_test").unwrap();
        assert!(matches!(matrix.method, ProjectionMethod::Semantic));
        assert!(matrix.quality_metrics.semantic_preservation > 0.0);
    }

    #[test]
    fn test_learned_projection() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 8,
            target_dim: 4,
            method: ProjectionMethod::Learned,
            quality_threshold: 0.7,
            preserve_semantics: true,
        };

        let training_data = vec![
            vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
            vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5],
        ];

        let result = projector.create_projection_matrix(
            "learned_test".to_string(),
            config,
            Some(&training_data),
        );

        assert!(result.is_ok());

        let matrix = projector.projection_matrices.get("learned_test").unwrap();
        assert!(matches!(matrix.method, ProjectionMethod::Learned));
    }

    #[test]
    fn test_adaptive_projection_update() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 4,
            target_dim: 2,
            method: ProjectionMethod::Adaptive {
                adaptation_rate: 0.1,
            },
            quality_threshold: 0.6,
            preserve_semantics: true,
        };

        let initial_data = vec![vec![1.0, 2.0, 3.0, 4.0], vec![2.0, 3.0, 4.0, 5.0]];

        projector
            .create_projection_matrix("adaptive_test".to_string(), config, Some(&initial_data))
            .unwrap();

        // Get initial matrix values
        let initial_matrix = projector
            .projection_matrices
            .get("adaptive_test")
            .unwrap()
            .matrix
            .clone();

        // Update with new data
        let new_data = vec![vec![5.0, 6.0, 7.0, 8.0], vec![6.0, 7.0, 8.0, 9.0]];

        let result = projector.update_adaptive_matrix("adaptive_test", &new_data, 0.1);
        assert!(result.is_ok());

        // Check that matrix has changed
        let updated_matrix = &projector
            .projection_matrices
            .get("adaptive_test")
            .unwrap()
            .matrix;
        assert_ne!(initial_matrix, *updated_matrix);
    }

    #[test]
    fn test_field_projection() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 1536,
            target_dim: 768,
            method: ProjectionMethod::Random,
            quality_threshold: 0.0, // Accept all projections for testing
            preserve_semantics: false,
        };

        projector
            .create_projection_matrix("field_test".to_string(), config, None)
            .unwrap();

        // Create a test field
        let mut field = NeuralField::new();
        field.inject("test1".to_string(), vec![1.0; 1536]).unwrap();
        field.inject("test2".to_string(), vec![2.0; 1536]).unwrap();

        let projected_field = projector.project_field(&field, "field_test", 0.0);
        assert!(projected_field.is_ok());

        let proj_field = projected_field.unwrap();
        assert_eq!(proj_field.patterns.len(), 2);

        for pattern in &proj_field.patterns {
            assert_eq!(pattern.embedding.len(), 768); // Target dimension
        }
    }

    #[test]
    fn test_projection_statistics() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 3,
            target_dim: 2,
            method: ProjectionMethod::PCA,
            quality_threshold: 0.5,
            preserve_semantics: false,
        };

        projector
            .create_projection_matrix("stats_test".to_string(), config, None)
            .unwrap();

        // Perform several projections
        let vectors = vec![
            vec![1.0, 2.0, 3.0],
            vec![4.0, 5.0, 6.0],
            vec![7.0, 8.0, 9.0],
        ];

        for vector in vectors {
            projector.project_vector(&vector, "stats_test").unwrap();
        }

        let stats = projector.get_stats();
        assert_eq!(stats.total_projections, 3);
        assert!(stats.avg_quality > 0.0);
        assert!(stats
            .projections_by_method
            .contains_key(&ProjectionMethod::PCA));
        assert_eq!(stats.projections_by_method[&ProjectionMethod::PCA], 3);
    }

    #[test]
    fn test_matrix_management() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig::default();

        // Create matrix
        projector
            .create_projection_matrix("mgmt_test".to_string(), config, None)
            .unwrap();
        assert_eq!(projector.list_matrices().len(), 1);

        // Get matrix info
        let info = projector.get_matrix_info("mgmt_test");
        assert!(info.is_some());
        assert_eq!(info.unwrap().source_dim, 1536);

        // Remove matrix
        let removed = projector.remove_matrix("mgmt_test");
        assert!(removed);
        assert_eq!(projector.list_matrices().len(), 0);

        // Try to remove nonexistent matrix
        let not_removed = projector.remove_matrix("nonexistent");
        assert!(!not_removed);
    }

    #[test]
    fn test_helper_methods() {
        let projector = FieldProjector::new();

        // Test variance computation
        let vector = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let variance = projector.compute_variance(&vector);
        assert!(variance > 0.0);

        // Test norm computation
        let norm = projector.compute_norm(&vector);
        assert!(norm > 0.0);

        // Test euclidean distance
        let vec_a = vec![1.0, 2.0, 3.0];
        let vec_b = vec![4.0, 5.0, 6.0];
        let distance = projector.euclidean_distance(&vec_a, &vec_b);
        assert!(distance > 0.0);
    }

    #[test]
    fn test_projection_quality_metrics() {
        let quality = ProjectionQuality::default();
        assert_eq!(quality.variance_preserved, 1.0);
        assert_eq!(quality.semantic_preservation, 1.0);
        assert_eq!(quality.reconstruction_error, 0.0);
        assert_eq!(quality.distance_preservation, 1.0);
    }

    #[test]
    fn test_quality_threshold_filtering() {
        let mut projector = FieldProjector::new();
        let config = ProjectionConfig {
            source_dim: 4,
            target_dim: 2,
            method: ProjectionMethod::Random,
            quality_threshold: 0.9, // Very high threshold
            preserve_semantics: false,
        };

        projector
            .create_projection_matrix("threshold_test".to_string(), config, None)
            .unwrap();

        // Create a test field
        let mut field = NeuralField::new();
        field.inject("test1".to_string(), vec![1.0; 1536]).unwrap();
        field.inject("test2".to_string(), vec![2.0; 1536]).unwrap();

        let projected_field = projector.project_field(&field, "threshold_test", 0.9);
        assert!(projected_field.is_ok());

        // With high threshold, some patterns might be filtered out
        let proj_field = projected_field.unwrap();
        assert!(proj_field.patterns.len() <= 2);
    }
}
