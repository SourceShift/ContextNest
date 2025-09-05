use crate::context::field::{FieldHealth, NeuralField};
use crate::context::memory::AttractorField;
use crate::context::meta_recursive::MetaRecursiveEngine;
use crate::protocols::ProtocolRegistry;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Comprehensive Context Engineering metrics system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEngineeringMetrics {
    pub measurement_timestamp: DateTime<Utc>,
    pub field_metrics: NeuralFieldMetrics,
    pub memory_metrics: MemoryMetrics,
    pub protocol_metrics: ProtocolMetrics,
    pub meta_recursive_metrics: MetaRecursiveMetrics,
    pub system_metrics: SystemMetrics,
    pub performance_trends: PerformanceTrends,
}

/// Neural field performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralFieldMetrics {
    pub pattern_count: usize,
    pub average_pattern_strength: f32,
    pub average_resonance: f32,
    pub field_coherence: f32,
    pub pattern_diversity: f32,
    pub resonance_stability: f32,
    pub activation_efficiency: f32,
    pub decay_rate_distribution: DecayRateDistribution,
    pub health_status: FieldHealth,
    pub repair_frequency: f32,
    pub self_healing_rate: f32,
}

/// Memory system performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub attractor_count: usize,
    pub average_attractor_strength: f32,
    pub memory_utilization: f32,
    pub memory_efficiency: f32,
    pub persistence_rate: f32,
    pub retrieval_accuracy: f32,
    pub connection_density: f32,
    pub adaptive_decay_effectiveness: f32,
    pub attractor_formation_rate: f32,
    pub memory_fragmentation: f32,
}

/// Protocol system performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMetrics {
    pub registered_protocols: usize,
    pub total_executions: u64,
    pub success_rate: f32,
    pub average_execution_time: f32,
    pub protocol_efficiency: HashMap<String, f32>,
    pub lineage_integrity: f32,
    pub execution_reliability: f32,
    pub resource_utilization: f32,
}

/// Meta-recursive system metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaRecursiveMetrics {
    pub enhancement_count: usize,
    pub enhancement_success_rate: f32,
    pub average_improvement: f32,
    pub emergence_detection_rate: f32,
    pub self_modification_frequency: f32,
    pub recursive_depth_utilization: f32,
    pub stability_maintenance: f32,
    pub learning_velocity: f32,
}

/// Overall system health and performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub overall_health_score: f32,
    pub component_synergy: f32,
    pub cross_component_efficiency: f32,
    pub system_stability: f32,
    pub adaptation_rate: f32,
    pub cognitive_load: f32,
    pub processing_throughput: f32,
    pub error_rate: f32,
}

/// Performance trends over time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceTrends {
    pub field_coherence_trend: TrendData,
    pub memory_efficiency_trend: TrendData,
    pub protocol_success_trend: TrendData,
    pub enhancement_velocity_trend: TrendData,
    pub overall_performance_trend: TrendData,
}

/// Trend data for metrics over time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendData {
    pub current_value: f32,
    pub trend_direction: TrendDirection,
    pub change_rate: f32,
    pub historical_average: f32,
    pub volatility: f32,
}

/// Trend direction enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrendDirection {
    Improving,
    Stable,
    Declining,
    Volatile,
}

/// Decay rate distribution analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayRateDistribution {
    pub low_decay_patterns: usize,    // decay < 0.01
    pub medium_decay_patterns: usize, // 0.01 <= decay < 0.05
    pub high_decay_patterns: usize,   // decay >= 0.05
    pub average_decay_rate: f32,
    pub decay_variance: f32,
}

/// Context Engineering metrics collector
pub struct ContextMetricsCollector {
    historical_data: Vec<ContextEngineeringMetrics>,
    collection_interval_seconds: u64,
    max_history_length: usize,
}

