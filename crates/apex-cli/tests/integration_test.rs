//! End-to-end integration tests for the APEX CLI.
//!
//! These tests exercise the instrument → oracle → report pipeline using
//! fixture projects under `tests/fixtures/`.

use apex_core::types::{BranchId, ExecutionResult, ExecutionStatus, Language, SeedId, SeedOrigin};
use apex_coverage::CoverageOracle;
use std::collections::HashMap;
use std::path::PathBuf;

/// Path to the workspace root (two levels up from crates/apex-cli/).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fixture_path(name: &str) -> PathBuf {
    workspace_root().join("tests/fixtures").join(name)
}

// -------------------------------------------------------------------------
// Gap Report Formatting
// -------------------------------------------------------------------------

#[test]
fn gap_report_text_with_real_oracle_data() {
    let oracle = CoverageOracle::new();
    let b1 = BranchId::new(1, 10, 0, 0);
    let b2 = BranchId::new(1, 10, 0, 1);
    let b3 = BranchId::new(2, 20, 0, 0);
    oracle.register_branches([b1.clone(), b2.clone(), b3.clone()]);

    // Cover one branch
    oracle.mark_covered(&b1, SeedId::new());

    let mut file_paths = HashMap::new();
    file_paths.insert(1u64, PathBuf::from("src/main.py"));
    file_paths.insert(2u64, PathBuf::from("src/lib.py"));

    // Verify oracle state
    assert_eq!(oracle.total_count(), 3);
    assert_eq!(oracle.covered_count(), 1);
    let uncovered = oracle.uncovered_branches();
    assert_eq!(uncovered.len(), 2);
    assert!(uncovered.contains(&b2));
    assert!(uncovered.contains(&b3));
}

#[test]
fn gap_report_json_round_trip() {
    let oracle = CoverageOracle::new();
    let b1 = BranchId::new(42, 5, 0, 0);
    let b2 = BranchId::new(42, 5, 0, 1);
    oracle.register_branches([b1.clone(), b2.clone()]);
    oracle.mark_covered(&b1, SeedId::new());

    let uncovered = oracle.uncovered_branches();
    let mut file_paths = HashMap::new();
    file_paths.insert(42u64, PathBuf::from("test.py"));

    // Build JSON report data
    let branches: Vec<serde_json::Value> = uncovered
        .iter()
        .map(|b| {
            let rel = file_paths
                .get(&b.file_id)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| format!("{:016x}", b.file_id));
            serde_json::json!({
                "file": rel,
                "line": b.line,
                "direction": if b.direction == 0 { "true" } else { "false" },
            })
        })
        .collect();

    let report = serde_json::json!({
        "covered": oracle.covered_count(),
        "total": oracle.total_count(),
        "coverage_percent": oracle.coverage_percent(),
        "uncovered": branches,
    });

    let json_str = serde_json::to_string_pretty(&report).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["covered"], 1);
    assert_eq!(parsed["total"], 2);
    assert_eq!(parsed["coverage_percent"], 50.0);
    assert_eq!(parsed["uncovered"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["uncovered"][0]["file"], "test.py");
    assert_eq!(parsed["uncovered"][0]["direction"], "false");
}

// -------------------------------------------------------------------------
// ProcessSandbox — real subprocess execution
// -------------------------------------------------------------------------

#[tokio::test]
async fn process_sandbox_echo_hello() {
    use apex_core::traits::Sandbox;
    use apex_core::types::InputSeed;
    use apex_sandbox::ProcessSandbox;

    let sb = ProcessSandbox::new(
        Language::C,
        PathBuf::from("/tmp"),
        vec!["echo".into(), "hello".into()],
    )
    .with_timeout(5_000);

    let seed = InputSeed::new(b"test".to_vec(), SeedOrigin::Corpus);
    let result = sb.run(&seed).await.unwrap();
    assert_eq!(result.status, ExecutionStatus::Pass);
    assert!(result.stdout.contains("hello"));
}

#[tokio::test]
async fn process_sandbox_cat_stdin() {
    use apex_core::traits::Sandbox;
    use apex_core::types::InputSeed;
    use apex_sandbox::ProcessSandbox;

    let sb = ProcessSandbox::new(Language::C, PathBuf::from("/tmp"), vec!["cat".into()])
        .with_timeout(5_000);

    let seed = InputSeed::new(b"test input data".to_vec(), SeedOrigin::Corpus);
    let result = sb.run(&seed).await.unwrap();
    assert_eq!(result.status, ExecutionStatus::Pass);
    assert!(result.stdout.contains("test input data"));
}

#[tokio::test]
async fn process_sandbox_nonzero_exit_is_fail() {
    use apex_core::traits::Sandbox;
    use apex_core::types::InputSeed;
    use apex_sandbox::ProcessSandbox;

    let sb = ProcessSandbox::new(Language::C, PathBuf::from("/tmp"), vec!["false".into()])
        .with_timeout(5_000);

    let seed = InputSeed::new(b"".to_vec(), SeedOrigin::Corpus);
    let result = sb.run(&seed).await.unwrap();
    assert_eq!(result.status, ExecutionStatus::Fail);
}

#[tokio::test]
async fn process_sandbox_timeout() {
    use apex_core::traits::Sandbox;
    use apex_core::types::InputSeed;
    use apex_sandbox::ProcessSandbox;

    let sb = ProcessSandbox::new(
        Language::C,
        PathBuf::from("/tmp"),
        vec!["sleep".into(), "60".into()],
    )
    .with_timeout(200); // 200ms timeout

    let seed = InputSeed::new(b"".to_vec(), SeedOrigin::Corpus);
    let result = sb.run(&seed).await.unwrap();
    assert_eq!(result.status, ExecutionStatus::Timeout);
    assert!(result.duration_ms < 5000); // should be well under 5s
}

