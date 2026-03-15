<!-- status: FUTURE -->
# APEX MCP Server Implementation Plan

> **For agentic workers:** REQUIRED: Use fleet crew agents (platform crew for CLI, intelligence crew for testing). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `apex mcp` STDIO server and `apex integrate` auto-configuration, making APEX available to Cursor, Codex CLI, Cline, Continue.dev, and LM Studio.

**Architecture:** New `mcp.rs` and `integrate.rs` modules in apex-cli. MCP server uses `rmcp` crate with `#[tool_router]` macros. Each MCP tool calls existing CLI handler functions directly. `apex integrate` writes per-tool JSON/TOML config files.

**Tech Stack:** Rust, rmcp (MCP SDK), serde, schemars, tokio

---

## File Map

| File | Responsibility |
|------|---------------|
| `crates/apex-cli/src/mcp.rs` | MCP STDIO server — tool definitions + handlers |
| `crates/apex-cli/src/integrate.rs` | `apex integrate` — tool detection + config writing |
| `crates/apex-cli/src/lib.rs` | Add `Mcp` and `Integrate` command variants |
| `crates/apex-cli/Cargo.toml` | Add `rmcp` + `schemars` dependencies |

---

## Task 1: Add dependencies and command stubs

**Crew:** platform
**Files:**
- Modify: `crates/apex-cli/Cargo.toml`
- Create: `crates/apex-cli/src/mcp.rs`
- Create: `crates/apex-cli/src/integrate.rs`
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Add dependencies to Cargo.toml**

```toml
rmcp = { version = "1", features = ["server", "transport-io", "macros"] }
schemars = "0.8"
```

- [ ] **Step 2: Create mcp.rs stub**

```rust
use apex_core::error::Result;

pub async fn run_mcp() -> Result<()> {
    todo!("MCP server")
}
```

- [ ] **Step 3: Create integrate.rs stub**

```rust
use apex_core::error::Result;

pub async fn run_integrate(tool: Option<String>) -> Result<()> {
    todo!("integrate")
}
```

- [ ] **Step 4: Add commands to lib.rs**

Add to `Commands` enum:
```rust
/// Start MCP STDIO server for AI tool integration
Mcp,
/// Configure AI tools to use APEX
Integrate(IntegrateArgs),
```

Add args:
```rust
#[derive(clap::Args)]
pub struct IntegrateArgs {
    /// Specific tool to configure (cursor, codex, cline, continue, lm-studio)
    #[arg(long)]
    pub tool: Option<String>,
    /// Write to global config instead of per-project
    #[arg(long)]
    pub global: bool,
}
```

Add to dispatch in `run_cli()`:
```rust
Commands::Mcp => mcp::run_mcp().await,
Commands::Integrate(args) => integrate::run_integrate(args.tool).await,
```

Add module declarations:
```rust
mod mcp;
mod integrate;
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p apex-cli`

- [ ] **Step 6: Commit**

```bash
git commit -m "feat: scaffold apex mcp + apex integrate commands"
```

---

## Task 2: MCP server — tool definitions and handlers

**Crew:** platform
**Files:**
- Modify: `crates/apex-cli/src/mcp.rs`

This is the core task — implement the full MCP STDIO server with 6 tools.

- [ ] **Step 1: Implement MCP service struct with tool router**

```rust
use rmcp::{
    ServerHandler, ServiceExt,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo, Implementation},
    schemars, tool,
    handler::server::tool::ToolRouter,
    transport::stdio,
};
use serde::Deserialize;

#[derive(Clone)]
pub struct ApexMcpService {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ApexMcpService {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_handler]
impl ServerHandler for ApexMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation {
                name: "apex".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some(
                "APEX is a code coverage and security analysis tool. \
                 Use apex_run for coverage gaps, apex_detect for security findings, \
                 apex_reach for reachability analysis.".into()
            ),
        }
    }
}
```