impl ContextMetricsCollector {
    pub fn new(collection_interval_seconds: u64, max_history_length: usize) -> Self {
        Self {
            historical_data: Vec::new(),
            collection_interval_seconds,
            max_history_length,
        }
    }

    /// Collect comprehensive metrics from all Context Engineering components
    pub fn collect_metrics(
        &mut self,
        field: &NeuralField,
        memory: &AttractorField,
        protocols: &ProtocolRegistry,
        meta_engine: &MetaRecursiveEngine,
    ) -> ContextEngineeringMetrics {
        let timestamp = Utc::now();

        let field_metrics = self.collect_field_metrics(field);
        let memory_metrics = self.collect_memory_metrics(memory);
        let protocol_metrics = self.collect_protocol_metrics(protocols);
        let meta_recursive_metrics = self.collect_meta_recursive_metrics(meta_engine);
        let system_metrics = self.calculate_system_metrics(
            &field_metrics,
            &memory_metrics,
            &protocol_metrics,
            &meta_recursive_metrics,
        );
        let performance_trends = self.calculate_performance_trends(
            &field_metrics,
            &memory_metrics,
            &protocol_metrics,
            &meta_recursive_metrics,
        );

        let metrics = ContextEngineeringMetrics {
            measurement_timestamp: timestamp,
            field_metrics,
            memory_metrics,
            protocol_metrics,
            meta_recursive_metrics,
            system_metrics,
            performance_trends,
        };

        // Store in history
        self.historical_data.push(metrics.clone());
        if self.historical_data.len() > self.max_history_length {
            self.historical_data.remove(0);
        }

        metrics
    }

    /// Collect neural field specific metrics
    fn collect_field_metrics(&self, field: &NeuralField) -> NeuralFieldMetrics {
        let pattern_count = field.patterns.len();

        let (avg_strength, avg_resonance, decay_distribution) = if pattern_count > 0 {
            let total_strength: f32 = field.patterns.iter().map(|p| p.strength).sum();
            let total_resonance: f32 = field.patterns.iter().map(|p| p.resonance).sum();

            let avg_strength = total_strength / pattern_count as f32;
            let avg_resonance = total_resonance / pattern_count as f32;

            // Analyze decay rate distribution
            let mut low_decay = 0;
            let mut medium_decay = 0;
            let mut high_decay = 0;
            let total_decay: f32 = field.patterns.iter().map(|p| p.decay_rate).sum();
            let avg_decay = total_decay / pattern_count as f32;

            for pattern in &field.patterns {
                if pattern.decay_rate < 0.01 {
                    low_decay += 1;
                } else if pattern.decay_rate < 0.05 {
                    medium_decay += 1;
                } else {
                    high_decay += 1;
                }
            }

            let decay_variance = field
                .patterns
                .iter()
                .map(|p| (p.decay_rate - avg_decay).powi(2))
                .sum::<f32>()
                / pattern_count as f32;

            let decay_distribution = DecayRateDistribution {
                low_decay_patterns: low_decay,
                medium_decay_patterns: medium_decay,
                high_decay_patterns: high_decay,
                average_decay_rate: avg_decay,
                decay_variance,
            };

            (avg_strength, avg_resonance, decay_distribution)
        } else {
            let decay_distribution = DecayRateDistribution {
                low_decay_patterns: 0,
                medium_decay_patterns: 0,
                high_decay_patterns: 0,
                average_decay_rate: 0.0,
                decay_variance: 0.0,
            };
            (0.0, 0.0, decay_distribution)
        };

        // Calculate field coherence
        let coherence_analysis = field.detect_field_coherence();
        let field_coherence = coherence_analysis.overall_coherence;

        // Calculate pattern diversity (normalized entropy)
        let pattern_diversity = self.calculate_pattern_diversity(field);

        // Calculate resonance stability
        let resonance_stability = self.calculate_resonance_stability(field);

        // Calculate activation efficiency
        let activation_efficiency = self.calculate_activation_efficiency(field);

        // Get health status
        let coherence_analysis = field.detect_field_coherence();
        let health_status = if coherence_analysis.overall_coherence > 0.8 {
            FieldHealth::Excellent
        } else if coherence_analysis.overall_coherence > 0.6 {
            FieldHealth::Good
        } else if coherence_analysis.overall_coherence > 0.4 {
            FieldHealth::Fair
        } else if coherence_analysis.overall_coherence > 0.2 {
            FieldHealth::Poor
        } else {
            FieldHealth::Critical
        };

        NeuralFieldMetrics {
            pattern_count,
            average_pattern_strength: avg_strength,
            average_resonance: avg_resonance,
            field_coherence,
            pattern_diversity,
            resonance_stability,
            activation_efficiency,
            decay_rate_distribution: decay_distribution,
            health_status,
            // placeholder — these scalars were previously set to fake
            // non-zero values (0.1 / 0.8) which made dashboards/health
            // checks look like a measurement was happening. Set to 0.0
            // until real tracking lands (track via
            // ). Consumers should treat 0.0 as
            // "no signal" for these fields.
            repair_frequency: 0.0,
            self_healing_rate: 0.0,
        }
    }

