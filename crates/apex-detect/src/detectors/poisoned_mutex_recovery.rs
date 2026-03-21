use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::is_comment;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct PoisonedMutexRecoveryDetector;

/// More specific check: the line has a lock() call AND the poison recovery pattern.
fn is_poison_recovery(line: &str) -> bool {
    let has_lock = line.contains(".lock()") || line.contains(".write()") || line.contains(".read()");
    let has_recovery = line.contains("into_inner()") && line.contains("unwrap_or_else");
    has_lock && has_recovery
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

        // Check for the poison recovery pattern
        if is_poison_recovery(line) {
            let line_1based = (line_idx + 1) as u32;
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "poisoned-mutex-recovery".into(),
                severity: Severity::Low,
                category: FindingCategory::SecuritySmell,
                file: path.to_path_buf(),
                line: Some(line_1based),
                title: "Silent mutex poison recovery via into_inner()".into(),
                description: "`.unwrap_or_else(|e| e.into_inner())` silently recovers from \
                              mutex poisoning by extracting the inner data. A mutex is poisoned \
                              when a thread panics while holding it, potentially leaving the \
                              protected data in a corrupt or partially-updated state. Silently \
                              continuing with that data can cause logic errors or security issues."
                    .into(),
                evidence: vec![],
                covered: false,
                suggestion: "Handle the poisoned guard explicitly: log the poison event, \
                             validate or reset the inner data before using it, or propagate \
                             the error. Consider using `parking_lot::Mutex` which never poisons."
                    .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![362],
                    noisy: false, base_severity: None, coverage_confidence: None,
            });
        } else {
            // Multi-line pattern: `.lock()` on one line, `.unwrap_or_else(|e| e.into_inner())`
            // may be on the next. Check a 3-line window.
            let window_end = (line_idx + 3).min(lines.len());
            let window = lines[line_idx..window_end].join(" ");
            if (window.contains(".lock()") || window.contains(".write()") || window.contains(".read()"))
                && window.contains("into_inner()")
                && window.contains("unwrap_or_else")
            {
                // Only emit if this line starts the pattern (has the lock call)
                if (line.contains(".lock()") || line.contains(".write()") || line.contains(".read()"))
                    && !findings.iter().any(|f: &Finding| {
                        f.line == Some((line_idx + 1) as u32)
                    })
                {
                    let line_1based = (line_idx + 1) as u32;
                    findings.push(Finding {
                        id: Uuid::new_v4(),
                        detector: "poisoned-mutex-recovery".into(),
                        severity: Severity::Low,
                        category: FindingCategory::SecuritySmell,
                        file: path.to_path_buf(),
                        line: Some(line_1based),
                        title: "Silent mutex poison recovery via into_inner()".into(),
                        description: "`.unwrap_or_else(|e| e.into_inner())` silently recovers from \
                                      mutex poisoning. The protected data may be in a corrupt or \
                                      partially-updated state after a panic."
                            .into(),
                        evidence: vec![],
                        covered: false,
                        suggestion: "Handle the poisoned guard explicitly or use \
                                     `parking_lot::Mutex` which never poisons."
                            .into(),
                        explanation: None,
                        fix: None,
                        cwe_ids: vec![362],
                    noisy: false, base_severity: None, coverage_confidence: None,
                    });
                }
            }
        }
    }

    findings
}

#[async_trait]
impl Detector for PoisonedMutexRecoveryDetector {
    fn name(&self) -> &str {
        "poisoned-mutex-recovery"
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
    fn detects_into_inner_unwrap_or_else() {
        let src = "\
fn get_data(m: &Mutex<Vec<i32>>) -> Vec<i32> {
    m.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
        assert_eq!(findings[0].cwe_ids, vec![362]);
    }

    #[test]
    fn detects_write_lock_poison_recovery() {
        let src = "\
fn update(m: &RwLock<HashMap<String, i32>>, k: String, v: i32) {
    let mut guard = m.write().unwrap_or_else(|e| e.into_inner());
    guard.insert(k, v);
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("poison"));
    }

    #[test]
    fn no_finding_for_proper_unwrap() {
        let src = "\
fn get_data(m: &Mutex<Vec<i32>>) -> Vec<i32> {
    m.lock().unwrap().clone()
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_for_non_rust() {
        let src = "m.lock().unwrap_or_else(|e| e.into_inner())";
        let findings = analyze_source(
            &PathBuf::from("src/app.py"),
            src,
            Language::Python,
        );
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_multiline_pattern() {
        let src = "\
fn get(m: &Mutex<Data>) {
    let guard = m.lock()
        .unwrap_or_else(|e| e.into_inner());
    use_guard(guard);
}
";
        let findings = detect(src);
        assert!(!findings.is_empty());
    }
}
