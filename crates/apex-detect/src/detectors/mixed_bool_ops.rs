use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::is_comment;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct MixedBoolOpsDetector;

// C-family: detects `||` and `&&` mixed without parens on the same line
static CFAMILY_OR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\|\|").unwrap());
static CFAMILY_AND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&&").unwrap());

// Python: `or` and `and` as word-boundary tokens
static PY_OR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bor\b").unwrap());
static PY_AND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\band\b").unwrap());

// Innermost parenthesized group (no nested parens) — shared by both strip functions
static PAREN_GROUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^()]*\)").unwrap());

/// Strip the innermost parenthesized groups (no nested parens) repeatedly
/// until no more can be stripped. Only strip groups that contain exactly one
/// type of boolean operator — those are the ones that clarify precedence.
fn strip_clarifying_paren_groups_cfamily(line: &str) -> String {
    let mut s = line.to_string();
    loop {
        let next = PAREN_GROUP.replace_all(&s, |cap: &regex::Captures| {
            let inner = cap[0].to_string();
            let has_or = CFAMILY_OR.is_match(&inner);
            let has_and = CFAMILY_AND.is_match(&inner);
            // Only strip if it contains exactly one operator type (clarifying parens)
            if (has_or && !has_and) || (!has_or && has_and) {
                "(_)".to_string()
            } else {
                inner
            }
        });
        if next == s {
            break;
        }
        s = next.into_owned();
    }
    s
}

fn strip_clarifying_paren_groups_python(line: &str) -> String {
    let mut s = line.to_string();
    loop {
        let next = PAREN_GROUP.replace_all(&s, |cap: &regex::Captures| {
            let inner = cap[0].to_string();
            let has_or = PY_OR.is_match(&inner);
            let has_and = PY_AND.is_match(&inner);
            if (has_or && !has_and) || (!has_or && has_and) {
                "(_)".to_string()
            } else {
                inner
            }
        });
        if next == s {
            break;
        }
        s = next.into_owned();
    }
    s
}

/// Check if a line has mixed `||` and `&&` without grouping parens (C-family / JS / Java / Rust).
fn has_unparenthesized_mixed_ops(line: &str) -> bool {
    let has_or = CFAMILY_OR.is_match(line);
    let has_and = CFAMILY_AND.is_match(line);
    if !(has_or && has_and) {
        return false;
    }
    let stripped = strip_clarifying_paren_groups_cfamily(line);
    CFAMILY_OR.is_match(&stripped) && CFAMILY_AND.is_match(&stripped)
}

/// Check if a line has mixed `or` and `and` without grouping parens (Python).
fn has_unparenthesized_mixed_ops_python(line: &str) -> bool {
    let has_or = PY_OR.is_match(line);
    let has_and = PY_AND.is_match(line);
    if !(has_or && has_and) {
        return false;
    }
    let stripped = strip_clarifying_paren_groups_python(line);
    PY_OR.is_match(&stripped) && PY_AND.is_match(&stripped)
}

#[async_trait]
impl Detector for MixedBoolOpsDetector {
    fn name(&self) -> &str {
        "mixed-bool-ops"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || is_comment(trimmed, ctx.language) {
                    continue;
                }

                let mixed = match ctx.language {
                    Language::Python => has_unparenthesized_mixed_ops_python(trimmed),
                    _ => has_unparenthesized_mixed_ops(trimmed),
                };

                if mixed {
                    let line_1based = (line_num + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::Medium,
                        category: FindingCategory::LogicBug,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Mixed boolean operators without parentheses at line {}",
                            line_1based
                        ),
                        description: format!(
                            "Line {} in {} mixes boolean AND and OR without parentheses, \
                             which may cause unexpected precedence behavior",
                            line_1based,
                            path.display()
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion:
                            "Add parentheses to clarify precedence: `(a || b) && c` or `a || (b && c)`"
                                .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![],
                    noisy: false, base_severity: None, coverage_confidence: None,
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

    // ---- Rust / C-family tests ----

    #[tokio::test]
    async fn detects_mixed_ops_rust() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/lib.rs"), "if a || b && c { }\n".into());
        let ctx = make_ctx(files, Language::Rust);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::LogicBug);
    }

    #[tokio::test]
    async fn no_finding_when_parenthesized_rust() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/lib.rs"), "if (a || b) && c { }\n".into());
        let ctx = make_ctx(files, Language::Rust);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_finding_single_operator_rust() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/lib.rs"), "if a || b || c { }\n".into());
        let ctx = make_ctx(files, Language::Rust);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- JavaScript tests ----

    #[tokio::test]
    async fn detects_mixed_ops_js() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/utils.js"),
            "if (a || b && c) {}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn no_finding_when_parenthesized_js() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/utils.js"),
            "if ((a || b) && c) {}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Python tests ----

    #[tokio::test]
    async fn detects_mixed_ops_python() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/utils.py"), "if a or b and c:\n".into());
        let ctx = make_ctx(files, Language::Python);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn no_finding_when_parenthesized_python() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/utils.py"), "if (a or b) and c:\n".into());
        let ctx = make_ctx(files, Language::Python);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_finding_single_operator_python() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/utils.py"), "if a or b or c:\n".into());
        let ctx = make_ctx(files, Language::Python);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/lib.rs"), "// if a || b && c\n".into());
        let ctx = make_ctx(files, Language::Rust);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_python_comments() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/utils.py"), "# if a or b and c\n".into());
        let ctx = make_ctx(files, Language::Python);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    // ---- Java test ----

    #[tokio::test]
    async fn detects_mixed_ops_java() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/Main.java"),
            "if (a || b && c) {}\n".into(),
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = MixedBoolOpsDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!MixedBoolOpsDetector.uses_cargo_subprocess());
    }
}