    /// Collect memory system specific metrics
    fn collect_memory_metrics(&self, memory: &AttractorField) -> MemoryMetrics {
        let attractor_count = memory.attractors.len();

        let (avg_strength, connection_density, fragmentation) = if attractor_count > 0 {
            let total_strength: f32 = memory.attractors.iter().map(|(_, a)| a.strength).sum();
            let avg_strength = total_strength / attractor_count as f32;

            // Calculate connection density
            let total_connections: usize = memory
                .attractors
                .iter()
                .map(|(_, a)| a.connections.len())
                .sum();
            let max_possible_connections = attractor_count * (attractor_count - 1);
            let connection_density = if max_possible_connections > 0 {
                total_connections as f32 / max_possible_connections as f32
            } else {
                0.0
            };

            // Calculate memory fragmentation (how scattered attractors are)
            let fragmentation = self.calculate_memory_fragmentation(memory);

            (avg_strength, connection_density, fragmentation)
        } else {
            (0.0, 0.0, 0.0)
        };

        let memory_utilization = memory.calculate_utilization_metrics();
        let memory_efficiency = memory.calculate_efficiency_metrics();

        MemoryMetrics {
            attractor_count,
            average_attractor_strength: avg_strength,
            memory_utilization,
            memory_efficiency,
            // placeholder — set to 0.0 (no-signal) instead of the
            // previously-fake 0.85/0.92/0.78/0.15 values. Real tracking
            // tracked in
            persistence_rate: 0.0,
            retrieval_accuracy: 0.0,
            connection_density,
            adaptive_decay_effectiveness: 0.0,
            attractor_formation_rate: 0.0,
            memory_fragmentation: fragmentation,
        }
    }

    /// Collect protocol system specific metrics
    fn collect_protocol_metrics(&self, protocols: &ProtocolRegistry) -> ProtocolMetrics {
        let registered_protocols = protocols.protocols.len();
        let total_executions = protocols.execution_history.len() as u64;

        // Calculate aggregate metrics from individual protocol stats
        let mut total_success_rate = 0.0;
        let mut total_execution_time = 0.0;
        let mut protocol_efficiency = HashMap::new();

        for protocol_name in protocols.protocols.keys() {
            if let Some(stats) = protocols.get_protocol_stats(protocol_name) {
                total_success_rate += stats.success_rate;
                total_execution_time += stats.average_execution_time_ms as f64;

                // Calculate efficiency as success_rate / execution_time
                let efficiency = if stats.average_execution_time_ms > 0 {
                    stats.success_rate as f64 / stats.average_execution_time_ms as f64 * 1000.0
                // normalize to per second
                } else {
                    0.0
                };
                protocol_efficiency.insert(protocol_name.clone(), efficiency);
            }
        }

        let success_rate = if registered_protocols > 0 {
            total_success_rate / registered_protocols as f64
        } else {
            0.0
        };

        let average_execution_time = if registered_protocols > 0 {
            total_execution_time / registered_protocols as f64
        } else {
            0.0
        };

        ProtocolMetrics {
            registered_protocols,
            total_executions,
            success_rate: success_rate as f32,
            average_execution_time: average_execution_time as f32,
            protocol_efficiency: protocol_efficiency
                .into_iter()
                .map(|(k, v)| (k, v as f32))
                .collect(),
            // placeholder — set to 0.0 (no-signal) instead of the
            // previously-fake 0.95/0.65 values.
            lineage_integrity: 0.0,
            execution_reliability: success_rate as f32,
            resource_utilization: 0.0,
        }
    }

