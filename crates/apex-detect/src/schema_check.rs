//! Schema Migration Safety — analyzes SQL migrations for risky operations.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MigrationRisk {
    Safe,
    Caution,
    Dangerous,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationIssue {
    pub risk: MigrationRisk,
    pub line: u32,
    pub statement: String,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationReport {
    pub issues: Vec<MigrationIssue>,
    pub safe_count: usize,
    pub caution_count: usize,
    pub dangerous_count: usize,
}

static DROP_COLUMN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+DROP\s+COLUMN").unwrap()
});
static ADD_NOT_NULL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+ADD\s+(?:COLUMN\s+)?\S+\s+\S+\s+NOT\s+NULL").unwrap()
});
static HAS_DEFAULT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bDEFAULT\b").unwrap());
static DROP_TABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)DROP\s+TABLE").unwrap());
static RENAME_COL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+RENAME\s+COLUMN").unwrap()
});
static CHANGE_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+ALTER\s+COLUMN\s+\S+\s+(?:SET\s+DATA\s+)?TYPE")
        .unwrap()
});
static CREATE_INDEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)CREATE\s+(?:UNIQUE\s+)?INDEX\s+").unwrap()
});
static CONCURRENT_INDEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)CREATE\s+(?:UNIQUE\s+)?INDEX\s+CONCURRENTLY").unwrap()
});
static TRUNCATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)TRUNCATE\s+").unwrap());

pub fn analyze_migration(sql: &str) -> MigrationReport {
    let mut issues = Vec::new();

    for (line_num, line) in sql.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        let ln = (line_num + 1) as u32;

        if DROP_TABLE.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Dangerous,
                line: ln,
                statement: trimmed.into(),
                description: "DROP TABLE causes permanent data loss".into(),
                suggestion: "Rename table instead, drop after verification period".into(),
            });
        }
        if DROP_COLUMN.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Dangerous,
                line: ln,
                statement: trimmed.into(),
                description: "DROP COLUMN causes data loss and may break running queries".into(),
                suggestion: "Deprecate column first, stop reading, then drop later".into(),
            });
        }
        if ADD_NOT_NULL.is_match(trimmed) && !HAS_DEFAULT.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Dangerous,
                line: ln,
                statement: trimmed.into(),
                description: "NOT NULL without DEFAULT fails on non-empty tables".into(),
                suggestion: "Add nullable, backfill, then set NOT NULL".into(),
            });
        }
        if CHANGE_TYPE.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Caution,
                line: ln,
                statement: trimmed.into(),
                description: "Type change may require cast and locks table".into(),
                suggestion: "Add new column, migrate data, rename, drop old".into(),
            });
        }
        if RENAME_COL.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Caution,
                line: ln,
                statement: trimmed.into(),
                description: "Renaming breaks queries using the old name".into(),
                suggestion: "Add new column, migrate, update queries, drop old".into(),
            });
        }
        if CREATE_INDEX.is_match(trimmed) && !CONCURRENT_INDEX.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Caution,
                line: ln,
                statement: trimmed.into(),
                description: "CREATE INDEX without CONCURRENTLY locks table for writes".into(),
                suggestion: "Use CREATE INDEX CONCURRENTLY".into(),
            });
        }
        if TRUNCATE.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Dangerous,
                line: ln,
                statement: trimmed.into(),
                description: "TRUNCATE removes all data permanently".into(),
                suggestion: "Verify intentional, add comment explaining why".into(),
            });
        }
    }

    let dangerous_count = issues
        .iter()
        .filter(|i| i.risk == MigrationRisk::Dangerous)
        .count();
    let caution_count = issues
        .iter()
        .filter(|i| i.risk == MigrationRisk::Caution)
        .count();
    let safe_count = issues
        .iter()
        .filter(|i| i.risk == MigrationRisk::Safe)
        .count();

    MigrationReport {
        issues,
        safe_count,
        caution_count,
        dangerous_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_drop_column() {
        let r = analyze_migration("ALTER TABLE users DROP COLUMN email;");
        assert_eq!(r.dangerous_count, 1);
    }

    #[test]
    fn detects_not_null_without_default() {
        let r = analyze_migration("ALTER TABLE users ADD COLUMN age integer NOT NULL;");
        assert_eq!(r.dangerous_count, 1);
    }

    #[test]
    fn allows_not_null_with_default() {
        let r = analyze_migration("ALTER TABLE users ADD COLUMN age integer NOT NULL DEFAULT 0;");
        assert_eq!(r.dangerous_count, 0);
    }

    #[test]
    fn detects_non_concurrent_index() {
        let r = analyze_migration("CREATE INDEX idx_email ON users(email);");
        assert_eq!(r.caution_count, 1);
    }

    #[test]
    fn allows_concurrent_index() {
        let r = analyze_migration("CREATE INDEX CONCURRENTLY idx_email ON users(email);");
        assert_eq!(r.caution_count, 0);
    }

    #[test]
    fn safe_migration() {
        let r = analyze_migration("ALTER TABLE users ADD COLUMN nickname text;");
        assert_eq!(r.issues.len(), 0);
    }

    #[test]
    fn skips_comments() {
        let r = analyze_migration("-- ALTER TABLE users DROP COLUMN email;");
        assert_eq!(r.issues.len(), 0);
    }
}
