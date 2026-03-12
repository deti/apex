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
}
