//! Agent dispatch for the agentic coverage pipeline.
//!
//! Dispatches a `claude` subprocess to read the target project, install
//! dependencies, run tests with coverage instrumentation, and return a
//! structured [`AgentCoverageResult`] via stdout markers.

use apex_core::error::{ApexError, Result};
use apex_core::traits::PreflightInfo;
use apex_core::types::{AgentCoverageResult, Language};
use std::path::Path;
use tracing::{info, warn};

/// Dispatch a language crew agent to set up environment and run coverage.
pub async fn run_coverage_agent(
    lang: Language,
    target: &Path,
    preflight: &PreflightInfo,
    coverage_target: f64,
) -> Result<AgentCoverageResult> {
    let prompt = build_prompt(lang, target, preflight, coverage_target);

    // Try claude subprocess
    match dispatch_claude(&prompt, target).await {
        Ok(result) => Ok(result),
        Err(e) => {
            warn!("Agent dispatch failed: {e}. Agent coverage requires 'claude' CLI on PATH.");
            Err(e)
        }
    }
}

fn build_prompt(
    lang: Language,
    target: &Path,
    preflight: &PreflightInfo,
    coverage_target: f64,
) -> String {
    format!(
        "You are a coverage instrumentation agent. Set up the environment and run \
         coverage for this project.\n\n\
         Target: {}\n\
         Language: {:?}\n\
         Coverage target: {:.0}%\n\n\
         Preflight info:\n{}\n\n\
         Steps:\n\
         1. Read the project's README.md and build files\n\
         2. Install dependencies (create venvs if needed, install gems, etc.)\n\
         3. Run the test suite with coverage instrumentation\n\
         4. Save coverage output to .apex/coverage/ directory\n\
         5. Output the result as a JSON block between \
         APEX_COVERAGE_RESULT_BEGIN and APEX_COVERAGE_RESULT_END markers\n\n\
         If something fails, read the error and try an alternative approach.\n\
         Supported coverage formats: lcov, cobertura, jacoco, istanbul, v8, \
         llvm-cov-json, go-cover, simplecov, coverlet",
        target.display(),
        lang,
        coverage_target * 100.0,
        preflight.to_json(),
    )
}

async fn dispatch_claude(prompt: &str, working_dir: &Path) -> Result<AgentCoverageResult> {
    let claude_path = which_claude()?;

    info!(claude = %claude_path, "dispatching coverage agent");

    let output = tokio::process::Command::new(&claude_path)
        .args(["--print", "--dangerously-skip-permissions", "-p", prompt])
        .current_dir(working_dir)
        .output()
        .await
        .map_err(|e| ApexError::AgentDispatch(format!("spawn claude: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    parse_agent_output(&stdout)
}

fn which_claude() -> Result<String> {
    for candidate in &["claude", "/usr/local/bin/claude", "/opt/homebrew/bin/claude"] {
        let ok = std::process::Command::new("which")
            .arg(candidate)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Ok(candidate.to_string());
        }
    }
    Err(ApexError::AgentDispatch(
        "claude CLI not found on PATH".into(),
    ))
}

fn parse_agent_output(stdout: &str) -> Result<AgentCoverageResult> {
    let begin_marker = "APEX_COVERAGE_RESULT_BEGIN";
    let end_marker = "APEX_COVERAGE_RESULT_END";

    let begin = stdout.find(begin_marker).ok_or_else(|| {
        ApexError::AgentDispatch("no APEX_COVERAGE_RESULT_BEGIN marker in output".into())
    })?;
    let json_start = begin + begin_marker.len();
    let end = stdout[json_start..].find(end_marker).ok_or_else(|| {
        ApexError::AgentDispatch("no APEX_COVERAGE_RESULT_END marker in output".into())
    })?;

    let json_str = stdout[json_start..json_start + end].trim();

    serde_json::from_str(json_str)
        .map_err(|e| ApexError::AgentDispatch(format!("parse agent JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agent_output_valid() {
        let stdout = r#"Some preamble text
APEX_COVERAGE_RESULT_BEGIN
{
  "success": true,
  "coverage_dir": "/tmp/proj/.apex/coverage",
  "coverage_format": "lcov",
  "total_branches": 100,
  "covered_branches": 75,
  "coverage_pct": 75.0,
  "test_output_path": null,
  "test_count": 42,
  "test_pass": 40,
  "test_fail": 2,
  "test_skip": 0,
  "errors_encountered": ["pip failed", "retried with venv"],
  "tools_used": ["python3.12", "coverage.py 7.4"],
  "duration_secs": 30
}
APEX_COVERAGE_RESULT_END
Some trailing text"#;

        let result = parse_agent_output(stdout).unwrap();
        assert!(result.success);
        assert_eq!(result.coverage_format.as_deref(), Some("lcov"));
        assert_eq!(result.total_branches, Some(100));
        assert_eq!(result.covered_branches, Some(75));
        assert_eq!(result.errors_encountered.len(), 2);
        assert_eq!(result.tools_used.len(), 2);
    }

    #[test]
    fn parse_agent_output_missing_begin_marker() {
        let stdout = "no markers here";
        let err = parse_agent_output(stdout).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("APEX_COVERAGE_RESULT_BEGIN"), "{msg}");
    }

    #[test]
    fn parse_agent_output_missing_end_marker() {
        let stdout = "APEX_COVERAGE_RESULT_BEGIN\n{\"success\": true}";
        let err = parse_agent_output(stdout).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("APEX_COVERAGE_RESULT_END"), "{msg}");
    }

    #[test]
    fn build_prompt_non_empty() {
        let preflight = PreflightInfo {
            build_system: Some("poetry".into()),
            test_framework: Some("pytest".into()),
            ..Default::default()
        };
        let prompt = build_prompt(
            Language::Python,
            Path::new("/tmp/project"),
            &preflight,
            0.80,
        );
        assert!(!prompt.is_empty());
        assert!(prompt.contains("Python"), "prompt should mention language");
        assert!(prompt.contains("80%"), "prompt should mention target: {prompt}");
        assert!(prompt.contains("/tmp/project"), "prompt should mention path");
        assert!(prompt.contains("poetry"), "prompt should contain preflight info");
    }
}
