//! Memory Leak Detection — identifies common memory leak patterns in code.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub struct MemoryIssue {
    pub file: PathBuf,
    pub line: u32,
    pub pattern: String,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemCheckReport {
    pub issues: Vec<MemoryIssue>,
    pub files_scanned: usize,
}

// Python patterns
#[allow(dead_code)]
static PY_GLOBAL_LIST_APPEND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*\w+\s*\.\s*append\s*\(").unwrap());
static PY_CIRCULAR_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"self\.\w+\s*=\s*self").unwrap());
static PY_LARGE_CACHE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@(?:lru_cache|cache)\s*(?:\(\s*\)|$)").unwrap());
// Rust patterns
static RS_BOX_LEAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Box::leak\b").unwrap());
static RS_MEM_FORGET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:std::mem::forget|mem::forget)\b").unwrap());

pub fn check_memory(
    source_cache: &HashMap<PathBuf, String>,
    lang: apex_core::types::Language,
) -> MemCheckReport {
    let mut issues = Vec::new();
    let mut files_scanned = 0;

    for (path, source) in source_cache {
        files_scanned += 1;
        for (line_num, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            let ln = (line_num + 1) as u32;

            match lang {
                apex_core::types::Language::Python => {
                    if PY_CIRCULAR_REF.is_match(trimmed) {
                        issues.push(MemoryIssue {
                            file: path.clone(),
                            line: ln,
                            pattern: "circular-ref".into(),
                            description: "Potential circular reference (self.x = self)".into(),
                            suggestion: "Use weakref to break circular references".into(),
                        });
                    }
                    if PY_LARGE_CACHE.is_match(trimmed) {
                        issues.push(MemoryIssue {
                            file: path.clone(),
                            line: ln,
                            pattern: "unbounded-cache".into(),
                            description: "Unbounded cache — no maxsize set".into(),
                            suggestion: "Set maxsize parameter: @lru_cache(maxsize=128)".into(),
                        });
                    }
                }
                apex_core::types::Language::Rust => {
                    if RS_BOX_LEAK.is_match(trimmed) {
                        issues.push(MemoryIssue {
                            file: path.clone(),
                            line: ln,
                            pattern: "box-leak".into(),
                            description: "Box::leak intentionally leaks memory".into(),
                            suggestion:
                                "Ensure this is intentional; consider Arc or 'static lifetime"
                                    .into(),
                        });
                    }
                    if RS_MEM_FORGET.is_match(trimmed) {
                        issues.push(MemoryIssue {
                            file: path.clone(),
                            line: ln,
                            pattern: "mem-forget".into(),
                            description: "mem::forget prevents Drop from running".into(),
                            suggestion: "Use ManuallyDrop if you need to prevent drop".into(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    MemCheckReport {
        issues,
        files_scanned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_circular_ref() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("node.py"), "self.parent = self".into());
        let r = check_memory(&c, apex_core::types::Language::Python);
        assert!(r.issues.iter().any(|i| i.pattern == "circular-ref"));
    }

    #[test]
    fn detects_unbounded_cache() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("utils.py"),
            "@lru_cache()\ndef compute(): pass".into(),
        );
        let r = check_memory(&c, apex_core::types::Language::Python);
        assert!(r.issues.iter().any(|i| i.pattern == "unbounded-cache"));
    }

    #[test]
    fn detects_box_leak() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("lib.rs"),
            "let s = Box::leak(Box::new(data));".into(),
        );
        let r = check_memory(&c, apex_core::types::Language::Rust);
        assert!(r.issues.iter().any(|i| i.pattern == "box-leak"));
    }

    #[test]
    fn detects_mem_forget() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("lib.rs"),
            "std::mem::forget(guard);".into(),
        );
        let r = check_memory(&c, apex_core::types::Language::Rust);
        assert!(r.issues.iter().any(|i| i.pattern == "mem-forget"));
    }

    #[test]
    fn no_issues_in_clean_code() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("app.py"), "def hello(): return 42".into());
        let r = check_memory(&c, apex_core::types::Language::Python);
        assert!(r.issues.is_empty());
    }
}
