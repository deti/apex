use std::collections::HashMap;
use std::time::Duration;

use apex_core::error::ApexError;
use apex_core::types::Language;
use tracing::warn;

use crate::config::DetectConfig;
use crate::context::AnalysisContext;
use crate::detectors::*;
use crate::finding::{Finding, FindingCategory};
use crate::report::AnalysisReport;
use crate::Detector;

pub struct DetectorPipeline {
    pub(crate) detectors: Vec<Box<dyn Detector>>,
}

impl DetectorPipeline {
    pub fn new(detectors: Vec<Box<dyn Detector>>) -> Self {
        Self { detectors }
    }

    pub fn from_config(cfg: &DetectConfig, lang: Language) -> Self {
        let mut detectors: Vec<Box<dyn Detector>> = Vec::new();

        if cfg.enabled.contains(&"panic".to_string()) {
            detectors.push(Box::new(PanicPatternDetector));
        }
        if cfg.enabled.contains(&"unsafe".to_string()) && lang == Language::Rust {
            detectors.push(Box::new(UnsafeReachabilityDetector));
        }
        if cfg.enabled.contains(&"deps".to_string()) {
            detectors.push(Box::new(DependencyAuditDetector));
        }
        if cfg.enabled.contains(&"static".to_string()) {
            detectors.push(Box::new(StaticAnalysisDetector::new(&cfg.static_analysis)));
        }
        if cfg.enabled.contains(&"security".to_string()) {
            detectors.push(Box::new(SecurityPatternDetector));
        }
        if cfg.enabled.contains(&"secrets".to_string()) {
            detectors.push(Box::new(HardcodedSecretDetector));
        }

        Self { detectors }
    }

    pub async fn run_all(&self, ctx: &AnalysisContext) -> AnalysisReport {
        let per_detector_timeout = ctx
            .config
            .per_detector_timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(300));

        let (pure, subprocess): (Vec<_>, Vec<_>) = self
            .detectors
            .iter()
            .partition(|d| !d.uses_cargo_subprocess());

        // Pure detectors run concurrently
        let pure_futs = pure.iter().map(|d| {
            let timeout = per_detector_timeout;
            async move {
                let name = d.name().to_string();
                match tokio::time::timeout(timeout, d.analyze(ctx)).await {
                    Ok(result) => (name, result),
                    Err(_) => (name, Err(ApexError::Timeout(timeout.as_millis() as u64))),
                }
            }
        });
        let pure_results = futures::future::join_all(pure_futs);

        // Subprocess detectors run sequentially (Cargo.lock contention)
        let subprocess_results = async {
            let mut results = Vec::new();
            for d in &subprocess {
                let name = d.name().to_string();
                let result = match tokio::time::timeout(per_detector_timeout, d.analyze(ctx)).await
                {
                    Ok(r) => r,
                    Err(_) => Err(ApexError::Timeout(per_detector_timeout.as_millis() as u64)),
                };
                results.push((name, result));
            }
            results
        };

        let (pure_res, sub_res) = tokio::join!(pure_results, subprocess_results);

        let mut findings = Vec::new();
        let mut detector_status = Vec::new();

        for (name, result) in pure_res.into_iter().chain(sub_res) {
            match result {
                Ok(f) => {
                    detector_status.push((name, true));
                    findings.extend(f);
                }
                Err(e) => {
                    warn!(detector = %name, error = %e, "detector failed");
                    detector_status.push((name, false));
                }
            }
        }

        deduplicate(&mut findings);
        findings.sort_by_key(|f| (f.severity.rank(), f.covered as u8));

        AnalysisReport {
            findings,
            detector_status,
        }
    }
}

