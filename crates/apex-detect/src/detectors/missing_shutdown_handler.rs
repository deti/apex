use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct MissingShutdownHandlerDetector;

/// Entry-point macros that indicate a long-running async runtime.
static RUNTIME_MACROS: &[&str] = &[
    "#[tokio::main]",
    "#[actix_web::main]",
    "#[rocket::main]",
    "#[axum::main]",
];

/// Import/use patterns that indicate a signal handler is present.
static SIGNAL_PATTERNS: &[&str] = &[
    "tokio::signal",
    "signal::ctrl_c",
    "signal_hook",
    "ctrlc",
    "unix::signal",
    "SignalKind",
    "SIGTERM",
    "SIGINT",
];

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    if lang != Language::Rust {
        return Vec::new();
    }

    // Check if the file has a runtime entry-point macro
    let runtime_line = RUNTIME_MACROS
        .iter()
        .find_map(|&mac| {
            source
                .lines()
                .enumerate()
                .find(|(_, line)| line.trim() == mac || line.trim().starts_with(mac))
                .map(|(idx, _)| (idx, mac))
        });

    let Some((macro_line_idx, macro_name)) = runtime_line else {
        return Vec::new();
    };

    // Check if the file has any signal-handling import or call
    let has_signal_handler = SIGNAL_PATTERNS
        .iter()
        .any(|&pat| source.contains(pat));

    if has_signal_handler {
        return Vec::new();
    }

    vec![Finding {
        id: Uuid::new_v4(),
        detector: "missing-shutdown-handler".into(),
        severity: Severity::Low,
        category: FindingCategory::SecuritySmell,
        file: path.to_path_buf(),
        line: Some((macro_line_idx + 1) as u32),
        title: "Async runtime entry point without graceful shutdown handler".into(),
        description: format!(
            "File has `{macro_name}` but no signal handler (`tokio::signal`, `ctrlc`, \
             or `signal_hook`). Without a shutdown handler the service cannot perform \
             graceful shutdown — in-flight requests and buffered data may be lost on SIGTERM."
        ),
        evidence: vec![],
        covered: false,
        suggestion: "Add graceful shutdown with `tokio::signal::ctrl_c().await` or \
                     `tokio::signal::unix::signal(SignalKind::terminate())` and propagate \
                     a cancellation token to active tasks."
            .into(),
        explanation: None,
        fix: None,
        cwe_ids: vec![772],
    }]
}

#[async_trait]
impl Detector for MissingShutdownHandlerDetector {
    fn name(&self) -> &str {
        "missing-shutdown-handler"
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
        analyze_source(&PathBuf::from("src/main.rs"), source, Language::Rust)
    }

    #[test]
    fn detects_tokio_main_without_signal() {
        let src = "\
#[tokio::main]
async fn main() {
    server::run().await.unwrap();
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
        assert_eq!(findings[0].cwe_ids, vec![772]);
    }

    #[test]
    fn detects_actix_main_without_signal() {
        let src = "\
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| App::new()).bind(\"0.0.0.0:8080\")?.run().await
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("shutdown"));
    }

    #[test]
    fn no_finding_with_tokio_signal() {
        let src = "\
use tokio::signal;

#[tokio::main]
async fn main() {
    let server = server::run();
    tokio::select! {
        _ = server => {},
        _ = signal::ctrl_c() => {},
    }
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_with_ctrlc_crate() {
        let src = "\
use ctrlc;

#[tokio::main]
async fn main() {
    ctrlc::set_handler(|| std::process::exit(0)).unwrap();
    server::run().await.unwrap();
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_without_runtime_macro() {
        let src = "\
async fn handler() {
    do_work().await;
}
";
        let findings = detect(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_on_non_rust_file() {
        let src = "#[tokio::main]\nasync fn main() {}";
        let findings = analyze_source(
            &PathBuf::from("src/main.py"),
            src,
            Language::Python,
        );
        assert_eq!(findings.len(), 0);
    }
}
