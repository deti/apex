//! HAGNN (Heterogeneous Attention Graph Neural Network) vulnerability detector.
//!
//! Uses inter-procedural augmented graph (IPAG) features to predict vulnerabilities
//! via a trained GNN model.

use std::path::PathBuf;

/// Configuration for the HAGNN detector.
#[derive(Debug, Clone)]
pub struct HagnnConfig {
    /// Path to the serialized ONNX model.
    pub model_path: PathBuf,
    /// Minimum confidence to keep a prediction.
    pub confidence_threshold: f64,
    /// Maximum number of nodes in extracted subgraphs.
    pub max_subgraph_nodes: usize,
}

impl Default for HagnnConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("models/hagnn.onnx"),
            confidence_threshold: 0.7,
            max_subgraph_nodes: 500,
        }
    }
}

/// Feature vector extracted from an inter-procedural augmented graph.
#[derive(Debug, Clone)]
pub struct IpagFeatures {
    /// Number of nodes in the subgraph.
    pub node_count: usize,
    /// Histogram of node types (8 categories).
    pub type_histogram: [f32; 8],
    /// Maximum depth of data-flow chains.
    pub max_dataflow_depth: usize,
    /// Whether a taint path exists from source to sink.
    pub has_taint_path: bool,
}

impl IpagFeatures {
    /// Create an empty feature set (all zeros).
    pub fn empty() -> Self {
        Self {
            node_count: 0,
            type_histogram: [0.0; 8],
            max_dataflow_depth: 0,
            has_taint_path: false,
        }
    }

    /// Convert features to a flat f32 vector for model input.
    pub fn to_vec(&self) -> Vec<f32> {
        let mut v = Vec::with_capacity(11);
        v.push(self.node_count as f32);
        v.extend_from_slice(&self.type_histogram);
        v.push(self.max_dataflow_depth as f32);
        v.push(if self.has_taint_path { 1.0 } else { 0.0 });
        v
    }
}

/// A vulnerability prediction from the HAGNN model.
#[derive(Debug, Clone)]
pub struct VulnPrediction {
    /// CWE identifier for the predicted vulnerability class.
    pub cwe_id: u32,
    /// Model confidence score in [0, 1].
    pub confidence: f64,
    /// Human-readable label for the vulnerability.
    pub label: String,
}

/// HAGNN-based vulnerability detector.
#[derive(Debug)]
pub struct HagnnDetector {
    pub config: HagnnConfig,
}

impl HagnnDetector {
    /// Create a new detector with the given config.
    pub fn new(config: HagnnConfig) -> Self {
        Self { config }
    }

    /// Detector name.
    pub fn name(&self) -> &str {
        "hagnn"
    }

    /// Whether this detector spawns subprocesses.
    pub fn uses_subprocess(&self) -> bool {
        false
    }

    /// Filter predictions by the configured confidence threshold.
    pub fn filter_predictions(&self, predictions: Vec<VulnPrediction>) -> Vec<VulnPrediction> {
        predictions
            .into_iter()
            .filter(|p| p.confidence >= self.config.confidence_threshold)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let cfg = HagnnConfig::default();
        assert_eq!(cfg.model_path, PathBuf::from("models/hagnn.onnx"));
        assert!((cfg.confidence_threshold - 0.7).abs() < f64::EPSILON);
        assert_eq!(cfg.max_subgraph_nodes, 500);
    }

    #[test]
    fn ipag_features_empty() {
        let f = IpagFeatures::empty();
        assert_eq!(f.node_count, 0);
        assert_eq!(f.type_histogram, [0.0; 8]);
        assert_eq!(f.max_dataflow_depth, 0);
        assert!(!f.has_taint_path);
    }

    #[test]
    fn ipag_features_to_vec() {
        let f = IpagFeatures::empty();
        let v = f.to_vec();
        assert_eq!(v.len(), 11);
        assert!(v.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn ipag_features_to_vec_values() {
        let f = IpagFeatures {
            node_count: 10,
            type_histogram: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
            max_dataflow_depth: 3,
            has_taint_path: true,
        };
        let v = f.to_vec();
        assert_eq!(v.len(), 11);
        assert_eq!(v[0], 10.0);
        assert_eq!(v[1], 1.0);
        assert_eq!(v[8], 8.0);
        assert_eq!(v[9], 3.0);
        assert_eq!(v[10], 1.0);
    }

    #[test]
    fn vuln_prediction_creation() {
        let p = VulnPrediction {
            cwe_id: 79,
            confidence: 0.95,
            label: "Cross-site Scripting".into(),
        };
        assert_eq!(p.cwe_id, 79);
        assert!((p.confidence - 0.95).abs() < f64::EPSILON);
        assert_eq!(p.label, "Cross-site Scripting");
    }

    #[test]
    fn detector_creation() {
        let d = HagnnDetector::new(HagnnConfig::default());
        assert_eq!(d.name(), "hagnn");
    }

    #[test]
    fn detector_name() {
        let d = HagnnDetector::new(HagnnConfig::default());
        assert_eq!(d.name(), "hagnn");
    }

    #[test]
    fn detector_uses_subprocess() {
        let d = HagnnDetector::new(HagnnConfig::default());
        assert!(!d.uses_subprocess());
    }

    #[test]
    fn filter_predictions_passing() {
        let d = HagnnDetector::new(HagnnConfig::default());
        let preds = vec![
            VulnPrediction {
                cwe_id: 79,
                confidence: 0.9,
                label: "XSS".into(),
            },
            VulnPrediction {
                cwe_id: 89,
                confidence: 0.5,
                label: "SQLi".into(),
            },
            VulnPrediction {
                cwe_id: 22,
                confidence: 0.8,
                label: "Path Traversal".into(),
            },
        ];
        let filtered = d.filter_predictions(preds);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].cwe_id, 79);
        assert_eq!(filtered[1].cwe_id, 22);
    }

    #[test]
    fn filter_predictions_empty() {
        let d = HagnnDetector::new(HagnnConfig::default());
        let filtered = d.filter_predictions(vec![]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn filter_predictions_none_pass() {
        let d = HagnnDetector::new(HagnnConfig::default());
        let preds = vec![
            VulnPrediction {
                cwe_id: 79,
                confidence: 0.3,
                label: "XSS".into(),
            },
            VulnPrediction {
                cwe_id: 89,
                confidence: 0.1,
                label: "SQLi".into(),
            },
        ];
        let filtered = d.filter_predictions(preds);
        assert!(filtered.is_empty());
    }
}