pub fn deduplicate(findings: &mut Vec<Finding>) {
    let mut seen: HashMap<(std::path::PathBuf, Option<u32>, FindingCategory), usize> =
        HashMap::new();
    let mut merged: Vec<Finding> = Vec::new();

    for finding in findings.drain(..) {
        let key = (finding.file.clone(), finding.line, finding.category);
        if let Some(&idx) = seen.get(&key) {
            if finding.severity.rank() < merged[idx].severity.rank() {
                let existing: &mut Finding = &mut merged[idx];
                existing.severity = finding.severity;
                existing.title.clone_from(&finding.title);
                existing.description.clone_from(&finding.description);
            }
            merged[idx].evidence.extend(finding.evidence);
        } else {
            seen.insert(key, merged.len());
            merged.push(finding);
        }
    }

    *findings = merged;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DetectConfig;
    use crate::finding::{Finding, FindingCategory, Severity};
    use apex_core::types::Language;
    use async_trait::async_trait;
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // Mock detectors
    // -----------------------------------------------------------------------

    struct MockDetector {
        name: &'static str,
        subprocess: bool,
        findings: Vec<Finding>,
    }

    #[async_trait]
    impl Detector for MockDetector {
        fn name(&self) -> &str {
            self.name
        }
        async fn analyze(&self, _ctx: &AnalysisContext) -> apex_core::error::Result<Vec<Finding>> {
            Ok(self.findings.clone())
        }
        fn uses_cargo_subprocess(&self) -> bool {
            self.subprocess
        }
    }

    struct FailingDetector;

    #[async_trait]
    impl Detector for FailingDetector {
        fn name(&self) -> &str {
            "failing"
        }
        async fn analyze(&self, _: &AnalysisContext) -> apex_core::error::Result<Vec<Finding>> {
            Err(apex_core::error::ApexError::Detect("boom".into()))
        }
        fn uses_cargo_subprocess(&self) -> bool {
            false
        }
    }

    fn test_context() -> AnalysisContext {
        use std::sync::Arc;
        AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: apex_core::types::Language::Rust,
            oracle: Arc::new(apex_coverage::CoverageOracle::new()),
            file_paths: std::collections::HashMap::new(),
            known_bugs: vec![],
            source_cache: std::collections::HashMap::new(),
            fuzz_corpus: None,
            config: crate::config::DetectConfig::default(),
        }
    }

    fn make_finding(
        detector: &str,
        file: &str,
        line: u32,
        severity: Severity,
        category: FindingCategory,
    ) -> Finding {
        Finding {
            id: uuid::Uuid::new_v4(),
            detector: detector.into(),
            severity,
            category,
            file: PathBuf::from(file),
            line: Some(line),
            title: format!("{detector} finding"),
            description: "desc".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix it".into(),
            explanation: None,
            fix: None,
        }
    }

    #[test]
    fn deduplicate_merges_same_location_and_category() {
        let mut findings = vec![
            make_finding(
                "a",
                "src/main.rs",
                10,
                Severity::Medium,
                FindingCategory::PanicPath,
            ),
            make_finding(
                "b",
                "src/main.rs",
                10,
                Severity::High,
                FindingCategory::PanicPath,
            ),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn deduplicate_keeps_different_categories_separate() {
        let mut findings = vec![
            make_finding(
                "a",
                "src/main.rs",
                10,
                Severity::Medium,
                FindingCategory::PanicPath,
            ),
            make_finding(
                "b",
                "src/main.rs",
                10,
                Severity::High,
                FindingCategory::UnsafeCode,
            ),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn deduplicate_keeps_different_lines_separate() {
        let mut findings = vec![
            make_finding(
                "a",
                "src/main.rs",
                10,
                Severity::Medium,
                FindingCategory::PanicPath,
            ),
            make_finding(
                "a",
                "src/main.rs",
                20,
                Severity::Medium,
                FindingCategory::PanicPath,
            ),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn deduplicate_empty_is_noop() {
        let mut findings: Vec<Finding> = vec![];
        deduplicate(&mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn from_config_enables_all_by_default() {
        let cfg = DetectConfig::default();
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert_eq!(pipeline.detectors.len(), 6);
    }

    #[test]
    fn from_config_respects_enabled_list() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["panic".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "panic-pattern");
    }

    #[test]
    fn from_config_skips_unsafe_for_non_rust() {
        let cfg = DetectConfig::default();
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        assert!(pipeline
            .detectors
            .iter()
            .all(|d| d.name() != "unsafe-reachability"));
    }

    // -----------------------------------------------------------------------
    // run_all integration tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn run_all_with_pure_detector() {
        let finding = make_finding(
            "mock",
            "src/lib.rs",
            5,
            Severity::High,
            FindingCategory::PanicPath,
        );
        let detector = MockDetector {
            name: "mock-pure",
            subprocess: false,
            findings: vec![finding],
        };
        let pipeline = DetectorPipeline::new(vec![Box::new(detector)]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].detector, "mock");
        assert_eq!(report.detector_status.len(), 1);
        assert_eq!(report.detector_status[0], ("mock-pure".to_string(), true));
    }

    #[tokio::test]
    async fn run_all_with_subprocess_detector() {
        let finding = make_finding(
            "sub",
            "src/main.rs",
            10,
            Severity::Medium,
            FindingCategory::DependencyVuln,
        );
        let detector = MockDetector {
            name: "mock-sub",
            subprocess: true,
            findings: vec![finding],
        };
        let pipeline = DetectorPipeline::new(vec![Box::new(detector)]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].detector, "sub");
        assert_eq!(report.detector_status.len(), 1);
        assert_eq!(report.detector_status[0], ("mock-sub".to_string(), true));
    }

    #[tokio::test]
    async fn run_all_mixed_detectors() {
        let pure_finding = make_finding(
            "pure",
            "src/a.rs",
            1,
            Severity::Low,
            FindingCategory::PanicPath,
        );
        let sub_finding = make_finding(
            "sub",
            "src/b.rs",
            2,
            Severity::High,
            FindingCategory::UnsafeCode,
        );

        let pipeline = DetectorPipeline::new(vec![
            Box::new(MockDetector {
                name: "pure-det",
                subprocess: false,
                findings: vec![pure_finding],
            }),
            Box::new(MockDetector {
                name: "sub-det",
                subprocess: true,
                findings: vec![sub_finding],
            }),
        ]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.detector_status.len(), 2);

        // Both detectors must be marked successful
        let names: Vec<&str> = report
            .detector_status
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert!(names.contains(&"pure-det"));
        assert!(names.contains(&"sub-det"));
        assert!(report.detector_status.iter().all(|(_, ok)| *ok));
    }

    #[tokio::test]
    async fn run_all_failing_detector_marks_status_false() {
        let pipeline = DetectorPipeline::new(vec![Box::new(FailingDetector)]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        // Pipeline must not crash; no findings collected
        assert!(report.findings.is_empty());
        assert_eq!(report.detector_status.len(), 1);
        assert_eq!(report.detector_status[0], ("failing".to_string(), false));
    }

    #[tokio::test]
    async fn run_all_empty_pipeline() {
        let pipeline = DetectorPipeline::new(vec![]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        assert!(report.findings.is_empty());
        assert!(report.detector_status.is_empty());
    }

    #[tokio::test]
    async fn run_all_with_custom_timeout() {
        let finding = make_finding(
            "mock",
            "src/lib.rs",
            5,
            Severity::Low,
            FindingCategory::PanicPath,
        );
        let detector = MockDetector {
            name: "mock-timeout",
            subprocess: false,
            findings: vec![finding],
        };
        let pipeline = DetectorPipeline::new(vec![Box::new(detector)]);
        let mut ctx = test_context();
        ctx.config.per_detector_timeout_secs = Some(10);
        let report = pipeline.run_all(&ctx).await;
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.detector_status[0], ("mock-timeout".to_string(), true));
    }

    #[tokio::test]
    async fn run_all_multiple_subprocess_detectors_run_sequentially() {
        let f1 = make_finding("sub-a", "src/a.rs", 1, Severity::Medium, FindingCategory::DependencyVuln);
        let f2 = make_finding("sub-b", "src/b.rs", 2, Severity::Low, FindingCategory::DependencyVuln);
        let pipeline = DetectorPipeline::new(vec![
            Box::new(MockDetector {
                name: "sub-a",
                subprocess: true,
                findings: vec![f1],
            }),
            Box::new(MockDetector {
                name: "sub-b",
                subprocess: true,
                findings: vec![f2],
            }),
        ]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;
        assert_eq!(report.findings.len(), 2);
        assert!(report.detector_status.iter().all(|(_, ok)| *ok));
    }

    #[test]
    fn deduplicate_higher_severity_wins() {
        let mut findings = vec![
            make_finding("a", "src/lib.rs", 10, Severity::Low, FindingCategory::PanicPath),
            make_finding("b", "src/lib.rs", 10, Severity::Critical, FindingCategory::PanicPath),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn deduplicate_keeps_different_files_separate() {
        let mut findings = vec![
            make_finding("a", "src/a.rs", 10, Severity::Medium, FindingCategory::PanicPath),
            make_finding("a", "src/b.rs", 10, Severity::Medium, FindingCategory::PanicPath),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn deduplicate_merges_evidence() {
        use crate::finding::Evidence;
        let mut f1 = make_finding("a", "src/lib.rs", 10, Severity::Medium, FindingCategory::PanicPath);
        f1.evidence = vec![Evidence::SanitizerReport {
            sanitizer: "asan".into(),
            stderr: "overflow".into(),
        }];
        let mut f2 = make_finding("b", "src/lib.rs", 10, Severity::Low, FindingCategory::PanicPath);
        f2.evidence = vec![Evidence::SanitizerReport {
            sanitizer: "ubsan".into(),
            stderr: "undefined".into(),
        }];
        let mut findings = vec![f1, f2];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence.len(), 2); // evidence merged from both
    }

    #[tokio::test]
    async fn run_all_deduplicates_and_sorts() {
        // Two detectors emit findings at the same location/category — should be deduped.
        // Also include a Critical finding to verify severity sort (Critical first).
        let dup_a = make_finding(
            "det-a",
            "src/lib.rs",
            42,
            Severity::Medium,
            FindingCategory::PanicPath,
        );
        let dup_b = make_finding(
            "det-b",
            "src/lib.rs",
            42,
            Severity::High,
            FindingCategory::PanicPath,
        );
        let critical = make_finding(
            "det-a",
            "src/lib.rs",
            99,
            Severity::Critical,
            FindingCategory::MemorySafety,
        );

        let pipeline = DetectorPipeline::new(vec![
            Box::new(MockDetector {
                name: "det-a",
                subprocess: false,
                findings: vec![dup_a, critical],
            }),
            Box::new(MockDetector {
                name: "det-b",
                subprocess: false,
                findings: vec![dup_b],
            }),
        ]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        // dup_a and dup_b share (file, line, category) — deduplicated to 1 entry
        // plus the unique critical finding → 2 total
        assert_eq!(report.findings.len(), 2);

        // After sort: Critical (rank 0) comes before High/Medium (rank 1/2)
        assert_eq!(report.findings[0].severity, Severity::Critical);
    }
}
