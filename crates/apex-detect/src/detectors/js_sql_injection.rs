//! JavaScript/TypeScript SQL injection detector (CWE-89).
//!
//! Catches template-literal interpolation, string concatenation, and raw query
//! calls that build SQL statements from untrusted input.

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct JsSqlInjectionDetector;

struct CompiledPattern {
    name: &'static str,
    regex: Regex,
    description: &'static str,
}

static PATTERNS: LazyLock<Vec<CompiledPattern>> = LazyLock::new(|| {
    vec![
        CompiledPattern {
            name: "Template literal in query",
            regex: Regex::new(r#"\.query\s*\(\s*`[^`]*\$\{[^}]+\}[^`]*`"#).expect("invalid regex"),
            description: "SQL query built with template literal interpolation",
        },
        CompiledPattern {
            name: "String concatenation in query",
            regex: Regex::new(
                r#"\.query\s*\(\s*["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*["']\s*\+"#,
            )
            .expect("invalid regex"),
            description: "SQL query built with string concatenation",
        },
        CompiledPattern {
            name: "Raw query call",
            regex: Regex::new(r#"(?:\.raw|knex\.raw|sequelize\.query)\s*\("#)
                .expect("invalid regex"),
            description: "Raw SQL query API used — verify input is sanitized",
        },
        CompiledPattern {
            name: "Execute with interpolation",
            regex: Regex::new(r#"\.execute\s*\(\s*`[^`]*\$\{[^}]+\}[^`]*`"#)
                .expect("invalid regex"),
            description: "SQL execute call with template literal interpolation",
        },
    ]
});

/// Returns true when the line uses parameterized query syntax (safe pattern).
fn is_parameterized(line: &str) -> bool {
    // .query(stmt, [param]) or .query(stmt, params)
    // Matches patterns like: .query("...", [...]) or .query(sql, params)
    let has_query_with_array = line.contains(".query(") && line.contains(", [");
    let has_execute_with_array = line.contains(".execute(") && line.contains(", [");
    has_query_with_array || has_execute_with_array
}

/// Returns true when a `.query()` or `.execute()` call contains only a
/// hardcoded string literal with no interpolation.
fn is_hardcoded_string_query(line: &str) -> bool {
    // Match .query("...") or .query('...') where the string has no ${} or " +
    let trimmed = line.trim();

    // Check for .query("string") with no interpolation markers
    if let Some(pos) = trimmed.find(".query(") {
        let after = &trimmed[pos + 7..];
        // Starts with a quote and does not contain ${ or string concat
        if (after.starts_with('"') || after.starts_with('\''))
            && !after.contains("${")
            && !after.contains("\" +")
            && !after.contains("' +")
        {
            return true;
        }
    }
    false
}

#[async_trait]
impl Detector for JsSqlInjectionDetector {
    fn name(&self) -> &str {
        "js-sql-injection"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // Only run for JavaScript/TypeScript targets.
        if ctx.language != Language::JavaScript {
            return Ok(Vec::new());
        }

        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                // Skip parameterized queries (safe).
                if is_parameterized(trimmed) {
                    continue;
                }

                // Skip hardcoded string queries (no user input).
                if is_hardcoded_string_query(trimmed) {
                    continue;
                }

                for pattern in PATTERNS.iter() {
                    if pattern.regex.is_match(trimmed) {
                        let line_1based = (line_num + 1) as u32;

                        let evidence = super::util::reachability_evidence(ctx, path, line_1based);
                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: Severity::High,
                            category: FindingCategory::Injection,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!(
                                "{}: {} at line {}",
                                pattern.name, pattern.description, line_1based
                            ),
                            description: format!(
                                "{} pattern matched in {}:{}",
                                pattern.name,
                                path.display(),
                                line_1based
                            ),
                            evidence,
                            covered: false,
                            suggestion: "Use parameterized queries (e.g., db.query(sql, [param])) \
                                 instead of string interpolation."
                                .into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![89],
                    noisy: false, base_severity: None, coverage_confidence: None,
                        });
                        break; // One finding per line max
                    }
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
    async fn detects_template_literal_in_query() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.js"),
            "db.query(`SELECT * FROM ${table}`)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].category, FindingCategory::Injection);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    #[tokio::test]
    async fn detects_string_concat_in_query() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.js"),
            "db.query(\"SELECT * FROM \" + input)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    #[tokio::test]
    async fn detects_knex_raw() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/db.js"), "knex.raw(userQuery)\n".into());
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn detects_sequelize_query() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.js"),
            "sequelize.query(rawSql)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn skips_hardcoded_string_query() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.js"),
            "db.query(\"SELECT * FROM users\")\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "hardcoded string should not trigger");
    }

    #[tokio::test]
    async fn skips_parameterized_query() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.js"),
            "db.query(stmt, [param])\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(
            findings.is_empty(),
            "parameterized query should not trigger"
        );
    }

    #[tokio::test]
    async fn skips_non_javascript_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.py"),
            "db.query(`SELECT * FROM ${table}`)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = JsSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "should not fire for Python");
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/db.test.js"),
            "db.query(`SELECT * FROM ${table}`)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.js"),
            "// db.query(`SELECT * FROM ${table}`)\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = JsSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!JsSqlInjectionDetector.uses_cargo_subprocess());
    }
}
