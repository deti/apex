//! Toolchain detection and provisioning.
//!
//! Detects required toolchain versions from project files (`.tool-versions`,
//! `.node-version`, `go.mod`, CI configs, etc.) and optionally installs them
//! via `mise` if available.

use std::path::Path;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A toolchain requirement detected from project files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedToolchain {
    /// Canonical tool name (e.g. "go", "python", "node", "java").
    pub tool: String,
    /// Version constraint (e.g. "1.22", "3.12", "20.11.0").
    pub version: String,
    /// Where the requirement was discovered (e.g. "go.mod", ".node-version",
    /// "ci: .github/workflows/ci.yml").
    pub source: String,
}

/// Environment configuration detected in the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentConfig {
    Devcontainer,
    Devbox,
    Mise,
}

impl std::fmt::Display for EnvironmentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Devcontainer => write!(f, "devcontainer"),
            Self::Devbox => write!(f, "devbox"),
            Self::Mise => write!(f, "mise"),
        }
    }
}

// ---------------------------------------------------------------------------
// Version-file detection (Wave 1 — file-based)
// ---------------------------------------------------------------------------

/// Detect toolchain versions from well-known project files.
pub fn detect_toolchain_versions(target: &Path) -> Vec<DetectedToolchain> {
    let mut detected = Vec::new();

    // .tool-versions (mise / asdf universal format)
    let tv_path = target.join(".tool-versions");
    if let Ok(content) = std::fs::read_to_string(&tv_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            if let (Some(tool), Some(version)) = (parts.next(), parts.next()) {
                detected.push(DetectedToolchain {
                    tool: tool.to_string(),
                    version: version.to_string(),
                    source: ".tool-versions".into(),
                });
            }
        }
    }

    // .python-version
    if let Some(dt) = read_single_version_file(target, ".python-version", "python") {
        detected.push(dt);
    }

    // .node-version
    if let Some(dt) = read_single_version_file(target, ".node-version", "node") {
        detected.push(dt);
    }

    // .ruby-version
    if let Some(dt) = read_single_version_file(target, ".ruby-version", "ruby") {
        detected.push(dt);
    }

    // .go-version
    if let Some(dt) = read_single_version_file(target, ".go-version", "go") {
        detected.push(dt);
    }

    // .java-version
    if let Some(dt) = read_single_version_file(target, ".java-version", "java") {
        detected.push(dt);
    }

    // go.mod — extract `go 1.22` directive
    let go_mod = target.join("go.mod");
    if let Ok(content) = std::fs::read_to_string(&go_mod) {
        if let Some(ver) = extract_go_mod_version(&content) {
            detected.push(DetectedToolchain {
                tool: "go".into(),
                version: ver,
                source: "go.mod".into(),
            });
        }
    }

    // .nvmrc — Node version
    if let Some(dt) = read_single_version_file(target, ".nvmrc", "node") {
        detected.push(dt);
    }

    // CI config files
    detected.extend(parse_github_actions(target));

    detected
}

/// Read a single-line version file like `.python-version`.
fn read_single_version_file(target: &Path, filename: &str, tool: &str) -> Option<DetectedToolchain> {
    let path = target.join(filename);
    let content = std::fs::read_to_string(&path).ok()?;
    let version = content.lines().next()?.trim().to_string();
    if version.is_empty() {
        return None;
    }
    Some(DetectedToolchain {
        tool: tool.into(),
        version,
        source: filename.into(),
    })
}

