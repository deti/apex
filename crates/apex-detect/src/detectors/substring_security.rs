use apex_core::error::Result;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct SubstringSecurityDetector;

static SECURITY_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"fn\s+(is_source|is_sink|is_sanitizer|is_trusted|is_authorized|check_permission)")
        .expect("invalid security function regex")
});

#[async_trait]
impl Detector for SubstringSecurityDetector {
    fn name(&self) -> &str {
        "substring-security"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let mut in_security_fn = false;
            let mut brace_depth: i32 = 0;
            let mut fn_start_depth: i32 = 0;

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                // Check if we're entering a security-critical function
                if !in_security_fn && SECURITY_FN.is_match(trimmed) {
                    in_security_fn = true;
                    fn_start_depth = brace_depth;
                }

                // Track brace depth
                for ch in trimmed.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }

                // Check if we've exited the security function
                if in_security_fn && brace_depth <= fn_start_depth {
                    // If we opened and closed the function body, we're done
                    // (handle case where fn signature and opening brace are on same line)
                    if trimmed.contains('{') || brace_depth < fn_start_depth {
                        // Only reset if we actually entered the body at some point
                        // and depth dropped back
                        if brace_depth <= fn_start_depth && !trimmed.contains('{') {
                            in_security_fn = false;
                            continue;
                        }
                    }
                }

                // Flag .contains( calls inside security functions
                if in_security_fn && trimmed.contains(".contains(") {
                    let line_1based = (line_num + 1) as u32;

                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: self.name().into(),
                        severity: Severity::High,
                        category: FindingCategory::SecuritySmell,
                        file: path.clone(),
                        line: Some(line_1based),
                        title: format!(
                            "Substring match in security function at line {}",
                            line_1based
                        ),
                        description: format!(
                            "Using .contains() for security decisions in {}:{} can be \
                             bypassed with partial matches (CWE-183)",
                            path.display(),
                            line_1based
                        ),
                        evidence: vec![],
                        covered: false,
                        suggestion:
                            "Use exact match (`==`) or suffix match (`.ends_with()`) instead \
                             of substring `.contains()` for security decisions"
                                .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![183],
                    noisy: false,
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
    use apex_core::types::Language;
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
    async fn detects_contains_in_is_sink() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/taint.rs"),
            r#"fn is_sink(name: &str) -> bool {
    for s in SINKS {
        if name.contains(s.as_str()) {
            return true;
        }
    }
    false
}
"#
            .into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SubstringSecurityDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
        assert_eq!(findings[0].cwe_ids, vec![183]);
    }

    #[tokio::test]
    async fn detects_contains_in_is_source() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/taint.rs"),
            r#"fn is_source(name: &str) -> bool {
    SOURCES.iter().any(|s| name.contains(s))
}
"#
            .into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SubstringSecurityDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn ignores_exact_match_in_is_sink() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/taint.rs"),
            r#"fn is_sink(name: &str) -> bool {
    for s in SINKS {
        if name == s.as_str() {
            return true;
        }
    }
    false
}
"#
            .into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SubstringSecurityDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn ignores_contains_in_non_security_function() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/search.rs"),
            r#"fn search(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}
"#
            .into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SubstringSecurityDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/taint_test.rs"),
            r#"fn is_sink(name: &str) -> bool {
    name.contains("bad")
}
"#
            .into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SubstringSecurityDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_contains_in_check_permission() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/auth.rs"),
            r#"fn check_permission(role: &str) -> bool {
    role.contains("admin")
}
"#
            .into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SubstringSecurityDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!SubstringSecurityDetector.uses_cargo_subprocess());
    }
}
