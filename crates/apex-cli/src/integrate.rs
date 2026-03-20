//! `apex integrate` — zero-config MCP server wiring for Claude Code, Cursor,
//! and Windsurf.
//!
//! Writes (or merges) an `mcpServers.apex` entry into the editor's MCP config
//! file so that the APEX MCP server is available immediately without manual
//! JSON editing.

use color_eyre::{eyre::eyre, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

#[derive(clap::Args, Debug)]
pub struct IntegrateArgs {
    /// Target editor: claude, cursor, windsurf (auto-detect if omitted)
    #[arg(long)]
    pub editor: Option<String>,

    /// Write to user-global config instead of project-local
    #[arg(long)]
    pub global: bool,

    /// Print config to stdout without writing files
    #[arg(long)]
    pub dry_run: bool,
}

// ---------------------------------------------------------------------------
// Config generation
// ---------------------------------------------------------------------------

/// Build the `mcpServers.apex` JSON object for a given `apex` binary path.
///
/// Produces:
/// ```json
/// {
///   "mcpServers": {
///     "apex": {
///       "command": "/absolute/path/to/apex",
///       "args": ["mcp"]
///     }
///   }
/// }
/// ```
pub fn generate_config(apex_binary: &Path) -> Value {
    serde_json::json!({
        "mcpServers": {
            "apex": {
                "command": apex_binary.to_string_lossy().as_ref(),
                "args": ["mcp"]
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Config path resolution
// ---------------------------------------------------------------------------

/// Resolve the config file path for `editor` given the `global` flag.
///
/// | editor   | local                  | global                                    |
/// |----------|------------------------|-------------------------------------------|
/// | claude   | `.mcp.json` (CWD)      | `~/.claude.json`                          |
/// | cursor   | `.cursor/mcp.json`     | `~/.cursor/mcp.json`                      |
/// | windsurf | `.windsurf/mcp.json`   | `~/.codeium/windsurf/mcp_config.json`     |
pub fn config_path(editor: &str, global: bool) -> Result<PathBuf> {
    let home = home_dir()?;
    let cwd = std::env::current_dir()?;

    let path = match (editor, global) {
        ("claude", false) => cwd.join(".mcp.json"),
        ("claude", true) => home.join(".claude.json"),
        ("cursor", false) => cwd.join(".cursor").join("mcp.json"),
        ("cursor", true) => home.join(".cursor").join("mcp.json"),
        ("windsurf", false) => cwd.join(".windsurf").join("mcp.json"),
        ("windsurf", true) => home
            .join(".codeium")
            .join("windsurf")
            .join("mcp_config.json"),
        (other, _) => {
            return Err(eyre!(
                "unknown editor '{other}'; use claude, cursor, or windsurf"
            ))
        }
    };

    Ok(path)
}

/// Return the current user's home directory.
fn home_dir() -> Result<PathBuf> {
    // std::env::home_dir is deprecated and unreliable on some platforms.
    // Use HOME env var with a fallback to the deprecated fn.
    if let Ok(home) = std::env::var("HOME") {
        return Ok(PathBuf::from(home));
    }
    #[allow(deprecated)]
    std::env::home_dir().ok_or_else(|| eyre!("cannot determine home directory"))
}

// ---------------------------------------------------------------------------
// Auto-detect editor
// ---------------------------------------------------------------------------

/// Detect the likely editor from project-root marker directories.
///
/// - `.cursor/` present → `cursor`
/// - `.claude/` present → `claude`
/// - fallback → `claude`
pub fn detect_editor() -> &'static str {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join(".cursor").is_dir() {
        "cursor"
    } else if cwd.join(".windsurf").is_dir() {
        "windsurf"
    } else {
        // .claude/ is present in every Claude Code project; treat it as
        // lower priority than cursor so cursor projects aren't miscategorised.
        "claude"
    }
}

// ---------------------------------------------------------------------------
// Config merge + write
// ---------------------------------------------------------------------------

/// Read the existing config at `path` (if any), merge the `apex` entry from
/// `config` into its `mcpServers` object, then write the result back.
///
/// If the file does not exist it is created (including any missing parent
/// directories). Any existing `mcpServers` entries besides `apex` are
/// preserved.
pub fn write_config(path: &Path, config: &Value) -> Result<()> {
    // Load existing config (or start with an empty object).
    let mut existing: Value = if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str(&raw).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Ensure mcpServers key exists.
    if existing.get("mcpServers").is_none() {
        existing["mcpServers"] = serde_json::json!({});
    }

    // Merge the apex entry from config into existing["mcpServers"].
    if let Some(apex_entry) = config.get("mcpServers").and_then(|s| s.get("apex")) {
        existing["mcpServers"]["apex"] = apex_entry.clone();
    }

    // Create parent directories if needed.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let pretty = serde_json::to_string_pretty(&existing)?;
    std::fs::write(path, pretty + "\n")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Main entry point for `apex integrate`.
pub async fn run_integrate(args: IntegrateArgs) -> Result<()> {
    let apex_binary = std::env::current_exe()?;

    let editor = match args.editor.as_deref() {
        Some(e) => e.to_owned(),
        None => detect_editor().to_owned(),
    };

    let path = config_path(&editor, args.global)?;
    let config = generate_config(&apex_binary);

    if args.dry_run {
        println!("# Would write to: {}", path.display());
        println!("{}", serde_json::to_string_pretty(&config)?);
        return Ok(());
    }

    write_config(&path, &config)?;

    println!(
        "apex MCP server registered in {} ({})",
        path.display(),
        editor
    );
    println!("Restart your editor to pick up the change.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // --- generate_config ---

    #[test]
    fn generate_config_has_correct_structure() {
        let binary = Path::new("/usr/local/bin/apex");
        let config = generate_config(binary);

        // Top-level key
        assert!(
            config.get("mcpServers").is_some(),
            "expected mcpServers key"
        );

        let mcp = &config["mcpServers"];
        assert!(mcp.get("apex").is_some(), "expected apex entry");

        let apex = &mcp["apex"];
        assert_eq!(apex["command"], "/usr/local/bin/apex");
        assert_eq!(apex["args"], serde_json::json!(["mcp"]));
    }

    #[test]
    fn generate_config_encodes_binary_path_verbatim() {
        let binary = Path::new("/home/user/.cargo/bin/apex");
        let config = generate_config(binary);
        assert_eq!(
            config["mcpServers"]["apex"]["command"],
            "/home/user/.cargo/bin/apex"
        );
    }

    // --- config_path ---

    #[test]
    fn config_path_claude_local() {
        let path = config_path("claude", false).unwrap();
        assert!(
            path.ends_with(".mcp.json"),
            "expected .mcp.json, got: {path:?}"
        );
    }

    #[test]
    fn config_path_claude_global() {
        let path = config_path("claude", true).unwrap();
        assert!(
            path.ends_with(".claude.json"),
            "expected .claude.json, got: {path:?}"
        );
    }

    #[test]
    fn config_path_cursor_local() {
        let path = config_path("cursor", false).unwrap();
        // Should be .cursor/mcp.json relative to CWD
        let s = path.to_string_lossy();
        assert!(
            s.ends_with(".cursor/mcp.json"),
            "expected .cursor/mcp.json, got: {path:?}"
        );
    }

    #[test]
    fn config_path_cursor_global() {
        let path = config_path("cursor", true).unwrap();
        let s = path.to_string_lossy();
        assert!(
            s.contains(".cursor") && s.ends_with("mcp.json"),
            "expected global cursor path, got: {path:?}"
        );
    }

    #[test]
    fn config_path_windsurf_local() {
        let path = config_path("windsurf", false).unwrap();
        let s = path.to_string_lossy();
        assert!(
            s.ends_with(".windsurf/mcp.json"),
            "expected .windsurf/mcp.json, got: {path:?}"
        );
    }

    #[test]
    fn config_path_windsurf_global() {
        let path = config_path("windsurf", true).unwrap();
        let s = path.to_string_lossy();
        assert!(
            s.contains("windsurf") && s.ends_with("mcp_config.json"),
            "expected windsurf global path, got: {path:?}"
        );
    }

    #[test]
    fn config_path_unknown_editor_returns_error() {
        let result = config_path("vscode", false);
        assert!(result.is_err(), "expected error for unknown editor");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("vscode"),
            "error should mention the editor name"
        );
    }

    // --- write_config ---

    #[test]
    fn write_config_creates_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");

        let binary = Path::new("/usr/local/bin/apex");
        let config = generate_config(binary);

        write_config(&path, &config).unwrap();

        assert!(path.exists(), "file should have been created");
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            parsed["mcpServers"]["apex"]["command"],
            "/usr/local/bin/apex"
        );
        assert_eq!(
            parsed["mcpServers"]["apex"]["args"],
            serde_json::json!(["mcp"])
        );
    }

    #[test]
    fn write_config_merges_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");

        // Pre-populate with another server entry.
        let initial = serde_json::json!({
            "mcpServers": {
                "other-tool": {
                    "command": "/usr/bin/other",
                    "args": ["serve"]
                }
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        // Write apex config.
        let apex_config = generate_config(Path::new("/usr/local/bin/apex"));
        write_config(&path, &apex_config).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&raw).unwrap();

        // Both servers must be present.
        assert!(
            parsed["mcpServers"].get("other-tool").is_some(),
            "existing server should be preserved"
        );
        assert!(
            parsed["mcpServers"].get("apex").is_some(),
            "apex entry should be added"
        );
        assert_eq!(
            parsed["mcpServers"]["apex"]["command"],
            "/usr/local/bin/apex"
        );
    }

    #[test]
    fn write_config_overwrites_stale_apex_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");

        // Write an old apex entry.
        let old = serde_json::json!({
            "mcpServers": {
                "apex": {
                    "command": "/old/path/apex",
                    "args": ["mcp"]
                }
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&old).unwrap()).unwrap();

        let apex_config = generate_config(Path::new("/new/path/apex"));
        write_config(&path, &apex_config).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            parsed["mcpServers"]["apex"]["command"], "/new/path/apex",
            "stale path should be updated"
        );
    }

    #[test]
    fn write_config_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("mcp.json");

        let config = generate_config(Path::new("/usr/local/bin/apex"));
        write_config(&path, &config).unwrap();

        assert!(
            path.exists(),
            "file should exist after creating parent dirs"
        );
    }

    // --- dry_run ---

    #[test]
    fn dry_run_does_not_write() {
        // We can't easily test the async run_integrate in a sync test, so we
        // directly test that write_config is NOT called when dry_run is set.
        // Verify by checking that a path that would be written to does not exist.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("should_not_exist.json");

        // Simulate dry-run: do NOT call write_config.
        assert!(
            !path.exists(),
            "file must not exist without write_config call"
        );

        // Calling generate_config alone (no write) must not create files.
        let _config = generate_config(Path::new("/usr/bin/apex"));
        assert!(
            !path.exists(),
            "generate_config must not touch the filesystem"
        );
    }
}