/// Extract the Go version from `go.mod` content (e.g. `go 1.22`).
fn extract_go_mod_version(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("go ") {
            let version = rest.trim();
            if !version.is_empty() && version.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                return Some(version.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// CI config parser (Wave 2 — GitHub Actions)
// ---------------------------------------------------------------------------

/// Parse `.github/workflows/*.yml` for `actions/setup-*` steps.
pub fn parse_github_actions(target: &Path) -> Vec<DetectedToolchain> {
    let workflows_dir = target.join(".github").join("workflows");
    if !workflows_dir.is_dir() {
        return vec![];
    }

    let mut detected = Vec::new();
    let entries = match std::fs::read_dir(&workflows_dir) {
        Ok(entries) => entries,
        Err(_) => return vec![],
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_yaml = path
            .extension()
            .map(|e| e == "yml" || e == "yaml")
            .unwrap_or(false);
        if !is_yaml {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            detected.extend(extract_setup_actions(&content, &filename));
        }
    }
    detected
}

/// Extract `actions/setup-*` patterns from a GitHub Actions workflow file.
///
/// Looks for patterns like:
/// ```yaml
///   - uses: actions/setup-go@v4
///     with:
///       go-version: '1.22'
/// ```
///
/// Simple line-based parsing — no YAML crate needed.
fn extract_setup_actions(yaml: &str, source_file: &str) -> Vec<DetectedToolchain> {
    let mut detected = Vec::new();
    let lines: Vec<&str> = yaml.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Match: uses: actions/setup-go@v4
        let uses_prefix = trimmed
            .strip_prefix("- uses:")
            .or_else(|| trimmed.strip_prefix("uses:"))
            .map(|rest| rest.trim());

        let uses_value = match uses_prefix {
            Some(v) => v,
            None => continue,
        };

        // Extract tool name from actions/setup-<tool>@<ref>
        let tool_name = if let Some(rest) = uses_value.strip_prefix("actions/setup-") {
            rest.split('@').next().unwrap_or("").to_string()
        } else {
            continue;
        };

        if tool_name.is_empty() {
            continue;
        }

        // Look ahead for `with:` block and version key
        let version_key = format!("{}-version", tool_name);
        let mut version = None;
        let lookahead_end = lines.len().min(i + 10);

        for (offset, lookahead_line) in lines[i + 1..lookahead_end].iter().enumerate() {
            let next_trimmed = lookahead_line.trim();

            // Stop if we hit another step (but not the very next line)
            if next_trimmed.starts_with("- ") && offset > 0 {
                break;
            }

            // Look for <tool>-version: <value>
            if let Some(rest) = next_trimmed.strip_prefix(&format!("{version_key}:")) {
                let ver = rest.trim().trim_matches('\'').trim_matches('"').to_string();
                if !ver.is_empty() {
                    version = Some(ver);
                }
                break;
            }
        }

        let source = format!("ci: .github/workflows/{source_file}");
        detected.push(DetectedToolchain {
            tool: tool_name,
            version: version.unwrap_or_else(|| "latest".into()),
            source,
        });
    }

    detected
}

// ---------------------------------------------------------------------------
// Devcontainer / devbox detection (Wave 2)
// ---------------------------------------------------------------------------

/// Detect environment configuration files (devcontainer, devbox, mise).
pub fn detect_environment_config(target: &Path) -> Option<EnvironmentConfig> {
    if target.join(".devcontainer").join("devcontainer.json").exists() {
        return Some(EnvironmentConfig::Devcontainer);
    }
    if target.join("devbox.json").exists() {
        return Some(EnvironmentConfig::Devbox);
    }
    if target.join(".mise.toml").exists() || target.join("mise.toml").exists() {
        return Some(EnvironmentConfig::Mise);
    }
    None
}

// ---------------------------------------------------------------------------
// Mise backend (Wave 3 — provisioning)
// ---------------------------------------------------------------------------

/// Backend that uses `mise` to install toolchain versions.
pub struct MiseBackend;

impl MiseBackend {
    /// Check if `mise` is available on PATH.
    pub fn is_available() -> bool {
        std::process::Command::new("mise")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Attempt to install the given toolchains via `mise install`.
    ///
    /// Returns a list of (tool@version, success) pairs.
    pub fn ensure_installed(tools: &[DetectedToolchain]) -> Vec<(String, bool)> {
        let mut results = Vec::new();
        for tool in tools {
            let spec = format!("{}@{}", tool.tool, tool.version);
            let ok = std::process::Command::new("mise")
                .args(["install", &spec])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            results.push((spec, ok));
        }
        results
    }
}

// ---------------------------------------------------------------------------
// Doctor integration helpers
// ---------------------------------------------------------------------------

/// Format a toolchain detection result for `apex doctor` output.
pub fn format_toolchain_checks(target: &Path) -> Vec<ToolchainCheck> {
    let detected = detect_toolchain_versions(target);
    let mut checks = Vec::new();

    for dt in &detected {
        // Check if the tool is available on PATH
        let installed = check_tool_on_path(&dt.tool);
        let managed_by = if MiseBackend::is_available() {
            Some("mise")
        } else {
            None
        };

        checks.push(ToolchainCheck {
            tool: dt.tool.clone(),
            version: dt.version.clone(),
            source: dt.source.clone(),
            installed,
            managed_by: managed_by.map(String::from),
        });
    }

    checks
}

/// A single toolchain check result for doctor output.
#[derive(Debug, Clone)]
pub struct ToolchainCheck {
    pub tool: String,
    pub version: String,
    pub source: String,
    pub installed: bool,
    pub managed_by: Option<String>,
}

impl std::fmt::Display for ToolchainCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let icon = if self.installed { "\x1b[32m✓\x1b[0m" } else { "\x1b[31m✗\x1b[0m" };
        let managed = match &self.managed_by {
            Some(m) if self.installed => format!(", installed via {m}"),
            _ if !self.installed => ", not installed".to_string(),
            _ => ", system".to_string(),
        };
        write!(
            f,
            "  {icon} {:<12} {:<12} (from {}{managed})",
            self.tool, self.version, self.source
        )
    }
}

/// Check if a tool binary is on PATH.
fn check_tool_on_path(tool: &str) -> bool {
    // Map tool names to binary names
    let bin = match tool {
        "node" => "node",
        "python" => "python3",
        "go" => "go",
        "java" => "java",
        "ruby" => "ruby",
        "dotnet" => "dotnet",
        other => other,
    };
    std::process::Command::new(bin)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    // -- extract_go_mod_version ---

    #[test]
    fn test_extract_go_mod_version() {
        let content = "module example.com/foo\n\ngo 1.22\n\nrequire (\n)";
        assert_eq!(extract_go_mod_version(content), Some("1.22".into()));
    }

    #[test]
    fn test_extract_go_mod_version_with_patch() {
        let content = "module foo\ngo 1.22.3\n";
        assert_eq!(extract_go_mod_version(content), Some("1.22.3".into()));
    }

    #[test]
    fn test_extract_go_mod_version_missing() {
        let content = "module foo\nrequire bar v1.0.0\n";
        assert_eq!(extract_go_mod_version(content), None);
    }

    // -- extract_setup_actions ---

    #[test]
    fn test_extract_setup_go() {
        let yaml = r#"
jobs:
  build:
    steps:
      - uses: actions/setup-go@v4
        with:
          go-version: '1.22'
      - run: go build
"#;
        let result = extract_setup_actions(yaml, "ci.yml");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tool, "go");
        assert_eq!(result[0].version, "1.22");
        assert!(result[0].source.contains("ci.yml"));
    }

    #[test]
    fn test_extract_setup_multiple() {
        let yaml = r#"
steps:
  - uses: actions/setup-node@v4
    with:
      node-version: '20'
  - uses: actions/setup-python@v5
    with:
      python-version: '3.12'
"#;
        let result = extract_setup_actions(yaml, "test.yml");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].tool, "node");
        assert_eq!(result[0].version, "20");
        assert_eq!(result[1].tool, "python");
        assert_eq!(result[1].version, "3.12");
    }

    #[test]
    fn test_extract_setup_no_version() {
        let yaml = r#"
steps:
  - uses: actions/setup-java@v3
  - run: java -version
"#;
        let result = extract_setup_actions(yaml, "ci.yml");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tool, "java");
        assert_eq!(result[0].version, "latest");
    }

    #[test]
    fn test_extract_setup_quoted_version() {
        let yaml = r#"
steps:
  - uses: actions/setup-go@v4
    with:
      go-version: "1.21.5"
"#;
        let result = extract_setup_actions(yaml, "build.yaml");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].version, "1.21.5");
    }

    #[test]
    fn test_extract_no_setup_actions() {
        let yaml = r#"
steps:
  - uses: actions/checkout@v4
  - run: echo hello
"#;
        let result = extract_setup_actions(yaml, "ci.yml");
        assert!(result.is_empty());
    }

    // -- detect_environment_config ---

    #[test]
    fn test_detect_devcontainer() {
        let dir = make_temp_dir();
        let dc_dir = dir.path().join(".devcontainer");
        fs::create_dir_all(&dc_dir).unwrap();
        fs::write(dc_dir.join("devcontainer.json"), "{}").unwrap();

        assert_eq!(
            detect_environment_config(dir.path()),
            Some(EnvironmentConfig::Devcontainer)
        );
    }

    #[test]
    fn test_detect_devbox() {
        let dir = make_temp_dir();
        fs::write(dir.path().join("devbox.json"), "{}").unwrap();

        assert_eq!(
            detect_environment_config(dir.path()),
            Some(EnvironmentConfig::Devbox)
        );
    }

    #[test]
    fn test_detect_mise_toml() {
        let dir = make_temp_dir();
        fs::write(dir.path().join(".mise.toml"), "[tools]\n").unwrap();

        assert_eq!(
            detect_environment_config(dir.path()),
            Some(EnvironmentConfig::Mise)
        );
    }

    #[test]
    fn test_detect_mise_toml_no_dot() {
        let dir = make_temp_dir();
        fs::write(dir.path().join("mise.toml"), "[tools]\n").unwrap();

        assert_eq!(
            detect_environment_config(dir.path()),
            Some(EnvironmentConfig::Mise)
        );
    }

    #[test]
    fn test_detect_no_env_config() {
        let dir = make_temp_dir();
        assert_eq!(detect_environment_config(dir.path()), None);
    }

    // -- detect_toolchain_versions ---

    #[test]
    fn test_detect_tool_versions_file() {
        let dir = make_temp_dir();
        fs::write(
            dir.path().join(".tool-versions"),
            "python 3.12.1\nnode 20.11.0\n",
        )
        .unwrap();

        let detected = detect_toolchain_versions(dir.path());
        assert!(detected.iter().any(|d| d.tool == "python" && d.version == "3.12.1"));
        assert!(detected.iter().any(|d| d.tool == "node" && d.version == "20.11.0"));
    }

    #[test]
    fn test_detect_python_version_file() {
        let dir = make_temp_dir();
        fs::write(dir.path().join(".python-version"), "3.11.7\n").unwrap();

        let detected = detect_toolchain_versions(dir.path());
        assert!(detected.iter().any(|d| d.tool == "python" && d.version == "3.11.7"));
    }

    #[test]
    fn test_detect_go_mod() {
        let dir = make_temp_dir();
        fs::write(dir.path().join("go.mod"), "module foo\n\ngo 1.22\n").unwrap();

        let detected = detect_toolchain_versions(dir.path());
        assert!(detected.iter().any(|d| d.tool == "go" && d.version == "1.22"));
    }

    #[test]
    fn test_detect_github_actions() {
        let dir = make_temp_dir();
        let wf_dir = dir.path().join(".github").join("workflows");
        fs::create_dir_all(&wf_dir).unwrap();
        fs::write(
            wf_dir.join("ci.yml"),
            "steps:\n  - uses: actions/setup-node@v4\n    with:\n      node-version: '18'\n",
        )
        .unwrap();

        let detected = detect_toolchain_versions(dir.path());
        assert!(detected.iter().any(|d| d.tool == "node" && d.version == "18"));
    }

    #[test]
    fn test_detect_empty_project() {
        let dir = make_temp_dir();
        let detected = detect_toolchain_versions(dir.path());
        assert!(detected.is_empty());
    }

    // -- parse_github_actions with real directory ---

    #[test]
    fn test_parse_github_actions_no_dir() {
        let dir = make_temp_dir();
        let result = parse_github_actions(dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_github_actions_empty_dir() {
        let dir = make_temp_dir();
        let wf_dir = dir.path().join(".github").join("workflows");
        fs::create_dir_all(&wf_dir).unwrap();

        let result = parse_github_actions(dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_github_actions_non_yaml_ignored() {
        let dir = make_temp_dir();
        let wf_dir = dir.path().join(".github").join("workflows");
        fs::create_dir_all(&wf_dir).unwrap();
        fs::write(wf_dir.join("README.md"), "steps:\n  - uses: actions/setup-go@v4\n").unwrap();

        let result = parse_github_actions(dir.path());
        assert!(result.is_empty());
    }

    // -- ToolchainCheck display ---

    #[test]
    fn test_toolchain_check_display_installed() {
        let check = ToolchainCheck {
            tool: "go".into(),
            version: "1.22".into(),
            source: "go.mod".into(),
            installed: true,
            managed_by: Some("mise".into()),
        };
        let display = format!("{check}");
        assert!(display.contains("go"));
        assert!(display.contains("1.22"));
        assert!(display.contains("go.mod"));
        assert!(display.contains("mise"));
    }

    #[test]
    fn test_toolchain_check_display_not_installed() {
        let check = ToolchainCheck {
            tool: "python".into(),
            version: "3.12".into(),
            source: ".python-version".into(),
            installed: false,
            managed_by: None,
        };
        let display = format!("{check}");
        assert!(display.contains("python"));
        assert!(display.contains("not installed"));
    }

    // -- EnvironmentConfig display ---

    #[test]
    fn test_environment_config_display() {
        assert_eq!(EnvironmentConfig::Devcontainer.to_string(), "devcontainer");
        assert_eq!(EnvironmentConfig::Devbox.to_string(), "devbox");
        assert_eq!(EnvironmentConfig::Mise.to_string(), "mise");
    }

    // -- Tool versions file edge cases ---

    #[test]
    fn test_tool_versions_with_comments() {
        let dir = make_temp_dir();
        fs::write(
            dir.path().join(".tool-versions"),
            "# this is a comment\npython 3.12\n\n# another comment\nnode 20\n",
        )
        .unwrap();

        let detected = detect_toolchain_versions(dir.path());
        assert_eq!(
            detected
                .iter()
                .filter(|d| d.source == ".tool-versions")
                .count(),
            2
        );
    }

    #[test]
    fn test_nvmrc_detection() {
        let dir = make_temp_dir();
        fs::write(dir.path().join(".nvmrc"), "18.19.0\n").unwrap();

        let detected = detect_toolchain_versions(dir.path());
        assert!(detected.iter().any(|d| d.tool == "node" && d.version == "18.19.0"));
    }

    #[test]
    fn test_empty_version_file_ignored() {
        let dir = make_temp_dir();
        fs::write(dir.path().join(".python-version"), "\n").unwrap();

        let detected = detect_toolchain_versions(dir.path());
        assert!(detected.iter().all(|d| d.tool != "python"));
    }

    // -- extract_setup_actions edge cases ---

    #[test]
    fn test_setup_action_without_dash_prefix() {
        // Some workflows indent differently
        let yaml = r#"
    steps:
      - name: Setup Go
        uses: actions/setup-go@v4
        with:
          go-version: '1.21'
"#;
        let result = extract_setup_actions(yaml, "ci.yml");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tool, "go");
        assert_eq!(result[0].version, "1.21");
    }

    #[test]
    fn test_setup_action_dotnet() {
        let yaml = r#"
steps:
  - uses: actions/setup-dotnet@v3
    with:
      dotnet-version: '8.0'
"#;
        let result = extract_setup_actions(yaml, "ci.yml");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tool, "dotnet");
        assert_eq!(result[0].version, "8.0");
    }
}
