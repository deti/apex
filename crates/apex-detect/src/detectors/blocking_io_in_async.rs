use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{find_async_fn_scopes, in_any_scope, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct BlockingIoInAsyncDetector;

// ---------------------------------------------------------------------------
// Per-language blocking patterns
// ---------------------------------------------------------------------------

/// Rust blocking patterns inside async fn
static RUST_BLOCKING: &[&str] = &[
    "std::fs::",
    "std::io::stdin",
    "std::thread::sleep",
    "reqwest::blocking",
    "fs::read(",
    "fs::write(",
    "fs::read_to_string(",
    "fs::File::open(",
    "fs::File::create(",
    "File::open(",
    "File::create(",
];

/// Python blocking patterns inside async def
static PYTHON_BLOCKING: &[&str] = &["time.sleep(", "open("];

/// JS blocking patterns inside async function
static JS_BLOCKING: &[&str] = &[
    "readFileSync(",
    "writeFileSync(",
    "appendFileSync(",
    "existsSync(",
    "readdirSync(",
    "statSync(",
    "mkdirSync(",
    "unlinkSync(",
    "execSync(",
    "spawnSync(",
];

/// Rust suppression: `spawn_blocking` within N lines above suppresses the finding.
const SPAWN_BLOCKING_WINDOW: usize = 3;

fn is_suppressed_rust(lines: &[&str], line_idx: usize) -> bool {
    let start = line_idx.saturating_sub(SPAWN_BLOCKING_WINDOW);
    lines[start..=line_idx]
        .iter()
        .any(|l| l.contains("spawn_blocking"))
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let async_scopes = find_async_fn_scopes(source, lang);

    if async_scopes.is_empty() {
        return Vec::new();
    }

    let blocking_patterns: &[&str] = match lang {
        Language::Rust => RUST_BLOCKING,
        Language::Python => PYTHON_BLOCKING,
        Language::JavaScript => JS_BLOCKING,
        _ => return Vec::new(),
    };

    let mut findings = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment(trimmed, lang) {
            continue;
        }

        if !in_any_scope(&async_scopes, line_idx) {
            continue;
        }

        for pattern in blocking_patterns {
            if line.contains(pattern) {
                // Rust suppression: spawn_blocking nearby
                if lang == Language::Rust && is_suppressed_rust(&lines, line_idx) {
                    break;
                }

                let line_1based = (line_idx + 1) as u32;
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "blocking-io-in-async".into(),
                    severity: Severity::Medium,
                    category: FindingCategory::SecuritySmell,
                    file: path.to_path_buf(),
                    line: Some(line_1based),
                    title: "Blocking I/O inside async function".into(),
                    description: format!(
                        "Blocking operation `{}` used inside an async function. \
                         This blocks the async runtime thread and can cause deadlocks \
                         or severe latency spikes under load.",
                        pattern.trim_end_matches('(')
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: match lang {
                        Language::Rust => "Use `tokio::fs`, `tokio::time::sleep`, or wrap with \
                             `tokio::task::spawn_blocking`"
                            .into(),
                        Language::Python => {
                            "Use `asyncio.sleep`, `aiofiles.open`, or `asyncio.to_thread`".into()
                        }
                        _ => "Use async equivalents: `fs.promises.*`, `child_process.exec`".into(),
                    },
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![400],
                });
                break; // one finding per line
            }
        }
    }

    findings
}

#[async_trait]
impl Detector for BlockingIoInAsyncDetector {
    fn name(&self) -> &str {
        "blocking-io-in-async"
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

    fn detect_rust(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/lib.rs"), source, Language::Rust)
    }

    fn detect_python(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.py"), source, Language::Python)
    }

    fn detect_js(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.js"), source, Language::JavaScript)
    }

    // ---- positive: Rust ----

    #[test]
    fn detects_std_fs_in_rust_async() {
        let src = "\
async fn handler() {
    let data = std::fs::read_to_string(\"file.txt\").unwrap();
    process(data);
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].cwe_ids, vec![400]);
    }

    #[test]
    fn detects_thread_sleep_in_rust_async() {
        let src = "\
async fn wait() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("Blocking I/O"));
    }

    // ---- positive: Python ----

    #[test]
    fn detects_time_sleep_in_async_def() {
        let src = "\
async def handler():
    time.sleep(1)
    return 'done'
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cwe_ids, vec![400]);
    }

    #[test]
    fn detects_open_in_async_def() {
        let src = "\
async def read_file():
    f = open('data.txt')
    return f.read()
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- positive: JS ----

    #[test]
    fn detects_read_file_sync_in_js_async() {
        let src = "\
async function loadConfig() {
    const data = fs.readFileSync('config.json', 'utf8');
    return JSON.parse(data);
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn detects_exec_sync_in_js_async() {
        let src = "\
async function run() {
    const out = execSync('ls -la');
    return out.toString();
}
";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
    }

    // ---- negative: suppression ----

    #[test]
    fn suppressed_by_spawn_blocking_rust() {
        let src = "\
async fn handler() {
    let result = tokio::task::spawn_blocking(|| {
        std::fs::read_to_string(\"file.txt\")
    }).await.unwrap();
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- negative: sync fn not flagged ----

    #[test]
    fn no_finding_in_sync_rust_fn() {
        let src = "\
fn handler() {
    let data = std::fs::read_to_string(\"file.txt\").unwrap();
    process(data);
}
";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_in_sync_python_fn() {
        let src = "\
def read_file():
    f = open('data.txt')
    return f.read()
";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 0);
    }

    // ---- edge: empty async fn ----

    #[test]
    fn empty_async_fn_no_finding() {
        let src = "async fn noop() {}\n";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0);
    }
}
