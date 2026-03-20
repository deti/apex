use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::is_comment;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct ZombieSubprocessDetector;

/// Detect `Command::` usage inside a `timeout(` block without `kill_on_drop(true)`.
///
/// When `tokio::time::timeout` fires and the future is dropped, any child process
/// created via `Command::spawn()` or `Command::output()` is NOT automatically killed
/// unless `kill_on_drop(true)` is set. This turns the child into a zombie.
fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    if lang != Language::Rust {
        return Vec::new();
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    // Walk lines looking for `timeout(` blocks that contain `Command::` without `kill_on_drop`.
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() || is_comment(trimmed, lang) {
            i += 1;
            continue;
        }

        // Detect a timeout block opener
        if line.contains("timeout(") && !line.contains("kill_on_drop") {
            // Scan forward to find the closing of the timeout call (brace-balanced).
            let block_start = i;
            let mut depth: i32 = 0;
            let mut found_command = false;
            let mut found_kill_on_drop = false;
            let mut command_line = 0usize;
            let mut block_end = i;

            for (offset, scan_line) in lines[block_start..].iter().enumerate() {
                for ch in scan_line.chars() {
                    match ch {
                        '(' | '{' => depth += 1,
                        ')' | '}' => depth -= 1,
                        _ => {}
                    }
                }

                if scan_line.contains("Command::") {
                    found_command = true;
                    command_line = block_start + offset;
                }
                if scan_line.contains("kill_on_drop") {
                    found_kill_on_drop = true;
                }

                block_end = block_start + offset;

                // Once we've balanced back (timeout call closed) stop scanning.
                if depth <= 0 && offset > 0 {
                    break;
                }
            }

            if found_command && !found_kill_on_drop {
                let line_1based = (command_line + 1) as u32;
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "zombie-subprocess".into(),
                    severity: Severity::Medium,
                    category: FindingCategory::SecuritySmell,
                    file: path.to_path_buf(),
                    line: Some(line_1based),
                    title: "Zombie subprocess after async timeout".into(),
                    description: "A child process is spawned inside a `tokio::time::timeout` \
                                  block without `kill_on_drop(true)`. When the timeout fires and \
                                  the future is dropped, the child process continues running as \
                                  a zombie and consumes resources."
                        .into(),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Call `.kill_on_drop(true)` on the `Command` before spawning, \
                                 or explicitly kill the child in a drop guard."
                        .into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![772],
                });
            }

            // Skip past the scanned block to avoid double-reporting.
            i = block_end + 1;
            continue;
        }

        i += 1;
    }

    findings
}

#[async_trait]
impl Detector for ZombieSubprocessDetector {
    fn name(&self) -> &str {
        "zombie-subprocess"
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
    fn detects_command_in_timeout_without_kill_on_drop() {
        let src = "\
async fn run_with_timeout() {
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let output = Command::new(\"ls\").output().await.unwrap();
        output
    }).await;
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].cwe_ids, vec![772]);
    }

    #[test]
    fn suppressed_by_kill_on_drop() {
        let src = "\
async fn run_with_timeout() {
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let child = Command::new(\"ls\")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        child.wait_with_output().await.unwrap()
    }).await;
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_without_timeout_wrapper() {
        let src = "\
async fn run_command() {
    let output = Command::new(\"ls\").output().await.unwrap();
    println!(\"{:?}\", output);
}
";
        let findings = detect(src);
        // No timeout block, so no zombie subprocess finding
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_in_non_rust_file() {
        let src = "timeout(5, subprocess.run(['ls']))";
        let findings = analyze_source(
            &PathBuf::from("src/app.py"),
            src,
            Language::Python,
        );
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_command_spawn_in_timeout() {
        let src = "\
async fn timed_spawn() {
    let _ = timeout(Duration::from_millis(100), async {
        let mut child = Command::new(\"sleep\").arg(\"10\").spawn().unwrap();
        child.wait().await.unwrap()
    }).await;
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("Zombie subprocess"));
    }
}
