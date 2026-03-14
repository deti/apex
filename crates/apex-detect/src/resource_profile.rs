//! Resource Utilization Profiling — identifies resource-intensive patterns.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub struct ResourceIssue {
    pub file: PathBuf,
    pub line: u32,
    pub category: String,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceReport {
    pub issues: Vec<ResourceIssue>,
    pub files_scanned: usize,
}

#[allow(dead_code)]
static NESTED_LOOP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)for\s+\w+\s+in\s+.*:\s*$").unwrap());
static LARGE_ALLOC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:\[0\]\s*\*\s*\d{6,}|vec!\[.*;\s*\d{6,}\]|malloc\(\d{6,}\))").unwrap()
});
#[allow(dead_code)]
static SYNC_IO_IN_ASYNC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:open\(|read\(|write\()").unwrap());
#[allow(dead_code)]
static N_PLUS_ONE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)for\s+\w+\s+in\s+.*:\s*\n\s+.*(?:query|execute|fetch|select)").unwrap()
});
#[allow(dead_code)]
static UNBOUNDED_COLLECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.collect\(\)").unwrap());

pub fn profile_resources(source_cache: &HashMap<PathBuf, String>) -> ResourceReport {
    let mut issues = Vec::new();
    let mut files_scanned = 0;

    for (path, source) in source_cache {
        files_scanned += 1;
        for (line_num, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            let ln = (line_num + 1) as u32;

            if LARGE_ALLOC.is_match(trimmed) {
                issues.push(ResourceIssue {
                    file: path.clone(),
                    line: ln,
                    category: "memory".into(),
                    description: "Large allocation detected".into(),
                    suggestion: "Consider streaming or chunked processing".into(),
                });
            }
        }
    }

    ResourceReport {
        issues,
        files_scanned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_large_alloc_python() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("data.py"), "buffer = [0] * 10000000".into());
        let r = profile_resources(&c);
        assert!(r.issues.iter().any(|i| i.category == "memory"));
    }

    #[test]
    fn detects_large_alloc_rust() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("data.rs"),
            "let buf = vec![0u8; 1000000];".into(),
        );
        let r = profile_resources(&c);
        assert!(r.issues.iter().any(|i| i.category == "memory"));
    }

    #[test]
    fn clean_code_no_issues() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("app.py"), "x = [1, 2, 3]".into());
        let r = profile_resources(&c);
        assert!(r.issues.is_empty());
    }

    #[test]
    fn empty_source_no_issues() {
        let r = profile_resources(&HashMap::new());
        assert!(r.issues.is_empty());
    }
}