    /// Collect meta-recursive system specific metrics
    fn collect_meta_recursive_metrics(
        &self,
        meta_engine: &MetaRecursiveEngine,
    ) -> MetaRecursiveMetrics {
        let enhancement_count = meta_engine.enhancement_history.len();

        let (success_rate, avg_improvement) = if enhancement_count > 0 {
            let successful = meta_engine
                .enhancement_history
                .iter()
                .filter(|e| e.effectiveness > 0.5)
                .count();
            let success_rate = successful as f32 / enhancement_count as f32;

            let total_improvement: f32 = meta_engine
                .enhancement_history
                .iter()
                .map(|e| e.effectiveness)
                .sum();
            let avg_improvement = total_improvement / enhancement_count as f32;

            (success_rate, avg_improvement)
        } else {
            (0.0, 0.0)
        };

        let emergence_detection_rate =
            meta_engine.emergence_patterns.len() as f32 / (enhancement_count.max(1) as f32); // patterns per enhancement cycle

        let recursive_depth_utilization =
            meta_engine.recursive_depth as f32 / meta_engine.max_recursive_depth as f32;

        MetaRecursiveMetrics {
            enhancement_count,
            enhancement_success_rate: success_rate,
            average_improvement: avg_improvement,
            emergence_detection_rate,
            self_modification_frequency: meta_engine.self_modification_rules.len() as f32 / 100.0, // normalized
            recursive_depth_utilization,
            // placeholder — set to 0.0 (no-signal).
            stability_maintenance: 0.0,
            learning_velocity: meta_engine.enhancement_metrics.average_improvement,
        }
    }

    /// Calculate overall system health and performance metrics
    fn calculate_system_metrics(
        &self,
        field: &NeuralFieldMetrics,
        memory: &MemoryMetrics,
        protocols: &ProtocolMetrics,
        meta_recursive: &MetaRecursiveMetrics,
    ) -> SystemMetrics {
        // Overall health score combines component health
        let overall_health_score = field.field_coherence * 0.3
            + memory.memory_efficiency * 0.25
            + protocols.success_rate / 100.0 * 0.25
            + meta_recursive.enhancement_success_rate * 0.2;

        // Component synergy measures how well components work together
        let component_synergy = (field.resonance_stability
            * memory.memory_utilization
            * protocols.execution_reliability
            * meta_recursive.stability_maintenance)
            .powf(0.25); // geometric mean

        // Cross-component efficiency
        let cross_component_efficiency = (field.activation_efficiency
            + memory.memory_efficiency
            + protocols.resource_utilization
            + meta_recursive.learning_velocity)
            / 4.0;

        // System stability
        let system_stability = field.resonance_stability * 0.3
            + memory.persistence_rate * 0.3
            + protocols.lineage_integrity * 0.2
            + meta_recursive.stability_maintenance * 0.2;

        SystemMetrics {
            overall_health_score,
            component_synergy,
            cross_component_efficiency,
            system_stability,
            adaptation_rate: meta_recursive.self_modification_frequency,
            cognitive_load: 1.0 - cross_component_efficiency, // inverse of efficiency
            processing_throughput: protocols.resource_utilization,
            error_rate: 1.0 - protocols.success_rate / 100.0,
        }
    }

