//! Pattern Recognition and Processing Module
//! This module provides comprehensive pattern recognition capabilities including:
//! - Field pattern extraction and analysis
//! - Semantic pattern matching
//! - Temporal pattern detection
//! - Cross-domain pattern correlation

pub mod cross_domain_patterns;
pub mod field_patterns;
pub mod semantic_patterns;
pub mod temporal_patterns;

// Re-export main types for convenience
pub use cross_domain_patterns::*;
pub use field_patterns::*;
pub use semantic_patterns::*;
pub use temporal_patterns::*;
