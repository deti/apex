use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{find_loop_scopes, in_any_scope, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct ConnectionInLoopDetector;

/// Python DB connection patterns.
static PYTHON_CONN: &[&str] = &[
    "sqlite3.connect(",
    "psycopg2.connect(",
    "pymysql.connect(",
    "mysql.connector.connect(",
    "pyodbc.connect(",
    "create_engine(",
];

/// JavaScript/TypeScript DB connection patterns.
static JS_CONN: &[&str] = &[
    "new Pool(",
    "createPool(",
    "createConnection(",
    "new Sequelize(",
    "new mongoose.Connection(",
    "MongoClient.connect(",
];

/// Rust DB connection patterns.
static RUST_CONN: &[&str] = &[
    "Pool::connect(",
    "Pool::new(",
    "Client::connect(",
    "SqliteConnection::establish(",
    "PgConnection::establish(",
    "MysqlConnection::establish(",
    "establish(",
    "connect(",
];

fn connection_patterns(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Python => PYTHON_CONN,
        Language::JavaScript => JS_CONN,
        Language::Rust => RUST_CONN,
        _ => &[],
    }
}

fn suggestion(lang: Language) -> &'static str {
    match lang {
        Language::Python => {
            "Create the connection once before the loop and reuse it, or use a \
             connection pool (e.g. `psycopg2.pool.ThreadedConnectionPool`)."
        }
        Language::JavaScript => {
            "Create the pool/connection once outside the loop. Pools are designed \
             to be shared — creating a new pool per iteration leaks connections."
        }
        Language::Rust => {
            "Create the pool or connection once before the loop. Use \
             `sqlx::Pool` or `diesel::r2d2::Pool` for connection pooling."
        }
        _ => "Create the database connection once before the loop and reuse it.",
    }
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let patterns = connection_patterns(lang);
    if patterns.is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = source.lines().collect();
    let loop_scopes = find_loop_scopes(source, lang);

    if loop_scopes.is_empty() {
        return Vec::new();
    }

    let mut findings = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment(trimmed, lang) {
            continue;
        }

        if !in_any_scope(&loop_scopes, line_idx) {
            continue;
        }

        for &pattern in patterns {
            if line.contains(pattern) {
                let line_1based = (line_idx + 1) as u32;
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "connection-in-loop".into(),
                    severity: Severity::Medium,
                    category: FindingCategory::SecuritySmell,
                    file: path.to_path_buf(),
                    line: Some(line_1based),
                    title: "Database connection created inside a loop".into(),
                    description: format!(
                        "Database connection `{pattern}` is created inside a loop. \
                         Each iteration opens a new connection, exhausting the connection \
                         pool and degrading performance. This can cause connection limit \
                         errors (CWE-400) under load."
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: suggestion(lang).into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![400],
                    noisy: false, base_severity: None, coverage_confidence: None,
                });
                break; // one finding per line
            }
        }
    }

    findings
}

#[async_trait]
impl Detector for ConnectionInLoopDetector {
    fn name(&self) -> &str {
        "connection-in-loop"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => Language::Rust,
                Some("py") => Language::Python,
                Some("js") => Language::JavaScript,
                Some("ts") | Some("tsx") => Language::JavaScript,
                _ => continue,
            };
            findings.extend(analyze_source(path, source, lang));
        }
        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn detect_python(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.py"), source, Language::Python)
    }

    fn detect_js(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.js"), source, Language::JavaScript)
    }

    fn detect_rust(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/lib.rs"), source, Language::Rust)
    }

    #[test]
    fn detects_sqlite3_connect_in_for_loop_python() {
        let src = "\
for item in items:
    conn = sqlite3.connect('data.db')
    cur = conn.cursor()
    cur.execute('INSERT INTO t VALUES (?)', (item,))
    conn.close()
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].cwe_ids, vec![400]);
    }

    #[test]
    fn detects_new_pool_in_for_loop_js() {
        let src = "\
for (let i = 0; i < records.length; i++) {
    const pool = new Pool({ connectionString: DB_URL });
    await pool.query('INSERT INTO t VALUES ($1)', [records[i]]);
    await pool.end();
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("loop"));
    }

    #[test]
    fn detects_pool_connect_in_rust_loop() {
        let src = "\
async fn process(items: Vec<String>) {
    for item in &items {
        let pool = Pool::connect(DATABASE_URL).await?;
        sqlx::query(\"INSERT INTO t VALUES ($1)\").bind(&item).execute(&pool).await?;
    }
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![400]);
    }

    #[test]
    fn no_finding_when_connection_outside_loop() {
        let src = "\
conn = sqlite3.connect('data.db')
for item in items:
    cur = conn.cursor()
    cur.execute('INSERT INTO t VALUES (?)', (item,))
conn.close()
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_psycopg2_in_while_loop() {
        let src = "\
while True:
    conn = psycopg2.connect(host='db', dbname='mydb')
    process(conn)
    conn.close()
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn no_finding_for_unsupported_language() {
        let src = "for item in items { conn = sqlite3.connect('data.db'); }";
        let findings = analyze_source(
            &PathBuf::from("src/app.rb"),
            src,
            Language::Ruby,
        );
        assert_eq!(findings.len(), 0);
    }
}
