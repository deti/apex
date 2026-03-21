//! v0.4.0 integration tests — exercises the full pipeline for new output formats,
//! badge generation, changed-files filtering, CI report comparison, and LCOV export.

use apex_cli::{run_cli, Cli};
use apex_core::config::ApexConfig;
use clap::Parser;
use std::path::PathBuf;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_cfg() -> ApexConfig {
    ApexConfig::default()
}

/// Absolute path to a named fixture directory under `tests/fixtures/`.
fn fixture_path(name: &str) -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(format!("{manifest}/../../tests/fixtures/{name}"))
}

/// Write a minimal valid BranchIndex JSON to .apex/index.json in the given dir.
fn write_fixture_index(dir: &std::path::Path, covered: usize, total: usize) {
    let apex_dir = dir.join(".apex");
    std::fs::create_dir_all(&apex_dir).unwrap();
    std::fs::write(
        apex_dir.join("index.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "traces": [],
            "profiles": {},
            "file_paths": {},
            "total_branches": total,
            "covered_branches": covered,
            "created_at": "2026-01-01T00:00:00Z",
            "language": "Python",
            "target_root": dir.to_string_lossy(),
            "source_hash": "deadbeef"
        }))
        .unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// 1. SARIF output test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn audit_sarif_output_has_runs_and_results() {
    let target = fixture_path("tiny-python");
    assert!(target.exists(), "fixture missing: {}", target.display());

    let tmp = TempDir::new().unwrap();
    let output_path = tmp.path().join("report.sarif.json");

    let cli = Cli::parse_from([
        "apex",
        "audit",
        "--target",
        target.to_str().unwrap(),
        "--lang",
        "python",
        "--output-format",
        "sarif",
        "--output",
        output_path.to_str().unwrap(),
    ]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok(), "audit --output-format sarif failed: {:?}", result);

    let sarif_text = std::fs::read_to_string(&output_path).expect("read SARIF output file");
    let sarif: serde_json::Value =
        serde_json::from_str(&sarif_text).expect("SARIF output is valid JSON");

    // SARIF 2.1.0 structure: $.runs[0].results must exist
    assert!(
        sarif.get("runs").is_some(),
        "SARIF output missing 'runs' field"
    );
    let runs = sarif["runs"].as_array().expect("runs is an array");
    assert!(!runs.is_empty(), "runs array should not be empty");
    assert!(
        runs[0].get("results").is_some(),
        "SARIF output missing 'runs[0].results' field"
    );
    assert!(
        runs[0].get("tool").is_some(),
        "SARIF output missing 'runs[0].tool' field"
    );

    // Verify version field
    assert_eq!(
        sarif.get("version").and_then(|v| v.as_str()),
        Some("2.1.0"),
        "SARIF version should be 2.1.0"
    );
}

/// Test SARIF via the `findings_to_sarif` function directly with synthetic findings.
#[test]
fn sarif_function_produces_valid_structure() {
    use apex_detect::sarif::findings_to_sarif;
    use apex_detect::{Finding, FindingCategory, Severity};
    use uuid::Uuid;

    let finding = Finding {
        id: Uuid::new_v4(),
        detector: "test-detector".into(),
        severity: Severity::High,
        category: FindingCategory::Injection,
        file: PathBuf::from("src/main.py"),
        line: Some(42),
        title: "SQL Injection".into(),
        description: "User input reaches SQL query".into(),
        evidence: vec![],
        covered: false,
        suggestion: "Use parameterized queries".into(),
        explanation: None,
        fix: None,
        cwe_ids: vec![89],
        noisy: false,
        base_severity: None,
        coverage_confidence: None,
    };

    let report = findings_to_sarif(&[finding], "0.4.0");
    let json = serde_json::to_value(&report).unwrap();

    // Verify SARIF structure
    assert_eq!(json["version"], "2.1.0");
    let results = &json["runs"][0]["results"];
    assert_eq!(results.as_array().unwrap().len(), 1);
    assert!(results[0]["message"]["text"]
        .as_str()
        .unwrap()
        .contains("User input reaches SQL query"));
}

// ---------------------------------------------------------------------------
// 2. Markdown output test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn audit_markdown_output_has_table_headers_and_severity() {
    let target = fixture_path("tiny-python");
    assert!(target.exists());

    let tmp = TempDir::new().unwrap();
    let output_path = tmp.path().join("report.md");

    let cli = Cli::parse_from([
        "apex",
        "audit",
        "--target",
        target.to_str().unwrap(),
        "--lang",
        "python",
        "--output-format",
        "markdown",
        "--output",
        output_path.to_str().unwrap(),
    ]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(
        result.is_ok(),
        "audit --output-format markdown failed: {:?}",
        result
    );

    let md = std::fs::read_to_string(&output_path).expect("read markdown output");

    // Must contain markdown table headers
    assert!(
        md.contains("| Severity | Count |"),
        "Markdown output missing severity summary table"
    );
    // Must contain the heading
    assert!(
        md.contains("## APEX Security Audit"),
        "Markdown output missing heading"
    );
}

