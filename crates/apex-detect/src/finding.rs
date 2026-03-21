use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use apex_core::types::BranchId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: Uuid,
    pub detector: String,
    pub severity: Severity,
    pub category: FindingCategory,
    pub file: PathBuf,
    pub line: Option<u32>,
    pub title: String,
    pub description: String,
    pub evidence: Vec<Evidence>,
    pub covered: bool,
    pub suggestion: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cwe_ids: Vec<u32>,
    /// True for findings from detectors marked as noisy — code quality issues
    /// that are valid but produce high volume. Consumers can filter these in
    /// summaries while still showing them in full reports.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub noisy: bool,
    /// Original severity before coverage-informed re-scoring.
    /// Set by `apply_coverage_rescoring` — `None` means no re-scoring was applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_severity: Option<Severity>,
    /// Coverage confidence from the compound oracle (0.0 = uncovered, 1.0 = well-tested).
    /// Set by `apply_coverage_rescoring` — `None` means oracle was unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_confidence: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    pub fn rank(&self) -> u8 {
        match self {
            Severity::Critical => 0,
            Severity::High => 1,
            Severity::Medium => 2,
            Severity::Low => 3,
            Severity::Info => 4,
        }
    }

    /// Numeric score for severity (higher = more severe).
    pub fn numeric(&self) -> f64 {
        match self {
            Severity::Critical => 10.0,
            Severity::High => 8.0,
            Severity::Medium => 5.0,
            Severity::Low => 3.0,
            Severity::Info => 1.0,
        }
    }

    /// Convert a numeric severity score back to a Severity enum.
    /// Uses threshold-based mapping: >= 9 Critical, >= 7 High, >= 4 Medium, >= 2 Low, else Info.
    pub fn from_numeric(score: f64) -> Self {
        if score >= 9.0 {
            Severity::Critical
        } else if score >= 7.0 {
            Severity::High
        } else if score >= 4.0 {
            Severity::Medium
        } else if score >= 2.0 {
            Severity::Low
        } else {
            Severity::Info
        }
    }
}