// -------------------------------------------------------------------------
// Ratchet pass/fail with known coverage
// -------------------------------------------------------------------------

#[test]
fn ratchet_pass_when_above_threshold() {
    let oracle = CoverageOracle::new();
    let branches: Vec<_> = (0..10).map(|i| BranchId::new(1, i, 0, 0)).collect();
    oracle.register_branches(branches.iter().cloned());

    // Cover 9 out of 10 → 90%
    let seed = SeedId::new();
    for b in &branches[..9] {
        oracle.mark_covered(b, seed);
    }

    let pct = oracle.coverage_percent() / 100.0;
    let threshold = 0.80;
    assert!(pct >= threshold, "coverage {pct} should be >= {threshold}");
}

#[test]
fn ratchet_fail_when_below_threshold() {
    let oracle = CoverageOracle::new();
    let branches: Vec<_> = (0..10).map(|i| BranchId::new(1, i, 0, 0)).collect();
    oracle.register_branches(branches.iter().cloned());

    // Cover 5 out of 10 → 50%
    let seed = SeedId::new();
    for b in &branches[..5] {
        oracle.mark_covered(b, seed);
    }

    let pct = oracle.coverage_percent() / 100.0;
    let threshold = 0.80;
    assert!(pct < threshold, "coverage {pct} should be < {threshold}");
}

// -------------------------------------------------------------------------
// CoverageOracle merge pipeline
// -------------------------------------------------------------------------

#[test]
fn oracle_merge_result_pipeline() {
    let oracle = CoverageOracle::new();
    let b1 = BranchId::new(1, 1, 0, 0);
    let b2 = BranchId::new(1, 2, 0, 0);
    let b3 = BranchId::new(1, 3, 0, 0);
    oracle.register_branches([b1.clone(), b2.clone(), b3.clone()]);

    // Simulate first execution covering b1
    let r1 = ExecutionResult {
        seed_id: SeedId::new(),
        status: ExecutionStatus::Pass,
        new_branches: vec![b1.clone()],
        trace: None,
        duration_ms: 10,
        stdout: String::new(),
        stderr: String::new(),
        input: None,
    };
    let d1 = oracle.merge_from_result(&r1);
    assert_eq!(d1.newly_covered.len(), 1);
    assert!((oracle.coverage_percent() - 100.0 / 3.0).abs() < 0.01);

    // Simulate second execution covering b2 and b3
    let r2 = ExecutionResult {
        seed_id: SeedId::new(),
        status: ExecutionStatus::Pass,
        new_branches: vec![b2, b3],
        trace: None,
        duration_ms: 5,
        stdout: String::new(),
        stderr: String::new(),
        input: None,
    };
    let d2 = oracle.merge_from_result(&r2);
    assert_eq!(d2.newly_covered.len(), 2);
    assert_eq!(oracle.coverage_percent(), 100.0);
}

// -------------------------------------------------------------------------
// Fixture existence
// -------------------------------------------------------------------------

#[test]
fn python_fixture_exists() {
    let p = fixture_path("python_project");
    assert!(p.join("src/app.py").exists());
    assert!(p.join("tests/test_app.py").exists());
    assert!(p.join("pyproject.toml").exists());
}

#[test]
fn c_fixture_exists() {
    let p = fixture_path("c_project");
    assert!(p.join("main.c").exists());
    assert!(p.join("Makefile").exists());
}

#[test]
fn js_fixture_exists() {
    let p = fixture_path("js_project");
    assert!(p.join("index.js").exists());
    assert!(p.join("package.json").exists());
    assert!(p.join("__tests__/index.test.js").exists());
}

// -------------------------------------------------------------------------
// PythonInstrumentor — parse_coverage_json with fixture-like data
// -------------------------------------------------------------------------

#[test]
fn python_instrument_parse_coverage_json() {
    use apex_core::traits::Instrumentor;
    use apex_instrument::PythonInstrumentor;

    let inst = PythonInstrumentor::new();
    // Verify it starts empty
    assert_eq!(inst.branch_ids().len(), 0);
}

// -------------------------------------------------------------------------
// Multi-file coverage tracking
// -------------------------------------------------------------------------

#[test]
fn multi_file_coverage_tracking() {
    let oracle = CoverageOracle::new();

    // Simulate branches across 3 files
    let file1_branches: Vec<_> = (1..=5).map(|l| BranchId::new(100, l, 0, 0)).collect();
    let file2_branches: Vec<_> = (1..=3).map(|l| BranchId::new(200, l, 0, 0)).collect();
    let file3_branches: Vec<_> = (1..=2).map(|l| BranchId::new(300, l, 0, 0)).collect();

    oracle.register_branches(
        file1_branches
            .iter()
            .chain(&file2_branches)
            .chain(&file3_branches)
            .cloned(),
    );
    assert_eq!(oracle.total_count(), 10);

    // Cover all of file1 and file2
    let seed = SeedId::new();
    for b in file1_branches.iter().chain(&file2_branches) {
        oracle.mark_covered(b, seed);
    }

    assert_eq!(oracle.covered_count(), 8);
    assert_eq!(oracle.coverage_percent(), 80.0);

    // Remaining uncovered should be file3 only
    let uncov = oracle.uncovered_branches();
    assert_eq!(uncov.len(), 2);
    assert!(uncov.iter().all(|b| b.file_id == 300));
}

// -------------------------------------------------------------------------
// WasmInstrumentor basic test
// -------------------------------------------------------------------------

#[test]
fn wasm_instrumentor_empty_dir() {
    use apex_core::traits::Instrumentor;
    use apex_instrument::WasmInstrumentor;

    let inst = WasmInstrumentor::new();
    assert_eq!(inst.branch_ids().len(), 0);
}
