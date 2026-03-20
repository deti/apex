use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{find_async_fn_scopes, in_any_scope, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct MissingAsyncTimeoutDetector;

/// Rust async I/O patterns that require a timeout wrapper.
static RUST_ASYNC_IO: &[&str] = &[
    "TcpStream::connect(",
    "tokio::net::TcpStream::connect(",
    "tokio::net::UdpSocket::bind(",
    "tokio::net::TcpListener::bind(",
    "reqwest::Client::new(",
    "reqwest::get(",
    ".get(",
    ".post(",
    ".put(",
    ".delete(",
    ".send(",
];

/// Suppression: `tokio::time::timeout` anywhere in the async fn scope suppresses.
fn has_timeout_in_source(source: &str) -> bool {
    source.contains("tokio::time::timeout") || source.contains("timeout(")
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    if lang != Language::Rust {
        return Vec::new();
    }

    let lines: Vec<&str> = source.lines().collect();
    let async_scopes = find_async_fn_scopes(source, lang);

    if async_scopes.is_empty() {
        return Vec::new();
    }

    let mut findings = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment(trimmed, lang) {
            continue;
        }

        if !in_any_scope(&async_scopes, line_idx) {
            continue;
        }

        // Skip if this line or nearby context already has a timeout wrapper
        if line.contains("timeout(") {
            continue;
        }

        for pattern in RUST_ASYNC_IO {
            if line.contains(pattern) {
                // Check if the surrounding async fn scope has a timeout call.
                // Find which scope we're in and check the full scope source.
                let scope_has_timeout = async_scopes.iter().any(|s| {
                    if line_idx >= s.start_line && line_idx <= s.end_line {
                        let scope_source = lines[s.start_line..=s.end_line].join("\n");
                        has_timeout_in_source(&scope_source)
                    } else {
                        false
                    }
                });

                if scope_has_timeout {
                    break;
                }

                let line_1based = (line_idx + 1) as u32;
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "missing-async-timeout".into(),
                    severity: Severity::Medium,
                    category: FindingCategory::SecuritySmell,
                    file: path.to_path_buf(),
                    line: Some(line_1based),
                    title: "Async I/O without timeout in Rust async fn".into(),
                    description: format!(
                        "Async I/O call `{}` inside an async function has no \
                         `tokio::time::timeout` wrapper. Without a timeout, this \
                         future can hang indefinitely under network or resource contention.",
                        pattern.trim_end_matches('(')
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Wrap with `tokio::time::timeout(Duration::from_secs(N), ...)` \
                                 or set a deadline via `tokio::time::timeout_at`."
                        .into(),
                    explanation: None,
                    fix: None,
                    cwe_ids: vec![400],
                    noisy: false,
                });
                break; // one finding per line
            }
        }
    }

    findings
}

#[async_trait]
impl Detector for MissingAsyncTimeoutDetector {
    fn name(&self) -> &str {
        "missing-async-timeout"
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
    fn detects_tcpstream_connect_without_timeout() {
        let src = "\
async fn connect_server() {
    let stream = TcpStream::connect(\"127.0.0.1:8080\").await.unwrap();
    stream.write_all(b\"hello\").await.unwrap();
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].cwe_ids, vec![400]);
    }

    #[test]
    fn detects_reqwest_get_without_timeout() {
        let src = "\
async fn fetch_data() {
    let resp = reqwest::get(\"https://api.example.com/data\").await.unwrap();
    println!(\"{:?}\", resp.status());
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("timeout"));
    }

    #[test]
    fn suppressed_by_tokio_timeout_wrapper() {
        let src = "\
async fn connect_server() {
    let stream = tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect(\"127.0.0.1:8080\"),
    ).await.unwrap().unwrap();
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_in_sync_fn() {
        let src = "\
fn connect_server() {
    let stream = TcpStream::connect(\"127.0.0.1:8080\").unwrap();
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_on_non_rust_file() {
        let src = "async function fetch() { await fetch('http://a.com'); }";
        let findings = analyze_source(
            &PathBuf::from("src/app.js"),
            src,
            Language::JavaScript,
        );
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_tokio_net_connect() {
        let src = "\
async fn make_connection() {
    let conn = tokio::net::TcpStream::connect(addr).await?;
    Ok(conn)
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 1);
    }
}
