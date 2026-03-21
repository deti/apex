//! Type-state detector: resource lifecycle violations via CPG or pattern matching.
//!
//! Detects use-after-close (CWE-416), double-free/close (CWE-675),
//! resource leak (CWE-404), and double-acquire (CWE-764) for files,
//! mutexes/locks, and database connections.

use apex_core::error::Result;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::is_test_file;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct TypeStateDetector;

#[async_trait]
impl Detector for TypeStateDetector {
    fn name(&self) -> &str {
        "typestate"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        let machines = apex_cpg::typestate::builtin_state_machines();

        // If CPG is available, use it for precise analysis.
        if let Some(ref cpg) = ctx.cpg {
            let violations = apex_cpg::typestate::analyze_typestate(cpg, &machines);
            for v in violations {
                findings.push(violation_to_finding(
                    &v.variable,
                    &v.kind,
                    v.line,
                    &v.machine_name,
                    &format!(
                        "'{}' is {} when {} was called",
                        v.variable, v.state_at_violation, v.method
                    ),
                    ctx,
                ));
            }
        }

        // Also run source-level analysis on all cached files.
        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let violations = apex_cpg::typestate::analyze_source(source, &machines);
            for v in violations {
                let mut finding = violation_to_finding(
                    &v.variable,
                    &v.kind,
                    v.line,
                    &v.machine_name,
                    &v.message,
                    ctx,
                );
                finding.file = path.clone();
                findings.push(finding);
            }
        }

        Ok(findings)
    }
}

fn violation_to_finding(
    variable: &str,
    kind: &apex_cpg::typestate::ViolationKind,
    line: u32,
    machine_name: &str,
    message: &str,
    _ctx: &AnalysisContext,
) -> Finding {
    use apex_cpg::typestate::ViolationKind;

    let (severity, category, title, suggestion) = match kind {
        ViolationKind::UseAfterClose => (
            Severity::High,
            FindingCategory::MemorySafety,
            format!("Use-after-close on '{}' ({})", variable, machine_name),
            format!(
                "Do not use '{}' after closing it. Consider restructuring the code to avoid accessing closed resources.",
                variable
            ),
        ),
        ViolationKind::DoubleFree => (
            Severity::High,
            FindingCategory::MemorySafety,
            format!("Double-close on '{}' ({})", variable, machine_name),
            format!(
                "Remove the duplicate close/free call on '{}'. Guard with a flag or restructure control flow.",
                variable
            ),
        ),
        ViolationKind::ResourceLeak => (
            Severity::Medium,
            FindingCategory::LogicBug,
            format!("Resource leak: '{}' ({}) not closed", variable, machine_name),
            format!(
                "Ensure '{}' is closed before going out of scope. Use context managers (Python: `with`), RAII (Rust), or try-finally.",
                variable
            ),
        ),
        ViolationKind::DoubleAcquire => (
            Severity::High,
            FindingCategory::LogicBug,
            format!("Double-acquire on '{}' ({})", variable, machine_name),
            format!(
                "Do not acquire '{}' while it is already held. This may cause a deadlock. Release before re-acquiring.",
                variable
            ),
        ),
    };

    Finding {
        id: Uuid::new_v4(),
        detector: "typestate".into(),
        severity,
        category,
        file: std::path::PathBuf::from("<unknown>"),
        line: if line > 0 { Some(line) } else { None },
        title,
        description: message.into(),
        evidence: vec![],
        covered: false,
        suggestion,
        explanation: None,
        fix: None,
        cwe_ids: vec![kind.cwe_id()],
        noisy: false,
        base_severity: None,
        coverage_confidence: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_ctx_with_source(path: &str, source: &str) -> AnalysisContext {
        let mut ctx = AnalysisContext::test_default();
        ctx.source_cache
            .insert(PathBuf::from(path), source.to_string());
        ctx
    }

    #[tokio::test]
    async fn detector_python_use_after_close() {
        let src = "f = open('data.txt')\nf.read()\nf.close()\nf.read()\n";
        let ctx = make_ctx_with_source("app.py", src);
        let det = TypeStateDetector;
        let findings = det.analyze(&ctx).await.unwrap();
        assert!(
            findings.iter().any(|f| f.cwe_ids.contains(&416)),
            "expected CWE-416, got: {:?}",
            findings
        );
    }

    #[tokio::test]
    async fn detector_skips_test_files() {
        let src = "f = open('data.txt')\nf.read()\n";
        let ctx = make_ctx_with_source("tests/test_app.py", src);
        let det = TypeStateDetector;
        let findings = det.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty(), "should skip test files");
    }

    #[tokio::test]
    async fn detector_resource_leak() {
        let src = "conn = sqlite3.connect('db')\nconn.execute('SELECT 1')\n";
        let ctx = make_ctx_with_source("app.py", src);
        let det = TypeStateDetector;
        let findings = det.analyze(&ctx).await.unwrap();
        assert!(
            findings.iter().any(|f| f.cwe_ids.contains(&404)),
            "expected CWE-404 resource leak, got: {:?}",
            findings
        );
    }

    #[tokio::test]
    async fn detector_name() {
        assert_eq!(TypeStateDetector.name(), "typestate");
    }
}
