//! Multi-language SQL injection detector (CWE-89).
//!
//! Catches SQL queries built via string concatenation, interpolation, or format
//! strings across all 11 supported languages.

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_comment, is_test_file, taint_reaches_sink};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct MultiSqlInjectionDetector;

struct LangPattern {
    lang: Language,
    name: &'static str,
    regex: Regex,
    description: &'static str,
}

static PATTERNS: LazyLock<Vec<LangPattern>> = LazyLock::new(|| {
    vec![
        // ── Python ──────────────────────────────────────────────────
        LangPattern {
            lang: Language::Python,
            name: "f-string SQL",
            regex: Regex::new(r#"(?:execute|cursor\.execute|db\.execute|engine\.execute)\s*\(\s*f["']"#)
                .unwrap(),
            description: "SQL query built with f-string interpolation",
        },
        LangPattern {
            lang: Language::Python,
            name: "format SQL",
            regex: Regex::new(
                r#"(?:execute|cursor\.execute)\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE).*["']\s*\.format\s*\("#,
            )
            .unwrap(),
            description: "SQL query built with .format() interpolation",
        },
        LangPattern {
            lang: Language::Python,
            name: "percent-format SQL",
            regex: Regex::new(
                r#"(?:execute|cursor\.execute)\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE).*%s.*["']\s*%"#,
            )
            .unwrap(),
            description: "SQL query built with %-format interpolation",
        },
        LangPattern {
            lang: Language::Python,
            name: "concat SQL",
            regex: Regex::new(
                r#"(?:execute|cursor\.execute)\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*["']\s*\+"#,
            )
            .unwrap(),
            description: "SQL query built with string concatenation",
        },
        // ── JavaScript ──────────────────────────────────────────────
        LangPattern {
            lang: Language::JavaScript,
            name: "template literal in query",
            regex: Regex::new(r#"\.query\s*\(\s*`[^`]*\$\{[^}]+\}[^`]*`"#).unwrap(),
            description: "SQL query built with template literal interpolation",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "string concat in query",
            regex: Regex::new(
                r#"\.query\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*["']\s*\+"#,
            )
            .unwrap(),
            description: "SQL query built with string concatenation",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "raw query call",
            regex: Regex::new(r#"(?:\.raw|knex\.raw|sequelize\.query)\s*\("#).unwrap(),
            description: "Raw SQL query API used -- verify input is sanitized",
        },
        LangPattern {
            lang: Language::JavaScript,
            name: "execute with interpolation",
            regex: Regex::new(r#"\.execute\s*\(\s*`[^`]*\$\{[^}]+\}[^`]*`"#).unwrap(),
            description: "SQL execute call with template literal interpolation",
        },
        // ── Java ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Java,
            name: "Statement.execute concat",
            regex: Regex::new(
                r#"(?:Statement|stmt)\s*\.\s*(?:execute|executeQuery|executeUpdate)\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*["']\s*\+"#,
            )
            .unwrap(),
            description: "SQL statement built with string concatenation",
        },
        LangPattern {
            lang: Language::Java,
            name: "createQuery concat",
            regex: Regex::new(
                r#"createQuery\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*["']\s*\+"#,
            )
            .unwrap(),
            description: "JPA createQuery with string concatenation",
        },
        LangPattern {
            lang: Language::Java,
            name: "SQL string concat",
            regex: Regex::new(
                r#"["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s+.*["']\s*\+\s*[a-zA-Z_]"#,
            )
            .unwrap(),
            description: "SQL string concatenated with variable",
        },
        // ── Go ──────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Go,
            name: "db.Query with Sprintf",
            regex: Regex::new(r#"db\.(?:Query|Exec|QueryRow)\s*\(\s*fmt\.Sprintf\s*\("#).unwrap(),
            description: "SQL query built with fmt.Sprintf",
        },
        LangPattern {
            lang: Language::Go,
            name: "db.Query concat",
            regex: Regex::new(
                r#"db\.(?:Query|Exec|QueryRow)\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*["']\s*\+"#,
            )
            .unwrap(),
            description: "SQL query built with string concatenation",
        },
        // ── Ruby ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Ruby,
            name: "execute with interpolation",
            regex: Regex::new(r##"(?:execute|find_by_sql)\s*\(\s*"[^"]*#\{[^}]+\}"##).unwrap(),
            description: "SQL query built with string interpolation",
        },
        LangPattern {
            lang: Language::Ruby,
            name: "ActiveRecord execute",
            regex: Regex::new(
                r#"(?:ActiveRecord::Base\.connection\.execute|\.execute)\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*["']\s*\+"#,
            )
            .unwrap(),
            description: "ActiveRecord execute with string concatenation",
        },
        // ── C# ──────────────────────────────────────────────────────
        LangPattern {
            lang: Language::CSharp,
            name: "SqlCommand concat",
            regex: Regex::new(
                r#"(?:new\s+)?SqlCommand\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*["']\s*\+"#,
            )
            .unwrap(),
            description: "SqlCommand with string concatenation",
        },
        LangPattern {
            lang: Language::CSharp,
            name: "interpolated SqlCommand",
            regex: Regex::new(
                r#"(?:new\s+)?SqlCommand\s*\(\s*\$["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)"#,
            )
            .unwrap(),
            description: "SqlCommand with string interpolation",
        },
        LangPattern {
            lang: Language::CSharp,
            name: "ExecuteReader concat",
            regex: Regex::new(
                r#"(?:ExecuteNonQuery|ExecuteReader|ExecuteScalar)\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*["']\s*\+"#,
            )
            .unwrap(),
            description: "ExecuteReader with string concatenation",
        },
        // ── Swift ───────────────────────────────────────────────────
        LangPattern {
            lang: Language::Swift,
            name: "sqlite3_exec interpolation",
            regex: Regex::new(r#"sqlite3_exec\s*\(.*["\\]\(.*\)"#).unwrap(),
            description: "sqlite3_exec with string interpolation",
        },
        LangPattern {
            lang: Language::Swift,
            name: "execute interpolation",
            regex: Regex::new(
                r#"(?:execute|query)\s*\(\s*"(?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*\\["]\("#,
            )
            .unwrap(),
            description: "SQL execute with string interpolation",
        },
        // ── Kotlin ──────────────────────────────────────────────────
        LangPattern {
            lang: Language::Kotlin,
            name: "createQuery concat",
            regex: Regex::new(
                r#"createQuery\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s.*["']\s*\+"#,
            )
            .unwrap(),
            description: "JPA createQuery with string concatenation",
        },
        LangPattern {
            lang: Language::Kotlin,
            name: "SQL template string",
            regex: Regex::new(
                r#"["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s+.*\$[a-zA-Z_]"#,
            )
            .unwrap(),
            description: "SQL string with Kotlin template interpolation",
        },
        // ── C ───────────────────────────────────────────────────────
        LangPattern {
            lang: Language::C,
            name: "sprintf SQL",
            regex: Regex::new(
                r#"sprintf\s*\([^,]+,\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s"#,
            )
            .unwrap(),
            description: "SQL query built with sprintf",
        },
        LangPattern {
            lang: Language::C,
            name: "mysql_query/sqlite3_exec",
            regex: Regex::new(r#"(?:mysql_query|sqlite3_exec|PQexec)\s*\(\s*\w+\s*,"#).unwrap(),
            description: "Direct SQL query execution with variable",
        },
        // ── C++ ─────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Cpp,
            name: "sprintf SQL",
            regex: Regex::new(
                r#"sprintf\s*\([^,]+,\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)\s"#,
            )
            .unwrap(),
            description: "SQL query built with sprintf",
        },
        LangPattern {
            lang: Language::Cpp,
            name: "mysql_query/sqlite3_exec",
            regex: Regex::new(r#"(?:mysql_query|sqlite3_exec|PQexec)\s*\(\s*\w+\s*,"#).unwrap(),
            description: "Direct SQL query execution with variable",
        },
        // ── Rust ────────────────────────────────────────────────────
        LangPattern {
            lang: Language::Rust,
            name: "query with format!",
            regex: Regex::new(
                r#"(?:query|execute)\s*\(\s*&?format!\s*\(\s*["'](?i)(?:SELECT|INSERT|UPDATE|DELETE)"#,
            )
            .unwrap(),
            description: "SQL query built with format! macro",
        },
        LangPattern {
            lang: Language::Rust,
            name: "sql_query",
            regex: Regex::new(r#"(?:sql_query|sqlx::query)\s*\(\s*&?format!"#).unwrap(),
            description: "sql_query with format! macro -- use bind parameters",
        },
    ]
});

/// Returns true when the line uses parameterized/prepared query syntax.
fn is_parameterized(line: &str, lang: Language) -> bool {
    match lang {
        Language::JavaScript => {
            (line.contains(".query(") || line.contains(".execute(")) && line.contains(", [")
        }
        Language::Python => {
            // cursor.execute(sql, (param,)) or cursor.execute(sql, [param])
            (line.contains(".execute(") && (line.contains(", (") || line.contains(", [")))
                || line.contains("executemany(")
        }
        Language::Java | Language::Kotlin => {
            line.contains("prepareStatement(") || line.contains("setString(")
                || line.contains("setInt(")
        }
        Language::Go => {
            // db.Query(sql, param) with ? placeholder
            (line.contains(".Query(") || line.contains(".Exec(") || line.contains(".QueryRow("))
                && line.contains("?")
        }
        Language::CSharp => {
            line.contains("Parameters.Add") || line.contains("@") && line.contains("SqlParameter")
        }
        Language::Ruby => {
            line.contains("sanitize_sql") || line.contains("where(") || line.contains("find_by(")
        }
        Language::Rust => {
            line.contains(".bind(") || line.contains("query_as!(")
        }
        _ => false,
    }
}

/// Returns true when a query call contains only a hardcoded string literal.
fn is_hardcoded_query(line: &str) -> bool {
    let trimmed = line.trim();
    if let Some(pos) = trimmed.find(".query(") {
        let after = &trimmed[pos + 7..];
        if (after.starts_with('"') || after.starts_with('\''))
            && !after.contains("${")
            && !after.contains("\" +")
            && !after.contains("' +")
            && !after.contains("\\(")
        {
            return true;
        }
    }
    if let Some(pos) = trimmed.find(".execute(") {
        let after = &trimmed[pos + 9..];
        if (after.starts_with('"') || after.starts_with('\''))
            && !after.contains("${")
            && !after.contains("\" +")
            && !after.contains("' +")
            && !after.contains("\\(")
            && !after.contains("format")
        {
            return true;
        }
    }
    false
}

#[async_trait]
impl Detector for MultiSqlInjectionDetector {
    fn name(&self) -> &str {
        "multi-sql-injection"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // Skip Wasm — no SQL concept
        if ctx.language == Language::Wasm {
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

                if is_parameterized(trimmed, ctx.language) {
                    continue;
                }

                if is_hardcoded_query(trimmed) {
                    continue;
                }

                for pattern in PATTERNS.iter() {
                    if pattern.lang != ctx.language {
                        continue;
                    }

                    if pattern.regex.is_match(trimmed) {
                        let line_1based = (line_num + 1) as u32;

                        let mut finding = Finding {
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
                            evidence: super::util::reachability_evidence(ctx, path, line_1based),
                            covered: false,
                            suggestion:
                                "Use parameterized queries or prepared statements instead of \
                                 string interpolation/concatenation."
                                    .into(),
                            explanation: None,
                            fix: None,
                            cwe_ids: vec![89],
                            noisy: false, base_severity: None, coverage_confidence: None,
                        };

                        // Check taint flow if CPG is available — downgrade instead of discard.
                        if let Some(has_taint) = taint_reaches_sink(
                            ctx,
                            path,
                            line_1based,
                            &["user_input", "request", "args", "params", "query", "form"],
                        ) {
                            if !has_taint {
                                finding.noisy = true;
                                finding.severity = Severity::Low;
                                finding.description = format!(
                                    "{} (no taint flow detected — likely safe)",
                                    finding.description
                                );
                            }
                        }

                        findings.push(finding);
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

    fn single_file(name: &str, content: &str) -> HashMap<PathBuf, String> {
        let mut m = HashMap::new();
        m.insert(PathBuf::from(name), content.into());
        m
    }

    // ── Python ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_python_fstring_sql() {
        let files = single_file("src/db.py", "cursor.execute(f\"SELECT * FROM {table}\")\n");
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── JavaScript ──────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_js_template_literal_query() {
        let files = single_file("src/db.js", "db.query(`SELECT * FROM ${table}`)\n");
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── Java ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_java_sql_concat() {
        let files = single_file(
            "src/App.java",
            "String q = \"SELECT * FROM users WHERE id=\" + userId;\n",
        );
        let ctx = make_ctx(files, Language::Java);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── Go ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_go_sprintf_query() {
        let files = single_file(
            "src/main.go",
            "db.Query(fmt.Sprintf(\"SELECT * FROM %s\", table))\n",
        );
        let ctx = make_ctx(files, Language::Go);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── Ruby ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_ruby_interpolation_sql() {
        let files = single_file(
            "src/app.rb",
            "execute(\"SELECT * FROM users WHERE id=#{user_id}\")\n",
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── C# ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_csharp_sqlcommand_concat() {
        let files = single_file(
            "src/App.cs",
            "new SqlCommand(\"SELECT * FROM users WHERE id=\" + userId)\n",
        );
        let ctx = make_ctx(files, Language::CSharp);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── Swift ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_swift_sqlite3_exec() {
        let files = single_file(
            "src/App.swift",
            "sqlite3_exec(db, \"SELECT * FROM \\(table)\")\n",
        );
        let ctx = make_ctx(files, Language::Swift);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── Kotlin ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_kotlin_template_sql() {
        let files = single_file(
            "src/App.kt",
            "val q = \"SELECT * FROM users WHERE id=$userId\"\n",
        );
        let ctx = make_ctx(files, Language::Kotlin);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── C ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_c_sprintf_sql() {
        let files = single_file(
            "src/main.c",
            "sprintf(buf, \"SELECT * FROM users WHERE id=%s\", input);\n",
        );
        let ctx = make_ctx(files, Language::C);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── C++ ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_cpp_sprintf_sql() {
        let files = single_file(
            "src/main.cpp",
            "sprintf(buf, \"SELECT * FROM users WHERE id=%s\", input);\n",
        );
        let ctx = make_ctx(files, Language::Cpp);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── Rust ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detects_rust_format_query() {
        let files = single_file(
            "src/main.rs",
            "sqlx::query(&format!(\"SELECT * FROM users WHERE id={}\", id))\n",
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![89]);
    }

    // ── Negative tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn skips_parameterized_python() {
        let files = single_file(
            "src/db.py",
            "cursor.execute(\"SELECT * FROM users WHERE id=?\", (user_id,))\n",
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "parameterized query should not trigger");
    }

    #[tokio::test]
    async fn skips_parameterized_js() {
        let files = single_file("src/db.js", "db.query(stmt, [param])\n");
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "parameterized query should not trigger");
    }

    #[tokio::test]
    async fn skips_test_files() {
        let files = single_file(
            "tests/test_db.py",
            "cursor.execute(f\"SELECT * FROM {table}\")\n",
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_comments() {
        let files = single_file(
            "src/db.js",
            "// db.query(`SELECT * FROM ${table}`)\n",
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!MultiSqlInjectionDetector.uses_cargo_subprocess());
    }

    // -----------------------------------------------------------------------
    // Taint flow integration via CPG
    // -----------------------------------------------------------------------

    fn make_ctx_with_cpg(
        files: HashMap<PathBuf, String>,
        lang: Language,
        cpg: apex_cpg::Cpg,
    ) -> AnalysisContext {
        use std::sync::Arc;
        AnalysisContext {
            language: lang,
            source_cache: files,
            cpg: Some(Arc::new(cpg)),
            ..AnalysisContext::test_default()
        }
    }

    // CPG with taint flow → finding stays at original severity (High)
    //
    // For SQL injection the indicators include "params" and "query". We add an
    // Identifier "params" on line 1, connected via ReachingDef from a Parameter
    // (the taint source), so taint_reaches_sink returns Some(true).
    #[tokio::test]
    async fn taint_flow_present_keeps_original_severity() {
        use apex_cpg::{EdgeKind, NodeKind};

        let mut cpg = apex_cpg::Cpg::new();
        let param = cpg.add_node(NodeKind::Parameter {
            name: "params".into(),
            index: 0,
        });
        let sink_id = cpg.add_node(NodeKind::Identifier {
            name: "params".into(),
            line: 1,
        });
        cpg.add_edge(param, sink_id, EdgeKind::ReachingDef { variable: "params".into() });

        let files = single_file(
            "src/db.py",
            "cursor.execute(f\"SELECT * FROM users WHERE id={user_id}\")\n",
        );
        let ctx = make_ctx_with_cpg(files, Language::Python, cpg);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "taint flow present — should not be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "taint flow present — should stay High"
        );
    }

    // CPG with no taint flow → finding downgraded to noisy + Low
    //
    // We put a matching identifier on line 1 but no ReachingDef edge from any
    // Parameter to it — so taint_reaches_sink returns Some(false).
    #[tokio::test]
    async fn no_taint_flow_downgrades_to_noisy_low() {
        use apex_cpg::NodeKind;

        let mut cpg = apex_cpg::Cpg::new();
        // Sink candidate matches indicator "query", but no taint source connected.
        cpg.add_node(NodeKind::Identifier {
            name: "query".into(),
            line: 1,
        });

        let files = single_file(
            "src/db.py",
            "cursor.execute(f\"SELECT * FROM users WHERE id={user_id}\")\n",
        );
        let ctx = make_ctx_with_cpg(files, Language::Python, cpg);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].noisy, "no taint flow — should be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::Low,
            "no taint flow — should be downgraded to Low"
        );
        assert!(
            findings[0].description.contains("no taint flow"),
            "description should mention no taint flow"
        );
    }

    // No CPG → finding stays at original severity (fallback to pattern matching)
    #[tokio::test]
    async fn no_cpg_falls_back_to_pattern_severity() {
        let files = single_file(
            "src/db.py",
            "cursor.execute(f\"SELECT * FROM users WHERE id={user_id}\")\n",
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = MultiSqlInjectionDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].noisy, "no CPG — should not be noisy");
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "no CPG — should stay at pattern severity"
        );
    }
}
