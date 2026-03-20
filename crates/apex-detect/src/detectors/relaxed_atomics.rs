use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::is_comment;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct RelaxedAtomicsDetector;

/// Atomic operations where `Ordering::Relaxed` provides no inter-thread synchronization.
static ATOMIC_OPS: &[&str] = &[
    ".fetch_add(",
    ".fetch_sub(",
    ".fetch_and(",
    ".fetch_or(",
    ".fetch_xor(",
    ".store(",
    ".load(",
    ".swap(",
    ".compare_exchange(",
    ".compare_exchange_weak(",
];

/// Heuristics that indicate a variable is shared across threads (static or Arc-wrapped).
static SHARED_INDICATORS: &[&str] = &[
    "static ",
    "Arc<",
    "Arc::new(",
    "Mutex<",
    "RwLock<",
    "AtomicUsize",
    "AtomicI64",
    "AtomicU64",
    "AtomicI32",
    "AtomicU32",
    "AtomicBool",
    "AtomicPtr",
    "Atomic",
];

/// Returns true if `line_idx` falls inside a `#[cfg(test)]` block or a `mod tests` block.
///
/// Heuristic: scan backwards from `line_idx` looking for `#[cfg(test)]` or `mod tests`.
fn in_test_block(lines: &[&str], line_idx: usize) -> bool {
    for i in (0..line_idx).rev() {
        let trimmed = lines[i].trim();
        if trimmed.contains("#[cfg(test)]") || trimmed.contains("mod tests") {
            return true;
        }
    }
    false
}

/// Returns true if the variable named `var_name` is a local `let` binding in `source`.
/// Local atomics on the stack are single-threaded by definition — Relaxed is safe there.
fn is_local_atomic_binding(source: &str, var_name: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        // Match `let <var_name> =` or `let mut <var_name> =` — local binding only
        if (trimmed.starts_with(&format!("let {} =", var_name))
            || trimmed.starts_with(&format!("let mut {} =", var_name)))
            && !trimmed.contains("static ")
        {
            return true;
        }
    }
    false
}

fn is_likely_shared(source: &str, var_name: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.contains(var_name) {
            for indicator in SHARED_INDICATORS {
                if trimmed.contains(indicator) {
                    return true;
                }
            }
        }
    }
    false
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    if lang != Language::Rust {
        return Vec::new();
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment(trimmed, lang) {
            continue;
        }

        // Must contain Relaxed ordering AND an atomic operation
        if !line.contains("Ordering::Relaxed") && !line.contains("Relaxed)") {
            continue;
        }

        let has_atomic_op = ATOMIC_OPS.iter().any(|op| line.contains(op));
        if !has_atomic_op {
            continue;
        }

        // Skip findings inside #[cfg(test)] / mod tests blocks
        if in_test_block(&lines, line_idx) {
            continue;
        }

        // Extract the variable name being operated on (heuristic: word before the `.op(`)
        let var_name = extract_receiver(line).unwrap_or_default();

        // Skip local (stack) atomics — Relaxed is safe for thread-local use
        if !var_name.is_empty() && is_local_atomic_binding(source, &var_name) {
            continue;
        }

        // Check if this atomic is on a shared variable (static, Arc, etc.)
        let is_shared = (!var_name.is_empty() && is_likely_shared(source, &var_name))
            || source.contains("Arc<")
            || source.contains("static ")
            || line.contains("COUNTER")
            || line.contains("counter")
            || line.contains("COUNT")
            || line.contains("FLAG")
            || line.contains("flag");

        if !is_shared {
            continue;
        }

        let line_1based = (line_idx + 1) as u32;
        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "relaxed-atomics".into(),
            severity: Severity::Medium,
            category: FindingCategory::SecuritySmell,
            file: path.to_path_buf(),
            line: Some(line_1based),
            title: "Relaxed memory ordering on shared atomic".into(),
            description: "`Ordering::Relaxed` provides no synchronization guarantees — only \
                          atomicity. On shared state accessed from multiple threads, this can \
                          cause data races or stale reads. For counters/flags, use \
                          `SeqCst` or `AcqRel`/`Release`+`Acquire` pairs."
                .into(),
            evidence: vec![],
            covered: false,
            suggestion: "Replace `Ordering::Relaxed` with `Ordering::SeqCst` for simple cases, \
                         or use `Release` on writes and `Acquire` on reads for performance-critical \
                         shared state."
                .into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![362],
            noisy: false,
        });
    }

    findings
}