    /// Calculate performance trends based on historical data
    fn calculate_performance_trends(
        &self,
        field: &NeuralFieldMetrics,
        memory: &MemoryMetrics,
        protocols: &ProtocolMetrics,
        meta_recursive: &MetaRecursiveMetrics,
    ) -> PerformanceTrends {
        let field_coherence_trend = self.calculate_trend_data(
            field.field_coherence,
            &self
                .historical_data
                .iter()
                .map(|m| m.field_metrics.field_coherence)
                .collect::<Vec<_>>(),
        );

        let memory_efficiency_trend = self.calculate_trend_data(
            memory.memory_efficiency,
            &self
                .historical_data
                .iter()
                .map(|m| m.memory_metrics.memory_efficiency)
                .collect::<Vec<_>>(),
        );

        let protocol_success_trend = self.calculate_trend_data(
            protocols.success_rate,
            &self
                .historical_data
                .iter()
                .map(|m| m.protocol_metrics.success_rate)
                .collect::<Vec<_>>(),
        );

        let enhancement_velocity_trend = self.calculate_trend_data(
            meta_recursive.learning_velocity,
            &self
                .historical_data
                .iter()
                .map(|m| m.meta_recursive_metrics.learning_velocity)
                .collect::<Vec<_>>(),
        );

        let overall_performance = (field.field_coherence
            + memory.memory_efficiency
            + protocols.success_rate / 100.0
            + meta_recursive.average_improvement)
            / 4.0;
        let overall_performance_trend = self.calculate_trend_data(
            overall_performance,
            &self
                .historical_data
                .iter()
                .map(|m| {
                    (m.field_metrics.field_coherence
                        + m.memory_metrics.memory_efficiency
                        + m.protocol_metrics.success_rate / 100.0
                        + m.meta_recursive_metrics.average_improvement)
                        / 4.0
                })
                .collect::<Vec<_>>(),
        );

        PerformanceTrends {
            field_coherence_trend,
            memory_efficiency_trend,
            protocol_success_trend,
            enhancement_velocity_trend,
            overall_performance_trend,
        }
    }

    /// Calculate trend data for a specific metric
    fn calculate_trend_data(&self, current_value: f32, historical_values: &[f32]) -> TrendData {
        if historical_values.len() < 2 {
            return TrendData {
                current_value,
                trend_direction: TrendDirection::Stable,
                change_rate: 0.0,
                historical_average: current_value,
                volatility: 0.0,
            };
        }

        let historical_average =
            historical_values.iter().sum::<f32>() / historical_values.len() as f32;

        // Calculate recent trend (last 5 measurements)
        let recent_values = &historical_values[historical_values.len().saturating_sub(5)..];
        let change_rate = if recent_values.len() >= 2 {
            (current_value - recent_values[0]) / recent_values.len() as f32
        } else {
            0.0
        };

        let trend_direction = if change_rate.abs() < 0.01 {
            TrendDirection::Stable
        } else if change_rate > 0.0 {
            TrendDirection::Improving
        } else {
            TrendDirection::Declining
        };

        // Calculate volatility as standard deviation
        let variance = historical_values
            .iter()
            .map(|v| (v - historical_average).powi(2))
            .sum::<f32>()
            / historical_values.len() as f32;
        let volatility = variance.sqrt();

        TrendData {
            current_value,
            trend_direction,
            change_rate,
            historical_average,
            volatility,
        }
    }