// ---------------------------------------------------------------------------
// 3. Badge test
// ---------------------------------------------------------------------------

#[test]
fn badge_svg_is_valid_xml_with_correct_color() {
    // Test the generate_badge_svg function directly
    let svg = apex_cli::generate_badge_svg(85.0);

    // Must be valid XML — starts with <svg and ends with </svg>
    assert!(
        svg.trim().starts_with("<svg"),
        "Badge SVG must start with <svg"
    );
    assert!(
        svg.trim().ends_with("</svg>"),
        "Badge SVG must end with </svg>"
    );
    // 85% should be green (#a3c51c)
    assert!(
        svg.contains("#a3c51c"),
        "85% coverage should produce green badge"
    );
    assert!(svg.contains("85.0%"), "Badge should display 85.0%");
    assert!(svg.contains("coverage"), "Badge should contain 'coverage' label");
}

#[test]
fn badge_color_tiers() {
    // brightgreen for >= 90%
    let svg90 = apex_cli::generate_badge_svg(95.0);
    assert!(svg90.contains("#4c1"), ">=90% should be brightgreen");

    // yellow for 60-74%
    let svg65 = apex_cli::generate_badge_svg(65.0);
    assert!(svg65.contains("#dfb317"), "65% should be yellow");

    // red for < 40%
    let svg20 = apex_cli::generate_badge_svg(20.0);
    assert!(svg20.contains("#e05d44"), "<40% should be red");
}

#[tokio::test]
async fn badge_subcommand_writes_svg_file() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path(), 85, 100);

    let output_path = tmp.path().join("badge.svg");
    let cli = Cli::parse_from([
        "apex",
        "badge",
        "--target",
        tmp.path().to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok(), "apex badge failed: {:?}", result);

    let svg = std::fs::read_to_string(&output_path).expect("read badge SVG");
    assert!(svg.contains("<svg"), "Output must be SVG");
    assert!(svg.contains("85.0%"), "Badge should show 85.0%");
}