/// Extract the receiver variable name from a line like `foo.fetch_add(1, Ordering::Relaxed)`.
fn extract_receiver(line: &str) -> Option<String> {
    // Find the first `.` that precedes an atomic op
    let dot_pos = line.find('.')?;
    let before_dot = line[..dot_pos].trim();
    // Take the last identifier token
    let name: String = before_dot
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if name.is_empty() { None } else { Some(name) }
}

#[async_trait]
impl Detector for RelaxedAtomicsDetector {
    fn name(&self) -> &str {
        "relaxed-atomics"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => Language::Rust,
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

    fn detect(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/lib.rs"), source, Language::Rust)
    }

    #[test]
    fn detects_fetch_add_relaxed_on_static() {
        let src = "\
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn increment() {
    COUNTER.fetch_add(1, Ordering::Relaxed);
}
";
        let findings = detect(src);
        assert!(!findings.is_empty());
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].cwe_ids, vec![362]);
    }

    #[test]
    fn detects_store_relaxed_on_arc_atomic() {
        let src = "\
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

fn set_flag(flag: Arc<AtomicBool>) {
    flag.store(true, Ordering::Relaxed);
}
";
        let findings = detect(src);
        assert!(!findings.is_empty());
        assert_eq!(findings[0].cwe_ids, vec![362]);
    }

    #[test]
    fn no_finding_for_seqcst() {
        let src = "\
static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn increment() {
    COUNTER.fetch_add(1, Ordering::SeqCst);
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_for_non_rust() {
        let src = "counter.fetch_add(1, Ordering::Relaxed)";
        let findings = analyze_source(
            &PathBuf::from("src/app.py"),
            src,
            Language::Python,
        );
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_load_relaxed_on_shared_flag() {
        let src = "\
use std::sync::atomic::{AtomicBool, Ordering};

static FLAG: AtomicBool = AtomicBool::new(false);

fn check() -> bool {
    FLAG.load(Ordering::Relaxed)
}
";
        let findings = detect(src);
        assert!(!findings.is_empty());
        assert!(findings[0].title.contains("Relaxed memory ordering"));
    }

    // --- Suppression regression tests ---

    #[test]
    fn no_finding_in_cfg_test_block() {
        let src = "\
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn increment() {
    COUNTER.fetch_add(1, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        // In tests, Relaxed is acceptable for single-threaded test environments
        COUNTER.fetch_add(1, Ordering::Relaxed);
        assert!(COUNTER.load(Ordering::Relaxed) > 0);
    }
}
";
        let findings = detect(src);
        assert_eq!(
            findings.len(),
            0,
            "Relaxed ordering inside #[cfg(test)] block should be suppressed"
        );
    }

    #[test]
    fn no_finding_for_local_atomic() {
        let src = "\
use std::sync::atomic::{AtomicUsize, Ordering};

fn compute() -> usize {
    let local = AtomicUsize::new(0);
    local.fetch_add(1, Ordering::Relaxed);
    local.load(Ordering::Relaxed)
}
";
        let findings = detect(src);
        assert_eq!(
            findings.len(),
            0,
            "Local stack atomic with Relaxed ordering should not be flagged"
        );
    }

    #[test]
    fn no_finding_for_local_mut_atomic() {
        let src = "\
use std::sync::atomic::{AtomicUsize, Ordering};

fn compute() -> usize {
    let mut counter = AtomicUsize::new(0);
    counter.fetch_add(1, Ordering::Relaxed);
    counter.load(Ordering::Relaxed)
}
";
        let findings = detect(src);
        assert_eq!(
            findings.len(),
            0,
            "Local mut stack atomic with Relaxed ordering should not be flagged"
        );
    }

    #[test]
    fn still_detects_relaxed_on_static_outside_tests() {
        let src = "\
use std::sync::atomic::{AtomicUsize, Ordering};

static SHARED: AtomicUsize = AtomicUsize::new(0);

pub fn add(n: usize) {
    SHARED.fetch_add(n, Ordering::Relaxed);
}
";
        let findings = detect(src);
        assert!(
            !findings.is_empty(),
            "Static shared atomic with Relaxed outside test block should still be detected"
        );
    }

    #[test]
    fn still_detects_relaxed_on_arc_outside_tests() {
        let src = "\
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

pub fn flip(flag: Arc<AtomicBool>) {
    flag.store(false, Ordering::Relaxed);
}
";
        let findings = detect(src);
        assert!(
            !findings.is_empty(),
            "Arc-wrapped atomic with Relaxed outside test block should still be detected"
        );
    }
}