    /// Calculate pattern diversity using normalized entropy
    fn calculate_pattern_diversity(&self, field: &NeuralField) -> f32 {
        if field.patterns.is_empty() {
            return 0.0;
        }

        // Group patterns by strength ranges for diversity calculation
        let mut strength_buckets = [0; 10];
        for pattern in &field.patterns {
            let bucket = ((pattern.strength * 10.0) as usize).min(9);
            strength_buckets[bucket] += 1;
        }

        // Calculate entropy
        let total = field.patterns.len() as f32;
        let entropy = strength_buckets
            .iter()
            .filter(|&&count| count > 0)
            .map(|&count| {
                let p = count as f32 / total;
                -p * p.log2()
            })
            .sum::<f32>();

        // Normalize by maximum possible entropy
        let max_entropy = (strength_buckets.len() as f32).log2();
        if max_entropy > 0.0 {
            entropy / max_entropy
        } else {
            0.0
        }
    }

    /// Calculate resonance stability
    fn calculate_resonance_stability(&self, field: &NeuralField) -> f32 {
        if field.patterns.len() < 2 {
            return 1.0;
        }

        let resonances: Vec<f32> = field.patterns.iter().map(|p| p.resonance).collect();
        let mean = resonances.iter().sum::<f32>() / resonances.len() as f32;
        let variance =
            resonances.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / resonances.len() as f32;

        // Convert variance to stability (lower variance = higher stability)
        (1.0 / (1.0 + variance)).min(1.0)
    }

    /// Calculate activation efficiency
    fn calculate_activation_efficiency(&self, field: &NeuralField) -> f32 {
        if field.patterns.is_empty() {
            return 0.0;
        }

        // Efficiency = average(strength * resonance / activation_count)
        let efficiency_sum: f32 = field
            .patterns
            .iter()
            .map(|p| {
                let base_efficiency = p.strength * p.resonance;
                let activation_factor = 1.0 / (1.0 + p.activation_count as f32 * 0.1);
                base_efficiency * activation_factor
            })
            .sum();

        efficiency_sum / field.patterns.len() as f32
    }

    /// Calculate memory fragmentation
    fn calculate_memory_fragmentation(&self, memory: &AttractorField) -> f32 {
        if memory.attractors.len() < 2 {
            return 0.0;
        }

        // Calculate average distance between attractor centers
        let mut total_distance = 0.0;
        let mut count = 0;

        let attractor_values: Vec<&crate::context::memory::MemoryAttractor> =
            memory.attractors.values().collect();
        for i in 0..attractor_values.len() {
            for j in i + 1..attractor_values.len() {
                let distance = self.calculate_vector_distance(
                    &attractor_values[i].center,
                    &attractor_values[j].center,
                );
                total_distance += distance;
                count += 1;
            }
        }

        let average_distance = if count > 0 {
            total_distance / count as f32
        } else {
            0.0
        };

        // Normalize fragmentation (higher distance = more fragmentation)
        (average_distance / 2.0).min(1.0) // assuming max distance is ~2.0
    }

