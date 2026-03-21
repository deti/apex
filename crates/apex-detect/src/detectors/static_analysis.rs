use apex_core::error::{ApexError, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::StaticAnalysisConfig;
use crate::context::AnalysisContext;
use crate::finding::{Evidence, Finding, FindingCategory, Severity};
use crate::Detector;

#[derive(Default)]
pub struct StaticAnalysisDetector {
    pub extra_args: Vec<String>,
}

impl StaticAnalysisDetector {
    pub fn new(config: &StaticAnalysisConfig) -> Self {
        StaticAnalysisDetector {
            extra_args: config.clippy_extra_args.clone(),
        }
    }
}

#[async_trait]
impl Detector for StaticAnalysisDetector {
    fn name(&self) -> &str {
        "static-analysis"
    }

    fn uses_cargo_subprocess(&self) -> bool {
        true
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut args = vec![
            "clippy".to_string(),
            "--message-format".to_string(),
            "json".to_string(),
            "--".to_string(),
        ];
        args.extend(self.extra_args.clone());

        let output = tokio::process::Command::new("cargo")
            .args(&args)
            .current_dir(&ctx.target_root)
            .output()
            .await
            .map_err(|e| ApexError::Detect(format!("cargo-clippy: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut findings = Vec::new();

        for line in stdout.lines() {
            findings.extend(parse_clippy_line(line));
        }

        Ok(findings)
    }
}

pub fn clippy_code_to_category(code: &str) -> FindingCategory {
    if code.contains("unwrap") || code.contains("expect_used") || code.contains("panic") {
        FindingCategory::PanicPath
    } else if code.contains("cast") || code.contains("truncat") || code.contains("overflow") {
        FindingCategory::UndefinedBehavior
    } else if code.contains("unsafe") {
        FindingCategory::UnsafeCode
    } else {
        FindingCategory::SecuritySmell
    }
}

pub fn parse_clippy_line(line: &str) -> Vec<Finding> {
    let parsed: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    if parsed.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
        return vec![];
    }

    let message = &parsed["message"];
    let code = message["code"]["code"].as_str().unwrap_or("");
    if code.is_empty() {
        return vec![];
    }

    let msg_text = message["message"].as_str().unwrap_or("");
    let level = message["level"].as_str().unwrap_or("warning");

    let spans = message["spans"].as_array();
    let (file, line_num) = spans
        .and_then(|s| s.first())
        .map(|span| {
            let f = span["file_name"].as_str().unwrap_or("unknown");
            let l = span["line_start"].as_u64().unwrap_or(0) as u32;
            (PathBuf::from(f), Some(l))
        })
        .unwrap_or((PathBuf::from("unknown"), None));

    let severity = match level {
        "error" => Severity::High,
        "warning" => Severity::Medium,
        _ => Severity::Low,
    };

    let category = clippy_code_to_category(code);

    vec![Finding {
        id: Uuid::new_v4(),
        detector: "static-analysis".into(),
        severity,
        category,
        file,
        line: line_num,
        title: format!("{code}: {msg_text}"),
        description: msg_text.into(),
        evidence: vec![Evidence::StaticAnalysis {
            tool: "clippy".into(),
            rule_id: code.into(),
            sarif: serde_json::Value::Null,
        }],
        covered: false,
        suggestion: format!("Address clippy lint: {code}"),
        explanation: None,
        fix: None,
        cwe_ids: vec![],
                    noisy: false, base_severity: None, coverage_confidence: None,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::FindingCategory;

    #[test]
    fn parse_clippy_diagnostic() {
        let line = r#"{"reason":"compiler-message","message":{"code":{"code":"clippy::unwrap_used"},"level":"warning","message":"used `unwrap()` on a `Result`","spans":[{"file_name":"src/main.rs","line_start":42,"line_end":42,"column_start":5,"column_end":20}]}}"#;
        let findings = parse_clippy_line(line);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::PanicPath);
        assert_eq!(findings[0].line, Some(42));
        assert!(findings[0].title.contains("clippy::unwrap_used"));
    }

    #[test]
    fn parse_clippy_non_diagnostic_line() {
        let line = r#"{"reason":"build-script-executed"}"#;
        let findings = parse_clippy_line(line);
        assert!(findings.is_empty());
    }

    #[test]
    fn clippy_code_to_category_mapping() {
        assert_eq!(
            clippy_code_to_category("clippy::unwrap_used"),
            FindingCategory::PanicPath
        );
        assert_eq!(
            clippy_code_to_category("clippy::cast_possible_truncation"),
            FindingCategory::UndefinedBehavior
        );
        assert_eq!(
            clippy_code_to_category("clippy::some_other_lint"),
            FindingCategory::SecuritySmell
        );
    }

    #[test]
    fn uses_cargo_subprocess_returns_true() {
        let d = StaticAnalysisDetector::default();
        assert!(d.uses_cargo_subprocess());
    }

    #[test]
    fn parse_clippy_invalid_json() {
        let findings = parse_clippy_line("not json at all");
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_clippy_empty_code() {
        let line = r#"{"reason":"compiler-message","message":{"code":{"code":""},"level":"warning","message":"msg","spans":[]}}"#;
        let findings = parse_clippy_line(line);
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_clippy_no_code_field() {
        let line = r#"{"reason":"compiler-message","message":{"code":null,"level":"warning","message":"msg","spans":[]}}"#;
        let findings = parse_clippy_line(line);
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_clippy_error_level() {
        let line = r#"{"reason":"compiler-message","message":{"code":{"code":"clippy::cast_possible_truncation"},"level":"error","message":"truncation","spans":[{"file_name":"src/lib.rs","line_start":10,"line_end":10,"column_start":1,"column_end":20}]}}"#;
        let findings = parse_clippy_line(line);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::UndefinedBehavior);
    }

    #[test]
    fn parse_clippy_note_level() {
        let line = r#"{"reason":"compiler-message","message":{"code":{"code":"clippy::foo"},"level":"note","message":"info","spans":[]}}"#;
        let findings = parse_clippy_line(line);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    #[test]
    fn parse_clippy_no_spans() {
        let line = r#"{"reason":"compiler-message","message":{"code":{"code":"clippy::foo"},"level":"warning","message":"msg","spans":[]}}"#;
        let findings = parse_clippy_line(line);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, PathBuf::from("unknown"));
        assert_eq!(findings[0].line, None);
    }

    #[test]
    fn clippy_code_to_category_unsafe() {
        assert_eq!(
            clippy_code_to_category("clippy::unsafe_derive_deserialize"),
            FindingCategory::UnsafeCode
        );
    }

    #[test]
    fn clippy_code_to_category_expect_used() {
        assert_eq!(
            clippy_code_to_category("clippy::expect_used"),
            FindingCategory::PanicPath
        );
    }

    #[test]
    fn clippy_code_to_category_overflow() {
        assert_eq!(
            clippy_code_to_category("clippy::integer_overflow"),
            FindingCategory::UndefinedBehavior
        );
    }

    #[test]
    fn clippy_code_to_category_panic() {
        assert_eq!(
            clippy_code_to_category("clippy::panic_in_result"),
            FindingCategory::PanicPath
        );
    }

    #[test]
    fn static_analysis_new_from_config() {
        use crate::config::StaticAnalysisConfig;
        let config = StaticAnalysisConfig {
            clippy_extra_args: vec!["-W".into(), "clippy::pedantic".into()],
            sarif_paths: vec![],
        };
        let det = StaticAnalysisDetector::new(&config);
        assert_eq!(det.extra_args, vec!["-W", "clippy::pedantic"]);
    }
}
