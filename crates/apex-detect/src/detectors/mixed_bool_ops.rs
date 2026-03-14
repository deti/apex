use apex_core::error::Result;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;
use apex_core::types::Language;

pub struct MixedBoolOpsDetector;

/// Check whether mixing `||` and `&&` on this line is unparenthesized.
///
/// Heuristic: find the first `||`. Count `(` and `)` up to that position.
/// If they are balanced (i.e., the `||` is not inside parentheses), and
/// `&&` appears later on the same line, flag it.
fn has_unparenthesized_mixed_ops(line: &str) -> bool {
    // Must contain both operators
    let Some(or_pos) = line.find("||") else {
        return false;
    };
    if !line.contains("&&") {
        return false;
    }

    // The `&&` must appear after the `||`
    let after_or = &line[or_pos + 2..];
    if !after_or.contains("&&") {
        return false;
    }

    // Count parens up to the `||` position — if balanced, the `||` is at top level
    let before_or = &line[..or_pos];
    let open: i32 = before_or.chars().filter(|&c| c == '(').count() as i32;
    let close: i32 = before_or.chars().filter(|&c| c == ')').count() as i32;

    // If open == close, the `||` is not nested inside parens → flag
    // If open > close, the `||` is inside parens (e.g., `(a || b) && c`) → OK
    open == close
}

#[async_trait]
impl Detector for MixedBoolOpsDetector {
    fn name(&self) -> &str {
        "mixed-bool-ops"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        if ctx.language != Language::Rust {
            return Ok(findings);
        }

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if has_unparenthesized_mixed_ops(trimmed) {
                    let line_1based = (line_num + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::LogicBug,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Mixed `||` and `&&` without parentheses at line {}",
                            line_1based
                        ),
                        description: format!(
                            "Operator precedence may cause unexpected behavior in {}:{}. \
                             `&&` binds tighter than `||`, so `a || b && c` means `a || (b && c)`.",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Add explicit parentheses to clarify precedence".into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![783],
                    });
                }
            }
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AnalysisContext;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            language: lang,
            source_cache: files,
            ..AnalysisContext::test_default()
        }
    }

    #[tokio::test]
    async fn detects_mixed_ops_without_parens() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "if a.x() || b.y() && c.z() {\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::LogicBug);
        assert_eq!(findings[0].cwe_ids, vec![783]);
    }

    #[tokio::test]
    async fn ignores_parenthesized_mixed_ops() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "if (a.x() || b.y()) && c.z() {\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_single_and_operator() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "if a && b && c {\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_single_or_operator() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "if a || b || c {\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_non_rust_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.py"),
            "if a.x() || b.y() && c.z() {\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn has_unparenthesized_mixed_ops_positive() {
        assert!(has_unparenthesized_mixed_ops("a || b && c"));
        assert!(has_unparenthesized_mixed_ops("if x || y && z {"));
    }

    #[test]
    fn has_unparenthesized_mixed_ops_negative() {
        assert!(!has_unparenthesized_mixed_ops("(a || b) && c"));
        assert!(!has_unparenthesized_mixed_ops("a && b && c"));
        assert!(!has_unparenthesized_mixed_ops("a || b || c"));
        assert!(!has_unparenthesized_mixed_ops("a && b || c")); // && before || is fine
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!MixedBoolOpsDetector.uses_cargo_subprocess());
    }
}
