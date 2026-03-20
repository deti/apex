//! SQL injection detector — identifies unsanitized user input in SQL queries.
//!
//! Scans for string formatting/concatenation patterns used to build SQL
//! queries. A full CPG-based version would trace taint flows; this initial
//! implementation uses pattern matching on common injection vectors.

use crate::finding::{Finding, FindingCategory, Severity};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;
use uuid::Uuid;

static FSTRING_SQL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"f["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*\{[^}]+\}.*["']"#).unwrap()
});

static PERCENT_SQL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*%[sd].*["']\s*%"#).unwrap()
});

static CONCAT_SQL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*["']\s*\+"#).unwrap()
});

/// SQL execution function patterns.
const SQL_EXEC_PATTERNS: &[&str] = &[
    "execute(",
    "executemany(",
    "raw(",
    "cursor.execute(",
    "db.execute(",
    "conn.execute(",
    "session.execute(",
];

/// Scan source code for SQL injection vulnerabilities.
pub fn scan_sql_injection(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip parameterized queries (safe pattern).
        if SQL_EXEC_PATTERNS.iter().any(|p| trimmed.contains(p))
            && (trimmed.contains("%s\", (")
                || trimmed.contains("%s\", [")
                || trimmed.contains("?, (")
                || trimmed.contains("?, ["))
        {
            continue;
        }

        let is_vuln = FSTRING_SQL.is_match(trimmed)
            || PERCENT_SQL.is_match(trimmed)
            || CONCAT_SQL.is_match(trimmed);

        if is_vuln {
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "sql_injection".into(),
                severity: Severity::High,
                category: FindingCategory::Injection,
                file: PathBuf::from(file_path),
                line: Some(line_1based),
                title: "Potential SQL injection via string interpolation".into(),
                description: format!(
                    "SQL query constructed with string formatting at line {line_1based}. \
                     Use parameterized queries instead."
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Use parameterized queries (e.g., cursor.execute(\"SELECT ... WHERE x = %s\", (val,)))".into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![89],
                    noisy: false,
            });
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_string_format_injection() {
        let source = r#"
def get_user(request):
    name = request.args.get('name')
    query = "SELECT * FROM users WHERE name = '%s'" % name
    cursor.execute(query)
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(!findings.is_empty());
        assert!(findings[0].title.contains("SQL"));
    }

    #[test]
    fn detect_fstring_injection() {
        let source = r#"
def get_user(name):
    query = f"SELECT * FROM users WHERE name = '{name}'"
    db.execute(query)
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn safe_parameterized_query_not_flagged() {
        let source = r#"
def get_user(name):
    cursor.execute("SELECT * FROM users WHERE name = %s", (name,))
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn safe_no_user_input() {
        let source = r#"
def get_count():
    cursor.execute("SELECT COUNT(*) FROM users")
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_concatenation_injection() {
        let source = r#"
def search(query_str):
    sql = "SELECT * FROM items WHERE name = '" + query_str + "'"
    conn.execute(sql)
"#;
        let findings = scan_sql_injection(source, "search.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn finding_has_correct_category() {
        let source = "query = f\"SELECT * FROM t WHERE x = '{user_input}'\"\ndb.execute(query)";
        let findings = scan_sql_injection(source, "x.py");
        if !findings.is_empty() {
            assert_eq!(findings[0].category, FindingCategory::Injection);
        }
    }

    // -----------------------------------------------------------------------
    // BUG: SQL regexes recompiled on every call to scan_sql_injection
    // -----------------------------------------------------------------------
    // Regex::new() is called 3 times per invocation of scan_sql_injection.
    // For large codebases scanned file-by-file, this is a performance bug.
    // (Not a correctness bug, but worth fixing with LazyLock.)

    // -----------------------------------------------------------------------
    // BUG: percent_sql regex has quadratic backtracking on crafted input
    // -----------------------------------------------------------------------
    // Pattern: ["'](?i)(SELECT|...)\s.*%[sd].*["']\s*%
    // The two `.*` separated by `%[sd]` cause O(n^2) backtracking on long
    // lines that start with "SELECT but have no %s/%d near the end.
    #[test]
    fn percent_sql_regex_does_not_hang_on_long_input() {
        // A line with "SELECT" followed by 5000 chars and no %s/%d —
        // should return quickly, not hang.
        let padding = "x".repeat(5000);
        let line = format!("\"SELECT {padding}\"");
        let start = std::time::Instant::now();
        let _ = scan_sql_injection(&line, "app.py");
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 1000,
            "BUG: regex took {}ms on crafted input — possible quadratic backtracking",
            elapsed.as_millis()
        );
    }
}
