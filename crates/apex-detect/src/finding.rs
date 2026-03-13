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
        };
        let json = serde_json::to_string(&f).unwrap();
        let f2: Finding = serde_json::from_str(&json).unwrap();
        assert_eq!(f2.detector, "test");
        assert_eq!(f2.severity, Severity::Low);
        assert_eq!(f2.category, FindingCategory::LogicBug);
        assert_eq!(f2.line, Some(42));
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