    /// Calculate Euclidean distance between two vectors
    fn calculate_vector_distance(&self, v1: &[f32], v2: &[f32]) -> f32 {
        if v1.len() != v2.len() {
            return 0.0;
        }

        v1.iter()
            .zip(v2.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Get historical metrics data
    pub fn get_historical_data(&self) -> &[ContextEngineeringMetrics] {
        &self.historical_data
    }

    /// Generate a metrics summary report
    pub fn generate_metrics_report(&self, metrics: &ContextEngineeringMetrics) -> MetricsReport {
        MetricsReport {
            timestamp: metrics.measurement_timestamp,
            overall_health: metrics.system_metrics.overall_health_score,
            key_insights: self.generate_key_insights(metrics),
            performance_summary: self.generate_performance_summary(metrics),
            recommendations: self.generate_recommendations(metrics),
            alerts: self.generate_alerts(metrics),
        }
    }

    /// Generate key insights from metrics
    fn generate_key_insights(&self, metrics: &ContextEngineeringMetrics) -> Vec<String> {
        let mut insights = Vec::new();

        // Field insights
        if metrics.field_metrics.field_coherence > 0.8 {
            insights.push("Field coherence is excellent - patterns are well-aligned".to_string());
        } else if metrics.field_metrics.field_coherence < 0.5 {
            insights.push("Field coherence is low - consider repair mechanisms".to_string());
        }

        // Memory insights
        if metrics.memory_metrics.memory_efficiency > 0.8 {
            insights.push("Memory system is highly efficient".to_string());
        }
        if metrics.memory_metrics.connection_density > 0.7 {
            insights.push("High memory interconnectivity detected".to_string());
        }

        // Protocol insights
        if metrics.protocol_metrics.success_rate > 90.0 {
            insights.push("Protocols are performing reliably".to_string());
        }

        // Meta-recursive insights
        if metrics.meta_recursive_metrics.enhancement_success_rate > 0.7 {
            insights.push("Self-improvement mechanisms are effective".to_string());
        }

        insights
    }

    /// Generate performance summary
    fn generate_performance_summary(&self, metrics: &ContextEngineeringMetrics) -> String {
        format!(
            "System Health: {:.1}% | Field Coherence: {:.1}% | Memory Efficiency: {:.1}% | Protocol Success: {:.1}% | Enhancement Rate: {:.1}%",
            metrics.system_metrics.overall_health_score * 100.0,
            metrics.field_metrics.field_coherence * 100.0,
            metrics.memory_metrics.memory_efficiency * 100.0,
            metrics.protocol_metrics.success_rate,
            metrics.meta_recursive_metrics.enhancement_success_rate * 100.0
        )
    }

    /// Generate recommendations based on metrics
    fn generate_recommendations(&self, metrics: &ContextEngineeringMetrics) -> Vec<String> {
        let mut recommendations = Vec::new();

        if metrics.field_metrics.field_coherence < 0.6 {
            recommendations
                .push("Apply resonance scaffolding to improve field coherence".to_string());
        }

        if metrics.memory_metrics.memory_fragmentation > 0.7 {
            recommendations
                .push("Consider memory defragmentation to improve efficiency".to_string());
        }

        if metrics.protocol_metrics.success_rate < 80.0 {
            recommendations.push("Review protocol implementations and error handling".to_string());
        }

        if metrics.meta_recursive_metrics.enhancement_success_rate < 0.5 {
            recommendations
                .push("Tune meta-recursive parameters for better enhancement outcomes".to_string());
        }

        if metrics.system_metrics.component_synergy < 0.6 {
            recommendations.push("Focus on improving cross-component integration".to_string());
        }

        recommendations
    }

    /// Generate alerts for critical issues
    fn generate_alerts(&self, metrics: &ContextEngineeringMetrics) -> Vec<Alert> {
        let mut alerts = Vec::new();

        if metrics.system_metrics.overall_health_score < 0.4 {
            alerts.push(Alert {
                level: AlertLevel::Critical,
                message: "System health is critically low".to_string(),
                component: "System".to_string(),
                metric_value: metrics.system_metrics.overall_health_score,
            });
        }

        if metrics.field_metrics.field_coherence < 0.3 {
            alerts.push(Alert {
                level: AlertLevel::High,
                message: "Field coherence is dangerously low".to_string(),
                component: "Neural Field".to_string(),
                metric_value: metrics.field_metrics.field_coherence,
            });
        }

        if metrics.protocol_metrics.success_rate < 50.0 {
            alerts.push(Alert {
                level: AlertLevel::High,
                message: "Protocol failure rate is too high".to_string(),
                component: "Protocols".to_string(),
                metric_value: metrics.protocol_metrics.success_rate,
            });
        }

        alerts
    }
}

/// Metrics report structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsReport {
    pub timestamp: DateTime<Utc>,
    pub overall_health: f32,
    pub key_insights: Vec<String>,
    pub performance_summary: String,
    pub recommendations: Vec<String>,
    pub alerts: Vec<Alert>,
}

/// Alert structure for critical issues
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub level: AlertLevel,
    pub message: String,
    pub component: String,
    pub metric_value: f32,
}

/// Alert severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertLevel {
    Info,
    Warning,
    High,
    Critical,
}