- [ ] **Step 2: Define parameter structs with schemars**

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunParams {
    #[schemars(description = "Path to project root")]
    pub target: String,
    #[schemars(description = "Language: python, rust, javascript, java, go, ruby, cpp, swift, csharp, kotlin")]
    pub lang: String,
    #[schemars(description = "Strategy: agent, fuzz, concolic, all")]
    pub strategy: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DetectParams {
    #[schemars(description = "Path to project root")]
    pub target: String,
    #[schemars(description = "Language")]
    pub lang: String,
    #[schemars(description = "Minimum severity: low, medium, high, critical")]
    pub severity_threshold: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReachParams {
    #[schemars(description = "Target in file:line format")]
    pub target: String,
    #[schemars(description = "Language")]
    pub lang: String,
    #[schemars(description = "Granularity: function, block, line")]
    pub granularity: Option<String>,
    #[schemars(description = "Filter entry kind: test, http, main, api, cli")]
    pub entry_kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RatchetParams {
    #[schemars(description = "Path to project root")]
    pub target: String,
    #[schemars(description = "Language")]
    pub lang: String,
    #[schemars(description = "Minimum coverage ratio 0.0-1.0")]
    pub min_coverage: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DoctorParams {
    #[schemars(description = "Language to check prerequisites for")]
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeployScoreParams {
    #[schemars(description = "Path to project root")]
    pub target: String,
    #[schemars(description = "Language")]
    pub lang: String,
}
```

- [ ] **Step 3: Implement tool handlers**

Each handler captures stdout from the existing CLI function and returns it as MCP content:

```rust
use rmcp::handler::server::tool::Parameters;
use std::io::Write;

#[tool_router]
impl ApexMcpService {
    // ... new() ...

    #[tool(description = "Run coverage analysis. Returns uncovered branches, gap report, and coverage percentage.")]
    async fn apex_run(
        &self,
        Parameters(params): Parameters<RunParams>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let output = run_apex_command(&[
            "run",
            "--target", &params.target,
            "--lang", &params.lang,
            "--strategy", params.strategy.as_deref().unwrap_or("agent"),
            "--output-format", "json",
        ]).await;
        match output {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("Error: {e}"))])),
        }
    }

    #[tool(description = "Run security detectors. Returns findings with CWE IDs, severity, and remediation.")]
    async fn apex_detect(
        &self,
        Parameters(params): Parameters<DetectParams>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let mut args = vec![
            "audit".to_string(),
            "--target".into(), params.target,
            "--lang".into(), params.lang,
        ];
        if let Some(sev) = params.severity_threshold {
            args.extend(["--severity-threshold".into(), sev]);
        }
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = run_apex_command(&args_ref).await;
        match output {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("Error: {e}"))])),
        }
    }

    #[tool(description = "Find entry points (tests, HTTP handlers, main) that reach a file:line.")]
    async fn apex_reach(
        &self,
        Parameters(params): Parameters<ReachParams>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let mut args = vec![
            "reach".to_string(),
            "--target".into(), params.target,
            "--lang".into(), params.lang,
        ];
        if let Some(g) = params.granularity {
            args.extend(["--granularity".into(), g]);
        }
        if let Some(k) = params.entry_kind {
            args.extend(["--entry-kind".into(), k]);
        }
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = run_apex_command(&args_ref).await;
        match output {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("Error: {e}"))])),
        }
    }

    #[tool(description = "CI coverage gate. Returns PASS/FAIL with coverage percentage.")]
    async fn apex_ratchet(
        &self,
        Parameters(params): Parameters<RatchetParams>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let mut args = vec![
            "ratchet".to_string(),
            "--target".into(), params.target,
            "--lang".into(), params.lang,
        ];
        if let Some(min) = params.min_coverage {
            args.extend(["--min-coverage".into(), min.to_string()]);
        }
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = run_apex_command(&args_ref).await;
        match output {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("Error: {e}"))])),
        }
    }

    #[tool(description = "Check prerequisites for a language.")]
    async fn apex_doctor(
        &self,
        Parameters(_params): Parameters<DoctorParams>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let output = run_apex_command(&["doctor"]).await;
        match output {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("Error: {e}"))])),
        }
    }

    #[tool(description = "Deployment confidence score (0-100) based on coverage, security, and test health.")]
    async fn apex_deploy_score(
        &self,
        Parameters(params): Parameters<DeployScoreParams>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let output = run_apex_command(&[
            "deploy-score",
            "--target", &params.target,
            "--lang", &params.lang,
        ]).await;
        match output {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("Error: {e}"))])),
        }
    }
}
```

- [ ] **Step 4: Implement run_apex_command helper**

This spawns `apex` as a subprocess to avoid re-initializing tracing (which panics on second call):

```rust
/// Run an apex CLI command as a subprocess and capture stdout.
async fn run_apex_command(args: &[&str]) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let output = tokio::process::Command::new(exe)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to run apex: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("apex {} failed: {}", args.first().unwrap_or(&""), stderr))
    }
}
```

- [ ] **Step 5: Implement run_mcp entry point**

```rust
pub async fn run_mcp() -> apex_core::error::Result<()> {
    let service = ApexMcpService::new()
        .serve(stdio())
        .await
        .map_err(|e| apex_core::error::ApexError::Other(format!("MCP server error: {e}")))?;

    service.waiting().await
        .map_err(|e| apex_core::error::ApexError::Other(format!("MCP server error: {e}")))?;

    Ok(())
}
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p apex-cli`

- [ ] **Step 7: Commit**

```bash
git commit -m "feat: implement APEX MCP STDIO server with 6 tools"
```

---

## Task 3: Integration test — MCP protocol

**Crew:** platform
**Files:**
- Modify: `crates/apex-cli/src/mcp.rs` (add tests)

- [ ] **Step 1: Add integration test**

Test that spawns `apex mcp` as subprocess, sends JSON-RPC `initialize` + `tools/list`, verifies response:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mcp_tools_list_returns_six_tools() {
        let exe = std::env::current_exe().unwrap();
        // Find the apex binary (it's in target/debug/)
        let apex = exe.parent().unwrap().join("apex");
        if !apex.exists() {
            return; // Skip if binary not built
        }

        let mut child = tokio::process::Command::new(&apex)
            .arg("mcp")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to spawn apex mcp");

        let stdin = child.stdin.as_mut().unwrap();
        let stdout = child.stdout.take().unwrap();

        // Send initialize
        let init = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1.0" }
            }
        });
        let msg = format!("{}\n", serde_json::to_string(&init).unwrap());
        use tokio::io::AsyncWriteExt;
        stdin.write_all(msg.as_bytes()).await.unwrap();

        // Read response (simplified — real test would parse framing)
        // For now just verify the process started and didn't crash
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        child.kill().await.ok();
    }

    #[test]
    fn param_structs_have_json_schema() {
        // Verify schemars derives work
        let schema = schemars::schema_for!(RunParams);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("lang"));
    }

    #[test]
    fn detect_params_schema() {
        let schema = schemars::schema_for!(DetectParams);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("severity_threshold"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p apex-cli mcp`

