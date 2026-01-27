//! Temporal Pattern Recognition Module
//! Provides temporal pattern recognition capabilities for analyzing
//! time-series data and sequential patterns.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use serde::{Deserialize, Serialize};

/// Temporal pattern descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalPattern {
    /// Pattern identifier
    pub id: String,
    /// Pattern type
    pub pattern_type: TemporalPatternType,
    /// Time window (in milliseconds)
    pub time_window: u64,
    /// Pattern strength
    pub strength: f32,
    /// Frequency of occurrence
    pub frequency: f32,
    /// Pattern metadata
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Types of temporal patterns
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TemporalPatternType {
    /// Periodic patterns
    Periodic,
    /// Trend patterns
    Trend,
    /// Seasonal patterns
    Seasonal,
    /// Anomaly patterns
    Anomaly,
    /// Burst patterns
    Burst,
}

/// Temporal pattern detector
pub struct TemporalPatternDetector {
    /// Minimum pattern strength
    min_strength: f32,
    /// Maximum time window to consider (ms)
    max_time_window: u64,
    /// Minimum samples for pattern detection
    min_samples: usize,
}

impl TemporalPatternDetector {
    /// Create a new temporal pattern detector
    pub fn new() -> Self {
        Self {
            min_strength: 0.3,
            max_time_window: 86400000, // 24 hours
            min_samples: 10,
        }
    }

    /// Detect temporal patterns in time series data
    pub fn detect_patterns(
        &self,
        timestamps: &[u64],
        values: &[f32],
    ) -> ContextNestResult<Vec<TemporalPattern>> {
        if timestamps.len() != values.len() {
            return Err(ContextNestError::Configuration(
                "Timestamps and values must have the same length".to_string(),
            ));
        }

        if timestamps.len() < self.min_samples {
            return Ok(Vec::new());
        }

        let mut patterns = Vec::new();

        // Detect periodic patterns
        patterns.extend(self.detect_periodic_patterns(timestamps, values)?);

        // Detect trend patterns
        patterns.extend(self.detect_trend_patterns(timestamps, values)?);

        // Detect anomaly patterns
        patterns.extend(self.detect_anomaly_patterns(timestamps, values)?);

        // Filter by strength
        patterns.retain(|p| p.strength >= self.min_strength);

        Ok(patterns)
    }

    /// Detect periodic patterns
    fn detect_periodic_patterns(
        &self,
        timestamps: &[u64],
        values: &[f32],
    ) -> ContextNestResult<Vec<TemporalPattern>> {
        let mut patterns = Vec::new();

        if timestamps.len() < 6 {
            return Ok(patterns);
        }

        // Calculate time intervals between consecutive points
        let intervals: Vec<u64> = timestamps.windows(2).map(|w| w[1] - w[0]).collect();

        // Find common intervals
        let interval_counts = self.count_common_intervals(&intervals);
        for (interval, count) in interval_counts {
            if count >= 3 && interval <= self.max_time_window {
                let frequency = count as f32 / timestamps.len() as f32;
                patterns.push(TemporalPattern {
                    id: format!("periodic_{}", interval),
                    pattern_type: TemporalPatternType::Periodic,
                    time_window: interval,
                    strength: frequency,
                    frequency,
                    metadata: std::collections::HashMap::from([(
                        "occurrences".to_string(),
                        serde_json::Value::Number(count.into()),
                    )]),
                });
            }
        }

        Ok(patterns)
    }

    /// Detect trend patterns
    fn detect_trend_patterns(
        &self,
        timestamps: &[u64],
        values: &[f32],
    ) -> ContextNestResult<Vec<TemporalPattern>> {
        let mut patterns = Vec::new();

        if timestamps.len() < 5 {
            return Ok(patterns);
        }

        // Calculate overall trend
        let trend = self.calculate_linear_trend(values);
        let trend_strength = trend.abs();

        if trend_strength >= self.min_strength {
            let time_window = timestamps[timestamps.len() - 1] - timestamps[0];
            patterns.push(TemporalPattern {
                id: format!(
                    "trend_{:?}",
                    if trend > 0.0 {
                        "increasing"
                    } else {
                        "decreasing"
                    }
                ),
                pattern_type: TemporalPatternType::Trend,
                time_window,
                strength: trend_strength,
                frequency: 1.0, // Single trend over the period
                metadata: std::collections::HashMap::from([
                    (
                        "slope".to_string(),
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(trend as f64).unwrap(),
                        ),
                    ),
                    (
                        "direction".to_string(),
                        serde_json::Value::String(if trend > 0.0 {
                            "increasing".to_string()
                        } else {
                            "decreasing".to_string()
                        }),
                    ),
                ]),
            });
        }

