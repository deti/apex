use apex_cli::{Cli, run_cli};
use apex_core::config::ApexConfig;
use clap::Parser;
use tempfile::TempDir;

fn default_cfg() -> ApexConfig {
    ApexConfig::default()
}

/// Write a minimal valid BranchIndex JSON to .apex/index.json in the given dir.
fn write_fixture_index(dir: &std::path::Path) {
    let apex_dir = dir.join(".apex");
    std::fs::create_dir_all(&apex_dir).unwrap();
    std::fs::write(
        apex_dir.join("index.json"),
        r#"{
  "traces": [],
  "profiles": {},
  "file_paths": {},
  "total_branches": 100,
  "covered_branches": 85,
  "created_at": "2026-01-01T00:00:00Z",
  "language": "Rust",
  "target_root": "/tmp/fixture",
  "source_hash": "abc123"
}"#,
    )
    .unwrap();
}

// NOTE: doctor test omitted — doctor calls std::process::exit(1) when tools
// are missing, which kills the test process. Tested via assert_cmd instead.

#[tokio::test]
async fn run_cli_deploy_score_with_index() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path());
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "deploy-score", "--target", target]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_deploy_score_json() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path());
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from([
        "apex",
        "deploy-score",
        "--target",
        target,
        "--output-format",
        "json",
    ]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_test_optimize_empty_index() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path());
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "test-optimize", "--target", target]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_dead_code_with_index() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path());
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "dead-code", "--target", target]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_dead_code_json() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path());
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from([
        "apex",
        "dead-code",
        "--target",
        target,
        "--output-format",
        "json",
    ]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_complexity_with_index() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path());
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "complexity", "--target", target]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_hotpaths_with_index() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path());
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "hotpaths", "--target", target]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_contracts_with_index() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path());
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "contracts", "--target", target]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_risk_with_index() {
    let tmp = TempDir::new().unwrap();
    write_fixture_index(tmp.path());
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from([
        "apex",
        "risk",
        "--target",
        target,
        "--changed-files",
        "src/lib.rs",
    ]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_test_optimize_no_index_fails() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "test-optimize", "--target", target]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn run_cli_dead_code_no_index_fails() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "dead-code", "--target", target]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(result.is_err());
}
