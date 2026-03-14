//! Dual encoder vulnerability detector.
//!
//! Combines text-based and graph-based encoders to produce a fused vulnerability
//! score, weighted by configuration.

use std::path::PathBuf;

/// Configuration for the dual encoder detector.
#[derive(Debug, Clone)]
pub struct DualEncoderConfig {
    /// Path to the text encoder model.
    pub text_model_path: PathBuf,
    /// Path to the graph encoder model.
    pub graph_model_path: PathBuf,
    /// Minimum combined confidence to keep a score.
    pub confidence_threshold: f64,
    /// Weight given to text encoder (graph weight = 1 - text_weight).
    pub text_weight: f64,
}

impl Default for DualEncoderConfig {
    fn default() -> Self {
        Self {
            text_model_path: PathBuf::from("models/text_encoder.onnx"),
            graph_model_path: PathBuf::from("models/graph_encoder.onnx"),
            confidence_threshold: 0.7,
            text_weight: 0.5,
        }
    }
}

/// Features extracted from source code text.
#[derive(Debug, Clone)]
pub struct TextFeatures {
    /// Token frequency counts from the text encoder vocabulary.
    pub token_counts: Vec<f32>,
    /// Hash of the function name for deduplication.
    pub function_hash: u64,
    /// Length of the source code in bytes.
    pub code_length: usize,
}

/// Combined score from dual encoders.
#[derive(Debug, Clone)]
pub struct DualScore {
    /// Score from the text encoder.
    pub text_score: f64,
    /// Score from the graph encoder.
    pub graph_score: f64,
    /// Weighted combination of text and graph scores.
    pub combined_score: f64,
    /// Predicted CWE identifier, if any.
    pub predicted_cwe: Option<u32>,
}

impl DualScore {
    /// Combine text and graph scores using the given text weight.
    pub fn combine(text_score: f64, graph_score: f64, text_weight: f64) -> Self {
        let combined = text_score * text_weight + graph_score * (1.0 - text_weight);
        Self {
            text_score,
            graph_score,
            combined_score: combined,
            predicted_cwe: None,
        }
    }

    /// Attach a CWE prediction to this score.
    pub fn with_cwe(mut self, cwe_id: u32) -> Self {
        self.predicted_cwe = Some(cwe_id);
        self
    }
}

/// Dual encoder vulnerability detector.
#[derive(Debug)]
pub struct DualEncoderDetector {
    pub config: DualEncoderConfig,
}

impl DualEncoderDetector {
    /// Create a new detector with the given config.
    pub fn new(config: DualEncoderConfig) -> Self {
        Self { config }
    }

    /// Detector name.
    pub fn name(&self) -> &str {
        "dual-encoder"
    }

    /// Whether this detector spawns subprocesses.
    pub fn uses_subprocess(&self) -> bool {
        false
    }

    /// Filter scores by the configured confidence threshold.
    pub fn filter_scores(&self, scores: Vec<DualScore>) -> Vec<DualScore> {
        scores
            .into_iter()
            .filter(|s| s.combined_score >= self.config.confidence_threshold)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let cfg = DualEncoderConfig::default();
        assert_eq!(cfg.text_model_path, PathBuf::from("models/text_encoder.onnx"));
        assert_eq!(cfg.graph_model_path, PathBuf::from("models/graph_encoder.onnx"));
        assert!((cfg.confidence_threshold - 0.7).abs() < f64::EPSILON);
        assert!((cfg.text_weight - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn dual_score_combine_equal_weight() {
        let s = DualScore::combine(0.8, 0.6, 0.5);
        assert!((s.combined_score - 0.7).abs() < f64::EPSILON);
        assert!(s.predicted_cwe.is_none());
    }

    #[test]
    fn dual_score_combine_text_heavy() {
        let s = DualScore::combine(1.0, 0.0, 0.8);
        assert!((s.combined_score - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn dual_score_combine_graph_heavy() {
        let s = DualScore::combine(0.0, 1.0, 0.2);
        assert!((s.combined_score - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn dual_score_combine_zero_weight() {
        let s = DualScore::combine(1.0, 0.5, 0.0);
        assert!((s.combined_score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn dual_score_combine_full_weight() {
        let s = DualScore::combine(0.9, 0.1, 1.0);
        assert!((s.combined_score - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn dual_score_with_cwe() {
        let s = DualScore::combine(0.8, 0.6, 0.5).with_cwe(79);
        assert_eq!(s.predicted_cwe, Some(79));
        assert!((s.combined_score - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn detector_name() {
        let d = DualEncoderDetector::new(DualEncoderConfig::default());
        assert_eq!(d.name(), "dual-encoder");
        assert!(!d.uses_subprocess());
    }

    #[test]
    fn filter_scores_passing() {
        let d = DualEncoderDetector::new(DualEncoderConfig::default());
        let scores = vec![
            DualScore::combine(0.9, 0.8, 0.5),  // 0.85
            DualScore::combine(0.3, 0.2, 0.5),  // 0.25
            DualScore::combine(0.8, 0.7, 0.5),  // 0.75
        ];
        let filtered = d.filter_scores(scores);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filter_scores_empty() {
        let d = DualEncoderDetector::new(DualEncoderConfig::default());
        let filtered = d.filter_scores(vec![]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn text_features_creation() {
        let f = TextFeatures {
            token_counts: vec![1.0, 2.0, 3.0],
            function_hash: 12345,
            code_length: 100,
        };
        assert_eq!(f.token_counts.len(), 3);
        assert_eq!(f.function_hash, 12345);
        assert_eq!(f.code_length, 100);
    }

    #[test]
    fn debug_impls() {
        let cfg = DualEncoderConfig::default();
        let dbg = format!("{:?}", cfg);
        assert!(dbg.contains("DualEncoderConfig"));

        let score = DualScore::combine(0.5, 0.5, 0.5);
        let dbg = format!("{:?}", score);
        assert!(dbg.contains("DualScore"));

        let det = DualEncoderDetector::new(DualEncoderConfig::default());
        let dbg = format!("{:?}", det);
        assert!(dbg.contains("DualEncoderDetector"));
    }
}
