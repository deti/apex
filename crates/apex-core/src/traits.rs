use crate::error::Result;
use crate::types::{
    BranchId, ExecutionResult, ExplorationContext, InputSeed, InstrumentedTarget, Language,
    SnapshotId, SynthesizedTest, Target, TestCandidate,
};

/// A strategy that proposes inputs to drive coverage.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    async fn suggest_inputs(&self, ctx: &ExplorationContext) -> Result<Vec<InputSeed>>;
    async fn observe(&self, result: &ExecutionResult) -> Result<()>;
}

/// An execution environment that runs a seed and returns coverage data.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Sandbox: Send + Sync {
    async fn run(&self, input: &InputSeed) -> Result<ExecutionResult>;
    async fn snapshot(&self) -> Result<SnapshotId>;
    async fn restore(&self, id: SnapshotId) -> Result<()>;
    fn language(&self) -> Language;
}

/// Instruments a target to emit branch coverage data.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Instrumentor: Send + Sync {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget>;
    fn branch_ids(&self) -> &[BranchId];
}

/// Synthesizes concrete test files from `TestCandidate`s.
#[cfg_attr(test, mockall::automock)]
pub trait TestSynthesizer: Send + Sync {
    fn synthesize(&self, candidates: &[TestCandidate]) -> Result<Vec<SynthesizedTest>>;
    fn language(&self) -> Language;
}

/// Result of a preflight check on a target project.
///
/// Each language runner populates the fields relevant to its ecosystem.
/// Callers can inspect `missing_tools` to decide whether to proceed, and
/// `warnings` for non-fatal issues worth surfacing to the user.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PreflightInfo {
    /// Build system detected (e.g. "gradle", "cmake", "poetry").
    pub build_system: Option<String>,
    /// Test framework detected (e.g. "pytest", "jest", "rspec").
    pub test_framework: Option<String>,
    /// Package manager detected (e.g. "uv", "npm", "bundler").
    pub package_manager: Option<String>,
    /// Tools required but not found on PATH.
    pub missing_tools: Vec<String>,
    /// Tools found on PATH with their versions.
    pub available_tools: Vec<(String, String)>,
    /// Non-fatal warnings (e.g. "PEP 668 externally-managed Python").
    pub warnings: Vec<String>,
    /// Environment variables that should be set.
    pub env_vars: Vec<(String, String)>,
    /// Whether dependencies are already installed (e.g. node_modules exists).
    pub deps_installed: bool,
    /// Language-specific extra info as key-value pairs.
    pub extra: Vec<(String, String)>,
}

impl PreflightInfo {
    /// Returns true if any required tools are missing.
    pub fn has_missing_tools(&self) -> bool {
        !self.missing_tools.is_empty()
    }

    /// Serialize to a JSON string for passing to coverage agents.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Returns a human-readable summary of the preflight check.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        if let Some(ref bs) = self.build_system {
            lines.push(format!("build system: {bs}"));
        }
        if let Some(ref tf) = self.test_framework {
            lines.push(format!("test framework: {tf}"));
        }
        if let Some(ref pm) = self.package_manager {
            lines.push(format!("package manager: {pm}"));
        }
        if !self.missing_tools.is_empty() {
            lines.push(format!("missing tools: {}", self.missing_tools.join(", ")));
        }
        for w in &self.warnings {
            lines.push(format!("warning: {w}"));
        }
        lines.join("\n")
    }
}

/// Detects, installs, and runs the test suite for a given language.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait LanguageRunner: Send + Sync {
    fn language(&self) -> Language;
    fn detect(&self, target: &std::path::Path) -> bool;
    async fn install_deps(&self, target: &std::path::Path) -> Result<()>;
    async fn run_tests(
        &self,
        target: &std::path::Path,
        extra_args: &[String],
    ) -> Result<TestRunOutput>;

    /// Inspect the target project and report what tools, frameworks, and
    /// configuration are present (or missing) before attempting a full run.
    ///
    /// The default implementation returns an empty `PreflightInfo`.
    /// Language runners should override this with ecosystem-specific checks.
    fn preflight_check(&self, _target: &std::path::Path) -> Result<PreflightInfo> {
        Ok(PreflightInfo::default())
    }
}

#[derive(Debug, Clone)]
pub struct TestRunOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_info_to_json() {
        let info = PreflightInfo {
            build_system: Some("poetry".into()),
            test_framework: Some("pytest".into()),
            package_manager: Some("uv".into()),
            missing_tools: vec!["coverage".into()],
            available_tools: vec![("python3".into(), "3.12.1".into())],
            warnings: vec!["PEP 668 externally-managed Python".into()],
            env_vars: vec![("VIRTUAL_ENV".into(), "/tmp/venv".into())],
            deps_installed: false,
            extra: vec![("python_path".into(), "/usr/bin/python3".into())],
        };
        let json = info.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["build_system"], "poetry");
        assert_eq!(parsed["test_framework"], "pytest");
        assert_eq!(parsed["package_manager"], "uv");
        assert_eq!(parsed["missing_tools"][0], "coverage");
        assert_eq!(parsed["deps_installed"], false);
        assert_eq!(parsed["warnings"][0], "PEP 668 externally-managed Python");
    }

    #[test]
    fn preflight_info_default_to_json() {
        let info = PreflightInfo::default();
        let json = info.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["build_system"].is_null());
        assert_eq!(parsed["deps_installed"], false);
        assert!(parsed["missing_tools"].as_array().unwrap().is_empty());
    }
}
