use std::collections::HashMap;
use std::time::Duration;

use apex_core::error::ApexError;
use apex_core::types::Language;
use tracing::warn;

use crate::config::{DetectConfig, DetectMode};
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
        if cfg.enabled.contains(&"path-normalize".to_string()) {
            detectors.push(Box::new(PathNormalizationDetector));
        }

        if cfg.detect_mode == DetectMode::Fast {
            detectors.retain(|d| !d.uses_cargo_subprocess());
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
    use crate::config::{DetectConfig, DetectMode};
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
            runner: Arc::new(apex_core::command::RealCommandRunner),
            cpg: None,
            threat_model: Default::default(),
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
            cwe_ids: vec![],
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
        assert_eq!(pipeline.detectors.len(), 7);
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
        assert_eq!(
            report.detector_status[0],
            ("mock-timeout".to_string(), true)
        );
    }

    #[tokio::test]
    async fn run_all_multiple_subprocess_detectors_run_sequentially() {
        let f1 = make_finding(
            "sub-a",
            "src/a.rs",
            1,
            Severity::Medium,
            FindingCategory::DependencyVuln,
        );
        let f2 = make_finding(
            "sub-b",
            "src/b.rs",
            2,
            Severity::Low,
            FindingCategory::DependencyVuln,
        );
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
            make_finding(
                "a",
                "src/lib.rs",
                10,
                Severity::Low,
                FindingCategory::PanicPath,
            ),
            make_finding(
                "b",
                "src/lib.rs",
                10,
                Severity::Critical,
                FindingCategory::PanicPath,
            ),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn deduplicate_keeps_different_files_separate() {
        let mut findings = vec![
            make_finding(
                "a",
                "src/a.rs",
                10,
                Severity::Medium,
                FindingCategory::PanicPath,
            ),
            make_finding(
                "a",
                "src/b.rs",
                10,
                Severity::Medium,
                FindingCategory::PanicPath,
            ),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn deduplicate_merges_evidence() {
        use crate::finding::Evidence;
        let mut f1 = make_finding(
            "a",
            "src/lib.rs",
            10,
            Severity::Medium,
            FindingCategory::PanicPath,
        );
        f1.evidence = vec![Evidence::SanitizerReport {
            sanitizer: "asan".into(),
            stderr: "overflow".into(),
        }];
        let mut f2 = make_finding(
            "b",
            "src/lib.rs",
            10,
            Severity::Low,
            FindingCategory::PanicPath,
        );
        f2.evidence = vec![Evidence::SanitizerReport {
            sanitizer: "ubsan".into(),
            stderr: "undefined".into(),
        }];
        let mut findings = vec![f1, f2];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence.len(), 2); // evidence merged from both
    }

    // -----------------------------------------------------------------------
    // Additional from_config tests
    // -----------------------------------------------------------------------

    #[test]
    fn from_config_empty_enabled_list() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec![];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert!(pipeline.detectors.is_empty());
    }

    #[test]
    fn from_config_only_security() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["security".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "security-pattern");
    }

    #[test]
    fn from_config_only_secrets() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["secrets".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "hardcoded-secret");
    }

    #[test]
    fn from_config_only_path_normalize() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["path-normalize".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "path-normalize");
    }

    #[test]
    fn from_config_only_deps() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["deps".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "dependency-audit");
    }

    #[test]
    fn from_config_only_static() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["static".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "static-analysis");
    }

    #[test]
    fn from_config_unsafe_included_for_rust() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["unsafe".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "unsafe-reachability");
    }

    #[test]
    fn from_config_unsafe_excluded_for_python() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["unsafe".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        assert!(pipeline.detectors.is_empty());
    }

    #[test]
    fn from_config_panic_and_security_together() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["panic".into(), "security".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        assert_eq!(pipeline.detectors.len(), 2);
        let names: Vec<&str> = pipeline.detectors.iter().map(|d| d.name()).collect();
        assert!(names.contains(&"panic-pattern"));
        assert!(names.contains(&"security-pattern"));
    }

    #[test]
    fn from_config_unknown_detector_ignored() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["nonexistent".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert!(pipeline.detectors.is_empty());
    }

    #[test]
    fn from_config_all_non_rust_language() {
        // Python should get all except unsafe
        let cfg = DetectConfig::default();
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        assert_eq!(pipeline.detectors.len(), 6);
        assert!(pipeline
            .detectors
            .iter()
            .all(|d| d.name() != "unsafe-reachability"));
    }

    #[test]
    fn new_creates_pipeline_with_given_detectors() {
        let pipeline = DetectorPipeline::new(vec![
            Box::new(MockDetector {
                name: "a",
                subprocess: false,
                findings: vec![],
            }),
            Box::new(MockDetector {
                name: "b",
                subprocess: true,
                findings: vec![],
            }),
        ]);
        assert_eq!(pipeline.detectors.len(), 2);
        assert_eq!(pipeline.detectors[0].name(), "a");
        assert_eq!(pipeline.detectors[1].name(), "b");
    }

    // -----------------------------------------------------------------------
    // Additional deduplicate tests
    // -----------------------------------------------------------------------

    #[test]
    fn deduplicate_lower_severity_does_not_override() {
        // First finding is High, second is Low — High should remain.
        let mut findings = vec![
            make_finding(
                "a",
                "src/lib.rs",
                10,
                Severity::High,
                FindingCategory::PanicPath,
            ),
            make_finding(
                "b",
                "src/lib.rs",
                10,
                Severity::Low,
                FindingCategory::PanicPath,
            ),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn deduplicate_same_severity_keeps_first() {
        let mut findings = vec![
            make_finding(
                "first",
                "src/lib.rs",
                10,
                Severity::Medium,
                FindingCategory::PanicPath,
            ),
            make_finding(
                "second",
                "src/lib.rs",
                10,
                Severity::Medium,
                FindingCategory::PanicPath,
            ),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        // Same severity → first title preserved (no override since rank is not less)
        assert_eq!(findings[0].title, "first finding");
    }

    #[test]
    fn deduplicate_three_findings_same_location() {
        use crate::finding::Evidence;
        let mut f1 = make_finding("a", "x.rs", 1, Severity::Low, FindingCategory::PanicPath);
        f1.evidence = vec![Evidence::SanitizerReport {
            sanitizer: "s1".into(),
            stderr: "e1".into(),
        }];
        let mut f2 = make_finding("b", "x.rs", 1, Severity::High, FindingCategory::PanicPath);
        f2.evidence = vec![Evidence::SanitizerReport {
            sanitizer: "s2".into(),
            stderr: "e2".into(),
        }];
        let mut f3 = make_finding(
            "c",
            "x.rs",
            1,
            Severity::Critical,
            FindingCategory::PanicPath,
        );
        f3.evidence = vec![Evidence::SanitizerReport {
            sanitizer: "s3".into(),
            stderr: "e3".into(),
        }];

        let mut findings = vec![f1, f2, f3];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].evidence.len(), 3);
    }

    #[test]
    fn deduplicate_none_line_findings() {
        // Findings with line=None should be grouped by (file, None, category)
        let mut f1 = make_finding(
            "a",
            "lib.rs",
            0,
            Severity::Medium,
            FindingCategory::DependencyVuln,
        );
        f1.line = None;
        let mut f2 = make_finding(
            "b",
            "lib.rs",
            0,
            Severity::High,
            FindingCategory::DependencyVuln,
        );
        f2.line = None;

        let mut findings = vec![f1, f2];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn deduplicate_single_finding_unchanged() {
        let mut findings = vec![make_finding(
            "a",
            "src/main.rs",
            10,
            Severity::Medium,
            FindingCategory::PanicPath,
        )];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    // -----------------------------------------------------------------------
    // Additional run_all tests
    // -----------------------------------------------------------------------

    struct FailingSubprocessDetector;

    #[async_trait]
    impl Detector for FailingSubprocessDetector {
        fn name(&self) -> &str {
            "failing-sub"
        }
        async fn analyze(&self, _: &AnalysisContext) -> apex_core::error::Result<Vec<Finding>> {
            Err(apex_core::error::ApexError::Detect(
                "subprocess boom".into(),
            ))
        }
        fn uses_cargo_subprocess(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn run_all_failing_subprocess_detector_marks_false() {
        let pipeline = DetectorPipeline::new(vec![Box::new(FailingSubprocessDetector)]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        assert!(report.findings.is_empty());
        assert_eq!(report.detector_status.len(), 1);
        assert_eq!(
            report.detector_status[0],
            ("failing-sub".to_string(), false)
        );
    }

    #[tokio::test]
    async fn run_all_mixed_success_and_failure() {
        let finding = make_finding(
            "good",
            "src/lib.rs",
            1,
            Severity::High,
            FindingCategory::PanicPath,
        );
        let pipeline = DetectorPipeline::new(vec![
            Box::new(MockDetector {
                name: "good-det",
                subprocess: false,
                findings: vec![finding],
            }),
            Box::new(FailingDetector),
        ]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        // One finding from good detector, none from failing
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.detector_status.len(), 2);

        let good_status = report.detector_status.iter().find(|(n, _)| n == "good-det");
        let fail_status = report.detector_status.iter().find(|(n, _)| n == "failing");
        assert_eq!(good_status, Some(&("good-det".to_string(), true)));
        assert_eq!(fail_status, Some(&("failing".to_string(), false)));
    }

    #[tokio::test]
    async fn run_all_multiple_pure_detectors() {
        let f1 = make_finding("d1", "a.rs", 1, Severity::Low, FindingCategory::PanicPath);
        let f2 = make_finding(
            "d2",
            "b.rs",
            2,
            Severity::Medium,
            FindingCategory::UnsafeCode,
        );
        let f3 = make_finding(
            "d3",
            "c.rs",
            3,
            Severity::High,
            FindingCategory::SecuritySmell,
        );

        let pipeline = DetectorPipeline::new(vec![
            Box::new(MockDetector {
                name: "p1",
                subprocess: false,
                findings: vec![f1],
            }),
            Box::new(MockDetector {
                name: "p2",
                subprocess: false,
                findings: vec![f2],
            }),
            Box::new(MockDetector {
                name: "p3",
                subprocess: false,
                findings: vec![f3],
            }),
        ]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        assert_eq!(report.findings.len(), 3);
        assert_eq!(report.detector_status.len(), 3);
        assert!(report.detector_status.iter().all(|(_, ok)| *ok));

        // Sorted by severity rank: High(1), Medium(2), Low(3)
        assert_eq!(report.findings[0].severity, Severity::High);
        assert_eq!(report.findings[1].severity, Severity::Medium);
        assert_eq!(report.findings[2].severity, Severity::Low);
    }

    #[tokio::test]
    async fn run_all_sorts_by_covered_within_same_severity() {
        let mut f_uncovered =
            make_finding("d1", "a.rs", 1, Severity::High, FindingCategory::PanicPath);
        f_uncovered.covered = false;
        let mut f_covered =
            make_finding("d2", "b.rs", 2, Severity::High, FindingCategory::UnsafeCode);
        f_covered.covered = true;

        let pipeline = DetectorPipeline::new(vec![
            Box::new(MockDetector {
                name: "p1",
                subprocess: false,
                findings: vec![f_covered],
            }),
            Box::new(MockDetector {
                name: "p2",
                subprocess: false,
                findings: vec![f_uncovered],
            }),
        ]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        assert_eq!(report.findings.len(), 2);
        // Both High severity; covered=false (0) sorts before covered=true (1)
        assert!(!report.findings[0].covered);
        assert!(report.findings[1].covered);
    }

    #[tokio::test]
    async fn run_all_no_timeout_uses_default_300s() {
        // Config with no per_detector_timeout_secs → 300s default
        let finding = make_finding("m", "x.rs", 1, Severity::Info, FindingCategory::LogicBug);
        let pipeline = DetectorPipeline::new(vec![Box::new(MockDetector {
            name: "det",
            subprocess: false,
            findings: vec![finding],
        })]);
        let mut ctx = test_context();
        ctx.config.per_detector_timeout_secs = None;
        let report = pipeline.run_all(&ctx).await;
        assert_eq!(report.findings.len(), 1);
    }

    #[tokio::test]
    async fn run_all_detector_returning_empty_findings() {
        let pipeline = DetectorPipeline::new(vec![Box::new(MockDetector {
            name: "empty-det",
            subprocess: false,
            findings: vec![],
        })]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        assert!(report.findings.is_empty());
        assert_eq!(report.detector_status.len(), 1);
        assert_eq!(report.detector_status[0], ("empty-det".to_string(), true));
    }

    #[tokio::test]
    async fn run_all_subprocess_failure_with_successful_pure() {
        let finding = make_finding(
            "pure",
            "x.rs",
            1,
            Severity::High,
            FindingCategory::PanicPath,
        );
        let pipeline = DetectorPipeline::new(vec![
            Box::new(MockDetector {
                name: "pure-ok",
                subprocess: false,
                findings: vec![finding],
            }),
            Box::new(FailingSubprocessDetector),
        ]);
        let ctx = test_context();
        let report = pipeline.run_all(&ctx).await;

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.detector_status.len(), 2);

        let pure_ok = report.detector_status.iter().find(|(n, _)| n == "pure-ok");
        let sub_fail = report
            .detector_status
            .iter()
            .find(|(n, _)| n == "failing-sub");
        assert_eq!(pure_ok.unwrap().1, true);
        assert_eq!(sub_fail.unwrap().1, false);
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

    #[test]
    fn fast_mode_excludes_subprocess_detectors() {
        let mut cfg = DetectConfig::default();
        cfg.detect_mode = DetectMode::Fast;
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        for d in &pipeline.detectors {
            assert!(
                !d.uses_cargo_subprocess(),
                "fast mode should exclude {}",
                d.name()
            );
        }
    }

    #[test]
    fn full_mode_includes_subprocess_detectors() {
        let cfg = DetectConfig::default();
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        let has_subprocess = pipeline.detectors.iter().any(|d| d.uses_cargo_subprocess());
        assert!(
            has_subprocess,
            "full mode should include subprocess detectors"
        );
    }
}