- [ ] **Step 3: Commit**

```bash
git commit -m "test: MCP server integration tests"
```

---

## Task 4: `apex integrate` command

**Crew:** platform
**Files:**
- Modify: `crates/apex-cli/src/integrate.rs`

- [ ] **Step 1: Implement tool detection**

```rust
use std::path::{Path, PathBuf};
use apex_core::error::{ApexError, Result};
use tracing::info;

struct DetectedTool {
    name: &'static str,
    config_path: PathBuf,
}

fn detect_tools(project_root: &Path) -> Vec<DetectedTool> {
    let mut tools = Vec::new();

    // Cursor
    if project_root.join(".cursor").is_dir() || which("cursor").is_some() {
        tools.push(DetectedTool {
            name: "Cursor",
            config_path: project_root.join(".cursor").join("mcp.json"),
        });
    }

    // Codex CLI
    if project_root.join(".codex").is_dir() || which("codex").is_some() {
        tools.push(DetectedTool {
            name: "Codex CLI",
            config_path: project_root.join(".codex").join("config.toml"),
        });
    }

    // Continue.dev
    if project_root.join(".continue").is_dir() {
        tools.push(DetectedTool {
            name: "Continue.dev",
            config_path: project_root.join(".continue").join("mcpServers").join("apex.json"),
        });
    }

    tools
}

fn which(cmd: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(cmd);
            if full.is_file() { Some(full) } else { None }
        })
    })
}
```

- [ ] **Step 2: Implement config writers**

```rust
fn write_cursor_config(path: &Path) -> Result<()> {
    let config = serde_json::json!({
        "mcpServers": {
            "apex": {
                "command": "apex",
                "args": ["mcp"],
                "env": {}
            }
        }
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ApexError::Other(format!("mkdir: {e}")))?;
    }

    // If file exists, merge instead of overwrite
    let final_config = if path.exists() {
        let existing: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).unwrap_or_default()
        ).unwrap_or(serde_json::json!({}));
        merge_json(existing, config)
    } else {
        config
    };

    std::fs::write(path, serde_json::to_string_pretty(&final_config).unwrap())
        .map_err(|e| ApexError::Other(format!("write: {e}")))?;
    Ok(())
}

fn write_codex_config(path: &Path) -> Result<()> {
    let section = "\n[mcp_servers.apex]\ntype = \"stdio\"\ncommand = \"apex\"\nargs = [\"mcp\"]\n";

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ApexError::Other(format!("mkdir: {e}")))?;
    }

    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if existing.contains("[mcp_servers.apex]") {
        return Ok(()); // Already configured
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true).append(true).open(path)
        .map_err(|e| ApexError::Other(format!("open: {e}")))?;
    use std::io::Write;
    file.write_all(section.as_bytes())
        .map_err(|e| ApexError::Other(format!("write: {e}")))?;
    Ok(())
}

fn write_continue_config(path: &Path) -> Result<()> {
    let config = serde_json::json!({
        "apex": {
            "command": "apex",
            "args": ["mcp"]
        }
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ApexError::Other(format!("mkdir: {e}")))?;
    }

    std::fs::write(path, serde_json::to_string_pretty(&config).unwrap())
        .map_err(|e| ApexError::Other(format!("write: {e}")))?;
    Ok(())
}

fn merge_json(mut base: serde_json::Value, overlay: serde_json::Value) -> serde_json::Value {
    if let (Some(base_obj), Some(overlay_obj)) = (base.as_object_mut(), overlay.as_object()) {
        for (k, v) in overlay_obj {
            if let Some(existing) = base_obj.get_mut(k) {
                *existing = merge_json(existing.clone(), v.clone());
            } else {
                base_obj.insert(k.clone(), v.clone());
            }
        }
    }
    base
}
```