        Ok(patterns)
    }

    /// Detect anomaly patterns
    fn detect_anomaly_patterns(
        &self,
        timestamps: &[u64],
        values: &[f32],
    ) -> ContextNestResult<Vec<TemporalPattern>> {
        let mut patterns = Vec::new();

        if values.len() < 10 {
            return Ok(patterns);
        }

        // Calculate mean and standard deviation
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32;
        let std_dev = variance.sqrt();

        // Find anomalies (values > 2 standard deviations from mean)
        let anomaly_threshold = 2.0 * std_dev;
        let mut anomaly_count = 0;
        let mut anomaly_positions = Vec::new();

        for (i, &value) in values.iter().enumerate() {
            if (value - mean).abs() > anomaly_threshold {
                anomaly_count += 1;
                anomaly_positions.push(i);
            }
        }

        if anomaly_count > 0 {
            let anomaly_frequency = anomaly_count as f32 / values.len() as f32;
            patterns.push(TemporalPattern {
                id: "anomaly_pattern".to_string(),
                pattern_type: TemporalPatternType::Anomaly,
                time_window: self.max_time_window,
                strength: anomaly_frequency,
                frequency: anomaly_frequency,
                metadata: std::collections::HashMap::from([
                    (
                        "anomaly_count".to_string(),
                        serde_json::Value::Number(anomaly_count.into()),
                    ),
                    (
                        "mean".to_string(),
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(mean as f64).unwrap(),
                        ),
                    ),
                    (
                        "std_dev".to_string(),
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(std_dev as f64).unwrap(),
                        ),
                    ),
                    (
                        "anomaly_positions".to_string(),
                        serde_json::Value::Array(
                            anomaly_positions
                                .into_iter()
                                .map(|p| serde_json::Value::Number(p.into()))
                                .collect(),
                        ),
                    ),
                ]),
            });
        }

        Ok(patterns)
    }

    /// Count common time intervals
    fn count_common_intervals(&self, intervals: &[u64]) -> std::collections::HashMap<u64, usize> {
        let mut counts = std::collections::HashMap::new();

        // Group intervals by approximate equality (within 10% tolerance)
        for &interval in intervals {
            let mut found_match = false;
            for (&existing_interval, count) in counts.iter_mut() {
                let tolerance = existing_interval / 10; // 10% tolerance
                if (interval as i64 - existing_interval as i64).abs() <= tolerance as i64 {
                    *count += 1;
                    found_match = true;
                    break;
                }
            }

            if !found_match {
                counts.insert(interval, 1);
            }
        }

        counts
    }

    /// Calculate linear trend
    fn calculate_linear_trend(&self, values: &[f32]) -> f32 {
        if values.len() < 2 {
            return 0.0;
        }

        let n = values.len() as f32;
        let sum_x: f32 = (0..values.len()).map(|i| i as f32).sum();
        let sum_y: f32 = values.iter().sum();
        let sum_xy: f32 = values.iter().enumerate().map(|(i, &y)| i as f32 * y).sum();
        let sum_x2: f32 = (0..values.len()).map(|i| (i as f32).powi(2)).sum();

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x.powi(2));
        slope
    }

    /// Set minimum pattern strength
    pub fn set_min_strength(&mut self, min_strength: f32) {
        self.min_strength = min_strength.clamp(0.0, 1.0);
    }

    /// Set maximum time window
    pub fn set_max_time_window(&mut self, max_time_window: u64) {
        self.max_time_window = max_time_window;
    }

    /// Set minimum samples
    pub fn set_min_samples(&mut self, min_samples: usize) {
        self.min_samples = min_samples;
    }
}

impl Default for TemporalPatternDetector {
    fn default() -> Self {
        Self::new()
    }
}