// ---------------------------------------------------------------------------
// 4. Changed-files test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_prioritize_filters_by_changed_files() {
    let tmp = TempDir::new().unwrap();
    // Write an index with traces referencing specific files
    let apex_dir = tmp.path().join(".apex");
    std::fs::create_dir_all(&apex_dir).unwrap();
    std::fs::write(
        apex_dir.join("index.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "traces": [
                {
                    "test_name": "test_a",
                    "duration_ms": 100,
                    "status": "Pass",
                    "branches": [
                        {"file_id": 1, "line": 10, "col": 0, "direction": 0, "discriminator": 0}
                    ]
                },
                {
                    "test_name": "test_b",
                    "duration_ms": 200,
                    "status": "Pass",
                    "branches": [
                        {"file_id": 2, "line": 20, "col": 0, "direction": 0, "discriminator": 0}
                    ]
                }
            ],
            "profiles": {},
            "file_paths": {"1": "src/changed.py", "2": "src/other.py"},
            "total_branches": 2,
            "covered_branches": 2,
            "created_at": "2026-01-01T00:00:00Z",
            "language": "Python",
            "target_root": tmp.path().to_string_lossy(),
            "source_hash": "abc123"
        }))
        .unwrap(),
    )
    .unwrap();

    // Only src/changed.py is in changed-files, so test_a should be prioritized
    let cli = Cli::parse_from([
        "apex",
        "test-prioritize",
        "--target",
        tmp.path().to_str().unwrap(),
        "--changed-files",
        "src/changed.py",
    ]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(
        result.is_ok(),
        "test-prioritize with --changed-files failed: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// 5. CI report test
// ---------------------------------------------------------------------------

#[test]
fn ci_report_classifies_new_and_resolved_findings() {
    use apex_cli::ci_report::{compare_findings, format_markdown, format_json, AuditFinding};

    let base = vec![
        AuditFinding {
            severity: "Medium".into(),
            file: "src/old.py".into(),
            line: Some(10),
            title: "SQL injection".into(),
            description: "User input in query".into(),
            detector: "sql_injection".into(),
            suggestion: String::new(),
        },
        AuditFinding {
            severity: "Low".into(),
            file: "src/resolved.py".into(),
            line: Some(5),
            title: "Missing timeout".into(),
            description: "No timeout set".into(),
            detector: "missing_timeout".into(),
            suggestion: String::new(),
        },
    ];

    let head = vec![
        AuditFinding {
            severity: "Medium".into(),
            file: "src/old.py".into(),
            line: Some(10),
            title: "SQL injection".into(),
            description: "User input in query".into(),
            detector: "sql_injection".into(),
            suggestion: String::new(),
        },
        AuditFinding {
            severity: "High".into(),
            file: "src/new.py".into(),
            line: Some(42),
            title: "XSS vulnerability".into(),
            description: "Reflected XSS".into(),
            detector: "xss".into(),
            suggestion: String::new(),
        },
    ];

    let report = compare_findings(&base, &head);

    // Verify classification
    assert_eq!(report.new_count, 1, "Should have 1 new finding");
    assert_eq!(report.resolved_count, 1, "Should have 1 resolved finding");
    assert_eq!(report.unchanged_count, 1, "Should have 1 unchanged finding");
    assert!(report.has_new_high_critical, "New HIGH finding should flag");

    // Verify new finding details
    assert_eq!(report.new_findings[0].file, "src/new.py");
    assert_eq!(report.new_findings[0].detector, "xss");

    // Verify resolved finding details
    assert_eq!(report.resolved_findings[0].file, "src/resolved.py");

    // Markdown output
    let md = format_markdown(&report);
    assert!(md.contains("## APEX CI Report"), "Missing CI report heading");
    assert!(md.contains("### New Findings"), "Missing new findings section");
    assert!(md.contains("### Resolved Findings"), "Missing resolved findings section");
    assert!(md.contains("new.py"), "Missing new finding file");
    assert!(md.contains("resolved.py"), "Missing resolved finding file");

    // JSON output
    let json_str = format_json(&report).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(json["new_count"], 1);
    assert_eq!(json["resolved_count"], 1);
    assert_eq!(json["has_new_high_critical"], true);
}

#[test]
fn ci_report_reads_json_files() {
    let tmp = TempDir::new().unwrap();
    let base_path = tmp.path().join("base.json");
    let head_path = tmp.path().join("head.json");

    let base_json = serde_json::json!([
        {"severity": "Low", "file": "a.py", "line": 1, "title": "t", "description": "d", "detector": "d1", "suggestion": ""}
    ]);
    let head_json = serde_json::json!([
        {"severity": "Low", "file": "a.py", "line": 1, "title": "t", "description": "d", "detector": "d1", "suggestion": ""},
        {"severity": "Critical", "file": "b.py", "line": 2, "title": "t2", "description": "d2", "detector": "d2", "suggestion": ""}
    ]);

    std::fs::write(&base_path, serde_json::to_string(&base_json).unwrap()).unwrap();
    std::fs::write(&head_path, serde_json::to_string(&head_json).unwrap()).unwrap();

    let has_new = apex_cli::ci_report::run_ci_report(&base_path, &head_path, true).unwrap();
    assert!(has_new, "New CRITICAL finding should trigger exit code");
}

// ---------------------------------------------------------------------------
// 6. LCOV export test
// ---------------------------------------------------------------------------

#[test]
fn lcov_export_contains_required_markers() {
    use apex_core::types::BranchId;
    use apex_instrument::lcov_export::export_lcov;
    use std::collections::HashMap;

    let all_branches = vec![
        BranchId::new(1, 10, 0, 0),
        BranchId::new(1, 10, 0, 1),
        BranchId::new(1, 20, 0, 0),
        BranchId::new(2, 5, 0, 0),
    ];

    let executed_branches = vec![
        BranchId::new(1, 10, 0, 0),
        BranchId::new(2, 5, 0, 0),
    ];

    let mut file_paths = HashMap::new();
    file_paths.insert(1u64, PathBuf::from("src/main.py"));
    file_paths.insert(2u64, PathBuf::from("src/lib.py"));

    let lcov = export_lcov(&all_branches, &executed_branches, &file_paths);

    // LCOV format markers
    assert!(lcov.contains("SF:"), "LCOV must contain SF: (source file) marker");
    assert!(lcov.contains("DA:"), "LCOV must contain DA: (data) marker");
    assert!(
        lcov.contains("end_of_record"),
        "LCOV must contain end_of_record marker"
    );
    // Should reference both files
    assert!(lcov.contains("src/main.py"), "LCOV must reference main.py");
    assert!(lcov.contains("src/lib.py"), "LCOV must reference lib.py");
}

// ---------------------------------------------------------------------------
// OutputFormat::Sarif variant parses from CLI
// ---------------------------------------------------------------------------

#[test]
fn sarif_output_format_variant_parses() {
    use clap::ValueEnum;
    let sarif = apex_cli::OutputFormat::from_str("sarif", true).unwrap();
    assert!(matches!(sarif, apex_cli::OutputFormat::Sarif));
}