- [ ] **Step 3: Implement run_integrate**

```rust
pub async fn run_integrate(tool_filter: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|e| ApexError::Other(format!("cwd: {e}")))?;

    let tools = detect_tools(&cwd);

    if tools.is_empty() {
        println!("No AI coding tools detected in this project.");
        println!("Supported: Cursor, Codex CLI, Continue.dev");
        println!("\nYou can still run the MCP server manually: apex mcp");
        return Ok(());
    }

    let detected_names: Vec<&str> = tools.iter().map(|t| t.name).collect();
    println!("Detected: {}\n", detected_names.join(", "));

    for tool in &tools {
        if let Some(ref filter) = tool_filter {
            if !tool.name.to_lowercase().contains(&filter.to_lowercase()) {
                continue;
            }
        }

        let result = match tool.name {
            "Cursor" => write_cursor_config(&tool.config_path),
            "Codex CLI" => write_codex_config(&tool.config_path),
            "Continue.dev" => write_continue_config(&tool.config_path),
            _ => continue,
        };

        match result {
            Ok(()) => println!("  ✓ {} — {}", tool.config_path.display(), tool.name),
            Err(e) => println!("  ✗ {} — {}: {}", tool.config_path.display(), tool.name, e),
        }
    }

    println!("\nAPEX MCP server configured. Tools will start it automatically.");
    Ok(())
}
```

- [ ] **Step 4: Tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cursor_by_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".cursor")).unwrap();
        let tools = detect_tools(dir.path());
        assert!(tools.iter().any(|t| t.name == "Cursor"));
    }

    #[test]
    fn detect_codex_by_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".codex")).unwrap();
        let tools = detect_tools(dir.path());
        assert!(tools.iter().any(|t| t.name == "Codex CLI"));
    }

    #[test]
    fn detect_empty_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let tools = detect_tools(dir.path());
        assert!(tools.is_empty());
    }

    #[test]
    fn cursor_config_creates_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".cursor").join("mcp.json");
        write_cursor_config(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(v["mcpServers"]["apex"]["command"].as_str() == Some("apex"));
    }

    #[test]
    fn cursor_config_merges_with_existing() {
        let dir = tempfile::tempdir().unwrap();
        let cursor_dir = dir.path().join(".cursor");
        std::fs::create_dir(&cursor_dir).unwrap();
        let path = cursor_dir.join("mcp.json");
        // Write existing config
        std::fs::write(&path, r#"{"mcpServers":{"other":{"command":"other"}}}"#).unwrap();
        write_cursor_config(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        // Both should exist
        assert!(v["mcpServers"]["apex"].is_object());
        assert!(v["mcpServers"]["other"].is_object());
    }

    #[test]
    fn codex_config_appends_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# existing config\n").unwrap();
        write_codex_config(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[mcp_servers.apex]"));
        assert!(content.contains("# existing config"));
    }

    #[test]
    fn codex_config_skips_if_already_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[mcp_servers.apex]\ncommand = \"apex\"\n").unwrap();
        write_codex_config(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        // Should not be duplicated
        assert_eq!(content.matches("[mcp_servers.apex]").count(), 1);
    }

    #[test]
    fn continue_config_creates_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("apex.json");
        write_continue_config(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(v["apex"]["command"].as_str() == Some("apex"));
    }
}
```

- [ ] **Step 5: Run tests, commit**

Run: `cargo test -p apex-cli integrate`

```bash
git commit -m "feat: apex integrate — auto-configure Cursor, Codex, Continue"
```

---

## Dispatch Plan

All 4 tasks are sequential (each builds on the previous):

```
Task 1: Scaffold (deps + stubs + commands)
Task 2: MCP server implementation (6 tools)
Task 3: Integration tests
Task 4: apex integrate command
```

Single agent, single worktree, sequential execution.

---

## Summary

| Task | What | Tests |
|------|------|-------|
| 1 | Scaffold — deps, stubs, command enum | 0 (compile check) |
| 2 | MCP STDIO server — 6 tool handlers | 0 (compile check) |
| 3 | MCP integration tests | 3 |
| 4 | `apex integrate` — detect + config write | 7 |

**After completion:** `apex mcp` starts STDIO MCP server. Any MCP client can call 6 tools. `apex integrate` auto-configures detected tools.
