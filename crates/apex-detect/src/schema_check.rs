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

static DROP_COLUMN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+DROP\s+COLUMN").unwrap());
static ADD_NOT_NULL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+ADD\s+(?:COLUMN\s+)?\S+\s+\S+\s+NOT\s+NULL").unwrap()
});
static HAS_DEFAULT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bDEFAULT\b").unwrap());
static DROP_TABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)DROP\s+TABLE").unwrap());
static RENAME_COL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+RENAME\s+COLUMN").unwrap());
static CHANGE_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+ALTER\s+COLUMN\s+\S+\s+(?:SET\s+DATA\s+)?TYPE").unwrap()
});
static CREATE_INDEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)CREATE\s+(?:UNIQUE\s+)?INDEX\s+").unwrap());
static CONCURRENT_INDEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)CREATE\s+(?:UNIQUE\s+)?INDEX\s+CONCURRENTLY").unwrap());
static TRUNCATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)TRUNCATE\s+").unwrap());

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

    // -----------------------------------------------------------------------
    // Additional coverage: DROP TABLE, RENAME COLUMN, CHANGE TYPE, TRUNCATE,
    // multiple issues, case insensitivity, UNIQUE INDEX variations
    // -----------------------------------------------------------------------

    #[test]
    fn detects_drop_table() {
        // DROP TABLE is Dangerous
        let r = analyze_migration("DROP TABLE users;");
        assert_eq!(r.dangerous_count, 1);
        assert_eq!(r.issues[0].risk, MigrationRisk::Dangerous);
        assert!(r.issues[0].description.contains("permanent data loss"));
    }

    #[test]
    fn detects_drop_table_if_exists() {
        // DROP TABLE IF EXISTS is also dangerous
        let r = analyze_migration("DROP TABLE IF EXISTS users;");
        assert_eq!(r.dangerous_count, 1);
    }

    #[test]
    fn detects_rename_column() {
        // RENAME COLUMN is Caution
        let r = analyze_migration("ALTER TABLE users RENAME COLUMN email TO user_email;");
        assert_eq!(r.caution_count, 1);
        assert_eq!(r.issues[0].risk, MigrationRisk::Caution);
        assert!(r.issues[0].description.contains("old name"));
    }

    #[test]
    fn detects_change_type_set_data_type() {
        // ALTER COLUMN ... SET DATA TYPE is Caution
        let r = analyze_migration("ALTER TABLE users ALTER COLUMN age SET DATA TYPE bigint;");
        assert_eq!(r.caution_count, 1);
        assert_eq!(r.issues[0].risk, MigrationRisk::Caution);
    }

    #[test]
    fn detects_change_type_without_set_data() {
        // ALTER COLUMN ... TYPE (short form) is also Caution
        let r = analyze_migration("ALTER TABLE users ALTER COLUMN score TYPE numeric;");
        assert_eq!(r.caution_count, 1);
    }

    #[test]
    fn detects_truncate() {
        // TRUNCATE is Dangerous
        let r = analyze_migration("TRUNCATE TABLE audit_log;");
        assert_eq!(r.dangerous_count, 1);
        assert_eq!(r.issues[0].risk, MigrationRisk::Dangerous);
        assert!(r.issues[0]
            .description
            .contains("removes all data permanently"));
    }

    #[test]
    fn detects_multiple_issues_in_one_migration() {
        // A migration with several risky statements accumulates all counts
        let sql = "\
DROP TABLE old_users;\n\
ALTER TABLE users DROP COLUMN legacy;\n\
TRUNCATE TABLE sessions;\n\
ALTER TABLE posts ALTER COLUMN body TYPE text;\n\
CREATE INDEX idx_posts_title ON posts(title);\n\
";
        let r = analyze_migration(sql);
        assert_eq!(r.dangerous_count, 3, "DROP TABLE + DROP COLUMN + TRUNCATE");
        assert_eq!(r.caution_count, 2, "CHANGE TYPE + non-concurrent INDEX");
    }

    #[test]
    fn case_insensitive_drop_table_lowercase() {
        // All patterns are (?i) — lowercase should still match
        let r = analyze_migration("drop table users;");
        assert_eq!(r.dangerous_count, 1);
    }

    #[test]
    fn case_insensitive_rename_column_mixed() {
        let r = analyze_migration("Alter Table users Rename Column email To mail;");
        assert_eq!(r.caution_count, 1);
    }

    #[test]
    fn case_insensitive_truncate_uppercase() {
        let r = analyze_migration("TRUNCATE TABLE big_table;");
        assert_eq!(r.dangerous_count, 1);
    }

    #[test]
    fn unique_index_without_concurrently_is_caution() {
        // CREATE UNIQUE INDEX without CONCURRENTLY
        let r = analyze_migration("CREATE UNIQUE INDEX idx_email ON users(email);");
        assert_eq!(r.caution_count, 1);
    }

    #[test]
    fn unique_index_with_concurrently_is_safe() {
        // CREATE UNIQUE INDEX CONCURRENTLY — no caution
        let r = analyze_migration("CREATE UNIQUE INDEX CONCURRENTLY idx_email ON users(email);");
        assert_eq!(r.caution_count, 0);
    }

    #[test]
    fn empty_sql_produces_no_issues() {
        let r = analyze_migration("");
        assert_eq!(r.issues.len(), 0);
        assert_eq!(r.safe_count, 0);
        assert_eq!(r.caution_count, 0);
        assert_eq!(r.dangerous_count, 0);
    }

    #[test]
    fn only_whitespace_and_comments_produces_no_issues() {
        let sql = "   \n-- comment\n   \n-- another comment\n";
        let r = analyze_migration(sql);
        assert_eq!(r.issues.len(), 0);
    }

    #[test]
    fn line_numbers_are_correct() {
        // Issue on line 3 should report line = 3
        let sql = "-- safe\n\nDROP TABLE users;\n";
        let r = analyze_migration(sql);
        assert_eq!(r.issues.len(), 1);
        assert_eq!(r.issues[0].line, 3);
    }

    #[test]
    fn statement_field_is_trimmed_line_content() {
        let r = analyze_migration("  DROP TABLE users;  ");
        assert_eq!(r.issues.len(), 1);
        assert_eq!(r.issues[0].statement, "DROP TABLE users;");
    }

    #[test]
    fn not_null_with_default_is_safe() {
        // ADD COLUMN NOT NULL DEFAULT x should not fire
        let r = analyze_migration("ALTER TABLE users ADD COLUMN score integer NOT NULL DEFAULT 0;");
        assert_eq!(r.dangerous_count, 0);
    }

    #[test]
    fn add_column_not_null_missing_default_fires() {
        // No DEFAULT — should be dangerous
        let r = analyze_migration("ALTER TABLE users ADD score integer NOT NULL;");
        assert_eq!(r.dangerous_count, 1);
    }

    #[test]
    fn add_column_with_column_keyword_not_null_no_default() {
        // "ADD COLUMN col_name type NOT NULL" — with COLUMN keyword
        let r = analyze_migration("ALTER TABLE users ADD COLUMN active boolean NOT NULL;");
        assert_eq!(r.dangerous_count, 1);
    }
}
