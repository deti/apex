//! Integration harness for apex-cli pipeline.
//!
//! Tests exercise `run_cli` directly (in-process) for commands that do not
//! require external tools (pytest, clang, etc.).  Commands that invoke
//! the full instrumentation + execution pipeline are exercised via the
//! `assert_cmd` subprocess approach so that missing tools cause a graceful
//! error rather than an async panic.

use apex_cli::{run_cli, Cli, Commands, DeadCodeArgs, DeployScoreArgs, FeaturesArgs, LangArg};
use apex_core::config::ApexConfig;
use clap::Parser;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Absolute path to a named fixture directory under `tests/fixtures/`.
fn fixture_path(name: &str) -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    std::path::PathBuf::from(format!("{manifest}/../../tests/fixtures/{name}"))
}

/// Build a minimal `BranchIndex` JSON and write it to `<dir>/.apex/index.json`.
/// Returns the temp directory so it stays alive for the duration of the test.
fn write_minimal_index(dir: &std::path::Path) {
    let apex_dir = dir.join(".apex");
    std::fs::create_dir_all(&apex_dir).expect("create .apex dir");
    let index_json = serde_json::json!({
        "traces": [],
        "profiles": {},
        "file_paths": {},
        "total_branches": 4,
        "covered_branches": 2,
        "created_at": "2026-01-01T00:00:00Z",
        "language": "Python",
        "target_root": dir.to_string_lossy(),
        "source_hash": "deadbeef"
    });
    std::fs::write(
        apex_dir.join("index.json"),
        serde_json::to_string_pretty(&index_json).unwrap(),
    )
    .expect("write index.json");
}

fn default_cfg() -> ApexConfig {
    ApexConfig::default()
}

// ---------------------------------------------------------------------------
// `apex run` — subprocess (needs full pipeline; nonexistent path must fail)
// ---------------------------------------------------------------------------

#[test]
fn test_run_nonexistent_target_fails() {
    // Using assert_cmd so we don't need a running async runtime here and the
    // failure mode is a clean process exit, not an in-process panic.
    use assert_cmd::Command;
    Command::cargo_bin("apex")
        .unwrap()
        .args([
            "run",
            "--target",
            "/nonexistent/path/does/not/exist",
            "--lang",
            "python",
            "--strategy",
            "baseline",
            "--no-install",
        ])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// `apex run` — in-process, baseline strategy against real fixture
// Guard with #[ignore] because this requires pytest to be installed.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires pytest installed in PATH"]
async fn test_run_python_baseline_succeeds() {
    let target = fixture_path("tiny-python");
    assert!(target.exists(), "fixture missing: {}", target.display());
    let cfg = default_cfg();
    let cli = Cli::parse_from([
        "apex",
        "run",
        "--target",
        target.to_str().unwrap(),
        "--lang",
        "python",
        "--strategy",
        "baseline",
        "--no-install",
        "--output-format",
        "json",
    ]);
    let result = run_cli(cli, &cfg).await;
    assert!(result.is_ok(), "apex run failed: {:?}", result);
}

// ---------------------------------------------------------------------------
// `apex audit` — in-process, Python fixture
// Guard with #[ignore] because the detector pipeline may shell out.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires external detector tools or network access"]
async fn test_audit_python_succeeds() {
    let target = fixture_path("tiny-python");
    assert!(target.exists(), "fixture missing: {}", target.display());
    let cfg = default_cfg();
    let cli = Cli::parse_from([
        "apex",
        "audit",
        "--target",
        target.to_str().unwrap(),
        "--lang",
        "python",
        "--output-format",
        "json",
    ]);
    let result = run_cli(cli, &cfg).await;
    assert!(result.is_ok(), "apex audit failed: {:?}", result);
}

// ---------------------------------------------------------------------------
// `apex deploy-score` — requires `.apex/index.json`; use a temp dir
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_deploy_score_succeeds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_minimal_index(tmp.path());

    let cfg = default_cfg();
    // Construct the CLI struct directly instead of parse_from to avoid quoting issues.
    let cli = Cli {
        config: None,
        log_level: None,
        command: Commands::DeployScore(DeployScoreArgs {
            target: tmp.path().to_path_buf(),
            detector_findings: 0,
            critical_findings: 0,
            output_format: apex_cli::OutputFormat::Json,
        }),
    };
    let result = run_cli(cli, &cfg).await;
    assert!(result.is_ok(), "apex deploy-score failed: {:?}", result);
}

// ---------------------------------------------------------------------------
// `apex complexity` — requires `.apex/index.json`
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_complexity_succeeds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_minimal_index(tmp.path());

    let cfg = default_cfg();
    let cli = Cli {
        config: None,
        log_level: None,
        command: Commands::Complexity(apex_cli::ComplexityArgs {
            target: tmp.path().to_path_buf(),
            output_format: apex_cli::OutputFormat::Text,
        }),
    };
    let result = run_cli(cli, &cfg).await;
    assert!(result.is_ok(), "apex complexity failed: {:?}", result);
}

// ---------------------------------------------------------------------------
// `apex dead-code` — requires `.apex/index.json`
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dead_code_succeeds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_minimal_index(tmp.path());

    let cfg = default_cfg();
    let cli = Cli {
        config: None,
        log_level: None,
        command: Commands::DeadCode(DeadCodeArgs {
            target: tmp.path().to_path_buf(),
            output_format: apex_cli::OutputFormat::Text,
        }),
    };
    let result = run_cli(cli, &cfg).await;
    assert!(result.is_ok(), "apex dead-code failed: {:?}", result);
}

// ---------------------------------------------------------------------------
// `apex features` — pure in-process, no filesystem
// ---------------------------------------------------------------------------

#[test]
fn test_features_python() {
    let cfg = default_cfg();
    let cli = Cli {
        config: None,
        log_level: None,
        command: Commands::Features(FeaturesArgs {
            lang: Some(LangArg::Python),
            output_format: apex_cli::OutputFormat::Text,
        }),
    };
    // run_features is sync but run_cli is async; drive it with a minimal runtime.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(run_cli(cli, &cfg));
    assert!(result.is_ok(), "apex features failed: {:?}", result);
}

// ---------------------------------------------------------------------------
// Fixture existence
// ---------------------------------------------------------------------------

#[test]
fn tiny_python_fixture_exists() {
    let p = fixture_path("tiny-python");
    assert!(p.join("main.py").exists(), "main.py missing");
    assert!(p.join("test_main.py").exists(), "test_main.py missing");
    assert!(p.join("pytest.ini").exists(), "pytest.ini missing");
}

#[test]
fn tiny_js_fixture_exists() {
    let p = fixture_path("tiny-js");
    assert!(p.join("index.js").exists(), "index.js missing");
    assert!(p.join("index.test.js").exists(), "index.test.js missing");
    assert!(p.join("package.json").exists(), "package.json missing");
}

// ---------------------------------------------------------------------------
// JavaScript integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_audit_js_succeeds() {
    let target = fixture_path("tiny-js");
    let cli = Cli::parse_from([
        "apex",
        "audit",
        "--lang",
        "js",
        "--target",
        target.to_str().unwrap(),
    ]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok(), "apex audit --lang js failed: {:?}", result);
}

#[tokio::test]
async fn test_features_js() {
    let cli = Cli::parse_from(["apex", "features", "--lang", "js"]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(
        result.is_ok(),
        "apex features --lang js failed: {:?}",
        result
    );
}