impl Finding {
    /// Returns a coverage annotation label for display.
    ///
    /// - `Some("↑ uncovered")` when coverage confidence < 0.3 and severity was amplified
    /// - `Some("↓ tested")` when coverage confidence > 0.7 and severity was reduced
    /// - `None` when no coverage data is available or confidence is neutral
    pub fn coverage_label(&self) -> Option<&'static str> {
        let conf = self.coverage_confidence?;
        let base = self.base_severity?;
        if conf < 0.3 && self.severity.rank() < base.rank() {
            Some("\u{2191} uncovered")
        } else if conf > 0.7 && self.severity.rank() > base.rank() {
            Some("\u{2193} tested")
        } else {
            None
        }
    }

    /// True if coverage re-scoring changed the severity from the base.
    pub fn severity_was_adjusted(&self) -> bool {
        self.base_severity
            .map(|base| base != self.severity)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    MemorySafety,
    UndefinedBehavior,
    Injection,
    PanicPath,
    UnsafeCode,
    DependencyVuln,
    LogicBug,
    SecuritySmell,
    PathTraversal,
    InsecureConfig,
    HardcodedSecret,
    LicenseViolation,
    FeatureFlagHygiene,
    ApiBreakingChange,
    ApiSpecCoverage,
    ServiceDependency,
    SchemaMigrationRisk,
    TestDataQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Evidence {
    CoverageGap {
        branch_id: BranchId,
        line: u32,
    },
    SanitizerReport {
        sanitizer: String,
        stderr: String,
    },
    StaticAnalysis {
        tool: String,
        rule_id: String,
        sarif: serde_json::Value,
    },
    UnsafeBlock {
        file: PathBuf,
        line_range: (u32, u32),
        reason: String,
    },
    DiffBehavior {
        input: Vec<u8>,
        expected: String,
        actual: String,
    },
    ReachabilityChain {
        tool: String,
        paths: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Fix {
    CodePatch { file: PathBuf, diff: String },
    DependencyUpgrade { package: String, to: String },
    TestCase { file: PathBuf, code: String },
    ConfigChange { description: String },
    Manual { steps: Vec<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_rank_ordering() {
        assert_eq!(Severity::Critical.rank(), 0);
        assert_eq!(Severity::High.rank(), 1);
        assert_eq!(Severity::Medium.rank(), 2);
        assert_eq!(Severity::Low.rank(), 3);
        assert_eq!(Severity::Info.rank(), 4);
    }

    #[test]
    fn severity_ord_matches_rank() {
        assert!(Severity::Critical < Severity::High);
        assert!(Severity::High < Severity::Medium);
        assert!(Severity::Medium < Severity::Low);
        assert!(Severity::Low < Severity::Info);
    }

    #[test]
    fn finding_serializes_to_json() {
        let f = Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity: Severity::High,
            category: FindingCategory::PanicPath,
            file: PathBuf::from("src/main.rs"),
            line: Some(42),
            title: "unwrap on error path".into(),
            description: "desc".into(),
            evidence: vec![],
            covered: false,
            suggestion: "add error test".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![],
                    noisy: false,
            base_severity: None,
            coverage_confidence: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"severity\":\"high\""));
        assert!(json.contains("\"category\":\"panic_path\""));
        assert!(json.contains("\"covered\":false"));
    }

    #[test]
    fn fix_variants_serialize() {
        let patch = Fix::CodePatch {
            file: "src/lib.rs".into(),
            diff: "+check".into(),
        };
        let json = serde_json::to_string(&patch).unwrap();
        assert!(json.contains("\"type\":\"code_patch\""));

        let upgrade = Fix::DependencyUpgrade {
            package: "openssl".into(),
            to: "0.10.55".into(),
        };
        let json = serde_json::to_string(&upgrade).unwrap();
        assert!(json.contains("\"type\":\"dependency_upgrade\""));
    }

    #[test]
    fn evidence_variants_serialize() {
        let e = Evidence::SanitizerReport {
            sanitizer: "asan".into(),
            stderr: "heap-buffer-overflow".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"type\":\"sanitizer_report\""));
        assert!(json.contains("asan"));
    }

    #[test]
    fn evidence_coverage_gap_serializes() {
        let e = Evidence::CoverageGap {
            branch_id: BranchId::new(1, 10, 5, 0),
            line: 10,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"type\":\"coverage_gap\""));
    }

    #[test]
    fn evidence_unsafe_block_serializes() {
        let e = Evidence::UnsafeBlock {
            file: PathBuf::from("src/lib.rs"),
            line_range: (10, 20),
            reason: "raw pointer deref".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"type\":\"unsafe_block\""));
        assert!(json.contains("raw pointer deref"));
    }

    #[test]
    fn evidence_diff_behavior_serializes() {
        let e = Evidence::DiffBehavior {
            input: vec![1, 2, 3],
            expected: "ok".into(),
            actual: "err".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"type\":\"diff_behavior\""));
    }

    #[test]
    fn evidence_static_analysis_serializes() {
        let e = Evidence::StaticAnalysis {
            tool: "clippy".into(),
            rule_id: "clippy::unwrap_used".into(),
            sarif: serde_json::json!({"version": "2.1.0"}),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"type\":\"static_analysis\""));
        assert!(json.contains("clippy::unwrap_used"));
    }

    #[test]
    fn fix_test_case_serializes() {
        let f = Fix::TestCase {
            file: "tests/test_bug.rs".into(),
            code: "fn test_it() {}".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"type\":\"test_case\""));
    }

    #[test]
    fn fix_config_change_serializes() {
        let f = Fix::ConfigChange {
            description: "Set timeout to 30s".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"type\":\"config_change\""));
    }

    #[test]
    fn fix_manual_serializes() {
        let f = Fix::Manual {
            steps: vec!["Step 1".into(), "Step 2".into()],
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"type\":\"manual\""));
        assert!(json.contains("Step 1"));
    }

    #[test]
    fn finding_with_explanation_and_fix() {
        let f = Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity: Severity::Critical,
            category: FindingCategory::MemorySafety,
            file: PathBuf::from("src/lib.rs"),
            line: None,
            title: "buffer overflow".into(),
            description: "desc".into(),
            evidence: vec![],
            covered: true,
            suggestion: "fix it".into(),
            explanation: Some("detailed explanation".into()),
            fix: Some(Fix::CodePatch {
                file: "src/lib.rs".into(),
                diff: "+bounds check".into(),
            }),
            cwe_ids: vec![],
                    noisy: false,
            base_severity: None,
            coverage_confidence: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"explanation\":\"detailed explanation\""));
        assert!(json.contains("\"type\":\"code_patch\""));
        assert!(json.contains("\"covered\":true"));
    }

    #[test]
    fn finding_roundtrip_serde() {
        let f = Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity: Severity::Low,
            category: FindingCategory::LogicBug,
            file: PathBuf::from("src/main.rs"),
            line: Some(42),
            title: "logic error".into(),
            description: "desc".into(),
            evidence: vec![Evidence::SanitizerReport {
                sanitizer: "msan".into(),
                stderr: "use of uninitialized value".into(),
            }],
            covered: false,
            suggestion: "initialize".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![],
                    noisy: false,
            base_severity: None,
            coverage_confidence: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        let f2: Finding = serde_json::from_str(&json).unwrap();
        assert_eq!(f2.detector, "test");
        assert_eq!(f2.severity, Severity::Low);
        assert_eq!(f2.category, FindingCategory::LogicBug);
        assert_eq!(f2.line, Some(42));
    }

    #[test]
    fn finding_cwe_ids_serializes() {
        let f = Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity: Severity::High,
            category: FindingCategory::Injection,
            file: PathBuf::from("src/main.rs"),
            line: Some(1),
            title: "cmd injection".into(),
            description: "desc".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix it".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![78, 94],
                    noisy: false,
            base_severity: None,
            coverage_confidence: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"cwe_ids\":[78,94]"));
    }

    #[test]
    fn finding_empty_cwe_ids_not_serialized() {
        let f = Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity: Severity::High,
            category: FindingCategory::Injection,
            file: PathBuf::from("src/main.rs"),
            line: Some(1),
            title: "cmd injection".into(),
            description: "desc".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix it".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![],
                    noisy: false,
            base_severity: None,
            coverage_confidence: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains("cwe_ids"));
    }

    #[test]
    fn finding_category_all_variants() {
        let cats = [
            FindingCategory::MemorySafety,
            FindingCategory::UndefinedBehavior,
            FindingCategory::Injection,
            FindingCategory::PanicPath,
            FindingCategory::UnsafeCode,
            FindingCategory::DependencyVuln,
            FindingCategory::LogicBug,
            FindingCategory::SecuritySmell,
            FindingCategory::PathTraversal,
            FindingCategory::InsecureConfig,
            FindingCategory::HardcodedSecret,
            FindingCategory::LicenseViolation,
        ];
        for cat in cats {
            let json = serde_json::to_string(&cat).unwrap();
            let back: FindingCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(cat, back);
        }
    }

    #[test]
    fn severity_all_variants_roundtrip() {
        let sevs = [
            Severity::Critical,
            Severity::High,
            Severity::Medium,
            Severity::Low,
            Severity::Info,
        ];
        for sev in sevs {
            let json = serde_json::to_string(&sev).unwrap();
            let back: Severity = serde_json::from_str(&json).unwrap();
            assert_eq!(sev, back);
        }
    }
}
