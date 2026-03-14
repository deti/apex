//! Migration Completeness Check — detects incomplete API/library migrations.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct MigrationFile {
    pub file: PathBuf,
    pub old_refs: usize,
    pub new_refs: usize,
    pub status: MigrationStatus,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum MigrationStatus {
    NotStarted,
    InProgress,
    Complete,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationReport {
    pub old_api: String,
    pub new_api: String,
    pub files: Vec<MigrationFile>,
    pub total_old_refs: usize,
    pub total_new_refs: usize,
    pub progress_pct: f64,
}

pub fn check_migration(
    source_cache: &HashMap<PathBuf, String>,
    old_api: &str,
    new_api: &str,
) -> MigrationReport {
    let old_re =
        Regex::new(&regex::escape(old_api)).unwrap_or_else(|_| Regex::new("$^").unwrap());
    let new_re =
        Regex::new(&regex::escape(new_api)).unwrap_or_else(|_| Regex::new("$^").unwrap());

    let mut files = Vec::new();
    let mut total_old = 0usize;
    let mut total_new = 0usize;

    for (path, source) in source_cache {
        let old_count = old_re.find_iter(source).count();
        let new_count = new_re.find_iter(source).count();

        if old_count > 0 || new_count > 0 {
            let status = if old_count > 0 && new_count > 0 {
                MigrationStatus::InProgress
            } else if old_count > 0 {
                MigrationStatus::NotStarted
            } else {
                MigrationStatus::Complete
            };

            total_old += old_count;
            total_new += new_count;

            files.push(MigrationFile {
                file: path.clone(),
                old_refs: old_count,
                new_refs: new_count,
                status,
            });
        }
    }

    let total = total_old + total_new;
    let progress_pct = if total > 0 {
        (total_new as f64 / total as f64) * 100.0
    } else {
        100.0
    };

    files.sort_by(|a, b| b.old_refs.cmp(&a.old_refs)); // most old refs first

    MigrationReport {
        old_api: old_api.into(),
        new_api: new_api.into(),
        files,
        total_old_refs: total_old,
        total_new_refs: total_new,
        progress_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_partial_migration() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("a.py"),
            "old_client.call()\nnew_client.call()".into(),
        );
        let r = check_migration(&c, "old_client", "new_client");
        assert_eq!(r.files.len(), 1);
        assert!(matches!(r.files[0].status, MigrationStatus::InProgress));
    }

    #[test]
    fn detects_not_started() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("a.py"), "old_client.call()".into());
        let r = check_migration(&c, "old_client", "new_client");
        assert!(matches!(r.files[0].status, MigrationStatus::NotStarted));
        assert_eq!(r.progress_pct, 0.0);
    }

    #[test]
    fn detects_complete() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("a.py"), "new_client.call()".into());
        let r = check_migration(&c, "old_client", "new_client");
        assert!(matches!(r.files[0].status, MigrationStatus::Complete));
        assert_eq!(r.progress_pct, 100.0);
    }

    #[test]
    fn no_refs_empty() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("a.py"), "unrelated code".into());
        let r = check_migration(&c, "old_client", "new_client");
        assert!(r.files.is_empty());
    }

    #[test]
    fn progress_percentage() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("a.py"),
            "old_client\nold_client\nnew_client".into(),
        );
        let r = check_migration(&c, "old_client", "new_client");
        assert!(r.progress_pct > 30.0 && r.progress_pct < 40.0); // 1/3 = 33%
    }
}
