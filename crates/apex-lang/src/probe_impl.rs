//! Environment probe — detects runtime toolchain details for a project.
//!
//! `probe_all()` gathers language-specific information (interpreter path,
//! venv, package manager, coverage tools) and returns an `EnvironmentProbe`
//! that can be cached to `.apex/environment.json` and loaded on future runs.

use apex_core::types::Language;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Cached probe result.  Serialised to `.apex/environment.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvironmentProbe {
    /// Unix timestamp of when this probe was collected.
    pub collected_at: u64,
    /// Python-specific findings, if the project is Python (or Python was detected).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python: Option<PythonProbe>,
    /// Rust-specific findings, if the project is Rust.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust: Option<RustProbe>,
    /// JavaScript/Node-specific findings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeProbe>,
    /// Go-specific findings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub go: Option<GoProbe>,
    /// Java-specific findings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java: Option<JavaProbe>,
}

/// How fresh a probe is considered — 7 days in seconds.
const FRESH_SECONDS: u64 = 7 * 24 * 3600;

impl EnvironmentProbe {
    /// Returns true when the cached probe is still fresh (collected < 7 days ago).
    pub fn is_fresh(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.collected_at) < FRESH_SECONDS
    }

    /// Load a cached probe from `<root>/.apex/environment.json`.
    /// Returns `None` when the file does not exist or cannot be parsed.
    pub fn load_cached(root: &Path) -> Option<Self> {
        let path = root.join(".apex").join("environment.json");
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Persist the probe to `<root>/.apex/environment.json`.
    /// Creates the `.apex/` directory if necessary.
    pub fn save_cache(&self, root: &Path) -> std::io::Result<()> {
        let dir = root.join(".apex");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("environment.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// One-line human-readable summary.
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(ref py) = self.python {
            let mut s = format!("Python {}", py.version);
            if let Some(ref venv) = py.venv {
                s.push_str(&format!(" via {}", venv.display()));
            }
            let mut flags: Vec<&str> = Vec::new();
            if let Some(ref pm) = py.package_manager {
                flags.push(pm.as_str());
            }
            if py.pytest_available {
                flags.push("pytest");
            }
            if py.coverage_py_available {
                flags.push("coverage.py");
            }
            if py.pep668_managed {
                flags.push("PEP668");
            }
            if !flags.is_empty() {
                s.push_str(&format!(" ({})", flags.join(", ")));
            }
            parts.push(s);
        }

        if let Some(ref rs) = self.rust {
            let mut s = format!("Rust {}", rs.toolchain);
            let mut flags: Vec<&str> = Vec::new();
            if rs.llvm_cov.is_some() {
                flags.push("llvm-cov");
            }
            if rs.nextest_available {
                flags.push("nextest");
            }
            if !flags.is_empty() {
                s.push_str(&format!(" ({})", flags.join(", ")));
            }
            parts.push(s);
        }

        if let Some(ref nd) = self.node {
            let mut s = format!("Node {}", nd.version);
            if let Some(ref pm) = nd.package_manager {
                s.push_str(&format!(" ({})", pm));
            }
            parts.push(s);
        }

        if let Some(ref go) = self.go {
            parts.push(format!("Go {}", go.version));
        }

        if let Some(ref java) = self.java {
            parts.push(format!("Java {}", java.version));
        }

        if parts.is_empty() {
            "no language toolchain detected".to_string()
        } else {
            parts.join("; ")
        }
    }
}

impl fmt::Display for EnvironmentProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary())
    }
}

// ---------------------------------------------------------------------------
// Language-specific probe structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonProbe {
    /// Python version string, e.g. "3.14.3".
    pub version: String,
    /// Path to the Python interpreter.
    pub interpreter: PathBuf,
    /// Active virtual environment directory (if any).
    pub venv: Option<PathBuf>,
    /// Package manager: "pip", "uv", "poetry", "pdm", "conda".
    pub package_manager: Option<String>,
    /// Whether pytest is importable.
    pub pytest_available: bool,
    /// Whether coverage.py is importable.
    pub coverage_py_available: bool,
    /// Whether this is a PEP 668 externally-managed environment.
    pub pep668_managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustProbe {
    /// Active toolchain name, e.g. "stable" or "1.78.0-x86_64-unknown-linux-gnu".
    pub toolchain: String,
    /// Path to cargo-llvm-cov binary if available.
    pub llvm_cov: Option<PathBuf>,
    /// Whether cargo-nextest is available.
    pub nextest_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProbe {
    /// Node.js version string, e.g. "v20.11.0".
    pub version: String,
    /// Detected package manager: "npm", "yarn", "pnpm", "bun".
    pub package_manager: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoProbe {
    /// Go version string, e.g. "go1.22.1".
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaProbe {
    /// Java version string.
    pub version: String,
    /// Build tool: "maven" or "gradle".
    pub build_tool: Option<String>,
}

// ---------------------------------------------------------------------------
// Probe implementation — synchronous, no tokio dependency
// ---------------------------------------------------------------------------

/// Run the full environment probe for a project root and language hint.
///
/// This function is intentionally **synchronous** so it can be called from
/// both sync and async contexts without spawning a blocking task.  Each
/// individual check is a fast filesystem/`Command::output()` call.
pub fn probe_all(root: &Path, lang: Language) -> EnvironmentProbe {
    let collected_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let python = match lang {
        Language::Python => Some(probe_python(root)),
        _ => {
            // Try Python opportunistically (e.g. mixed repos)
            if root.join("pyproject.toml").exists()
                || root.join("setup.py").exists()
                || root.join("requirements.txt").exists()
            {
                Some(probe_python(root))
            } else {
                None
            }
        }
    };

    let rust = match lang {
        Language::Rust => Some(probe_rust(root)),
        _ => {
            if root.join("Cargo.toml").exists() {
                Some(probe_rust(root))
            } else {
                None
            }
        }
    };

    let node = match lang {
        Language::JavaScript => Some(probe_node(root)),
        _ => {
            if root.join("package.json").exists() {
                Some(probe_node(root))
            } else {
                None
            }
        }
    };

    let go = match lang {
        Language::Go => Some(probe_go()),
        _ => {
            if root.join("go.mod").exists() {
                Some(probe_go())
            } else {
                None
            }
        }
    };

    let java = match lang {
        Language::Java | Language::Kotlin => Some(probe_java(root)),
        _ => None,
    };

    EnvironmentProbe {
        collected_at,
        python,
        rust,
        node,
        go,
        java,
    }
}

// ---------------------------------------------------------------------------
// Per-language probes
// ---------------------------------------------------------------------------

fn run_cmd_stdout(program: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

fn python_interpreter(root: &Path) -> PathBuf {
    // Prefer project-local venv
    for candidate in [".venv/bin/python", "venv/bin/python", ".venv/bin/python3"] {
        let p = root.join(candidate);
        if p.exists() {
            return p;
        }
    }
    // System python3
    PathBuf::from("python3")
}

fn probe_python(root: &Path) -> PythonProbe {
    let interpreter = python_interpreter(root);
    let interp_str = interpreter.to_string_lossy().to_string();

    let version = run_cmd_stdout(&interp_str, &["-c", "import sys; print(sys.version.split()[0])"])
        .unwrap_or_else(|| "unknown".to_string());

    // Detect venv
    let venv: Option<PathBuf> = [".venv", "venv", ".env"]
        .iter()
        .map(|v| root.join(v))
        .find(|p| p.join("bin/python").exists() || p.join("bin/python3").exists());

    // Package manager
    let package_manager: Option<String> = if run_cmd_stdout("uv", &["--version"]).is_some() {
        Some("uv".to_string())
    } else if run_cmd_stdout("poetry", &["--version"]).is_some() {
        Some("poetry".to_string())
    } else if run_cmd_stdout("pdm", &["--version"]).is_some() {
        Some("pdm".to_string())
    } else if run_cmd_stdout("pip", &["--version"]).is_some() {
        Some("pip".to_string())
    } else {
        None
    };

    // pytest availability
    let pytest_available = std::process::Command::new(&interp_str)
        .args(["-c", "import pytest"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    // coverage.py
    let coverage_py_available = std::process::Command::new(&interp_str)
        .args(["-c", "import coverage"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    // PEP 668 — check for EXTERNALLY-MANAGED marker
    let pep668_managed = run_cmd_stdout(
        &interp_str,
        &[
            "-c",
            "import sysconfig, os; print(os.path.exists(os.path.join(sysconfig.get_path('stdlib'), '..', 'EXTERNALLY-MANAGED')))",
        ],
    )
    .map(|s| s.trim() == "True")
    .unwrap_or(false);

    PythonProbe {
        version,
        interpreter,
        venv,
        package_manager,
        pytest_available,
        coverage_py_available,
        pep668_managed,
    }
}

fn probe_rust(root: &Path) -> RustProbe {
    // Active toolchain
    let toolchain = run_cmd_stdout("rustup", &["show", "active-toolchain"])
        .map(|s| {
            // "stable-x86_64-apple-darwin (default)" → keep first word
            s.split_whitespace()
                .next()
                .unwrap_or("stable")
                .to_string()
        })
        .or_else(|| run_cmd_stdout("rustc", &["--version"]))
        .unwrap_or_else(|| "unknown".to_string());

    // cargo-llvm-cov
    let llvm_cov = which_bin("cargo-llvm-cov")
        .or_else(|| which_bin_path(root, "cargo-llvm-cov"));

    // cargo-nextest
    let nextest_available = run_cmd_stdout("cargo", &["nextest", "--version"]).is_some();

    RustProbe {
        toolchain,
        llvm_cov,
        nextest_available,
    }
}

fn probe_node(root: &Path) -> NodeProbe {
    let version = run_cmd_stdout("node", &["--version"]).unwrap_or_else(|| "unknown".to_string());

    // Detect package manager from lockfile
    let package_manager: Option<String> = if root.join("yarn.lock").exists() {
        Some("yarn".to_string())
    } else if root.join("pnpm-lock.yaml").exists() {
        Some("pnpm".to_string())
    } else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
        Some("bun".to_string())
    } else if root.join("package-lock.json").exists() {
        Some("npm".to_string())
    } else {
        // Fallback: check what's in PATH
        if run_cmd_stdout("yarn", &["--version"]).is_some() {
            Some("yarn".to_string())
        } else if run_cmd_stdout("pnpm", &["--version"]).is_some() {
            Some("pnpm".to_string())
        } else {
            None
        }
    };

    NodeProbe {
        version,
        package_manager,
    }
}

fn probe_go() -> GoProbe {
    let version =
        run_cmd_stdout("go", &["version"])
            .map(|s| {
                // "go version go1.22.1 linux/amd64" → "go1.22.1"
                s.split_whitespace()
                    .nth(2)
                    .unwrap_or("unknown")
                    .to_string()
            })
            .unwrap_or_else(|| "unknown".to_string());

    GoProbe { version }
}

fn probe_java(root: &Path) -> JavaProbe {
    let version = run_cmd_stdout("java", &["-version"])
        .or_else(|| {
            // java -version often prints to stderr
            std::process::Command::new("java")
                .arg("-version")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stderr).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());

    let build_tool: Option<String> = if root.join("pom.xml").exists() {
        Some("maven".to_string())
    } else if root.join("build.gradle").exists() || root.join("build.gradle.kts").exists() {
        Some("gradle".to_string())
    } else {
        None
    };

    JavaProbe {
        version,
        build_tool,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn which_bin(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .as_deref()
        .and_then(|path| {
            std::env::split_paths(path).find_map(|dir| {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    Some(candidate)
                } else {
                    None
                }
            })
        })
}

fn which_bin_path(root: &Path, name: &str) -> Option<PathBuf> {
    let candidate = root.join("target").join("release").join(name);
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn probe_default_is_empty() {
        let probe = EnvironmentProbe::default();
        assert!(probe.python.is_none());
        assert!(probe.rust.is_none());
        assert!(probe.node.is_none());
        assert_eq!(probe.summary(), "no language toolchain detected");
    }

    #[test]
    fn is_fresh_for_new_probe() {
        let probe = EnvironmentProbe {
            collected_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            ..Default::default()
        };
        assert!(probe.is_fresh());
    }

    #[test]
    fn is_stale_for_old_probe() {
        let probe = EnvironmentProbe {
            collected_at: 0, // epoch
            ..Default::default()
        };
        assert!(!probe.is_fresh());
    }

    #[test]
    fn save_and_load_cache_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let probe = EnvironmentProbe {
            collected_at: 9999,
            rust: Some(RustProbe {
                toolchain: "stable".to_string(),
                llvm_cov: None,
                nextest_available: true,
            }),
            ..Default::default()
        };

        probe.save_cache(root).unwrap();
        let loaded = EnvironmentProbe::load_cached(root).unwrap();
        assert_eq!(loaded.collected_at, 9999);
        let rs = loaded.rust.unwrap();
        assert_eq!(rs.toolchain, "stable");
        assert!(rs.nextest_available);
    }

    #[test]
    fn load_cache_returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(EnvironmentProbe::load_cached(tmp.path()).is_none());
    }

    #[test]
    fn summary_python_only() {
        let probe = EnvironmentProbe {
            collected_at: 0,
            python: Some(PythonProbe {
                version: "3.14.3".to_string(),
                interpreter: PathBuf::from(".venv/bin/python"),
                venv: Some(PathBuf::from(".venv")),
                package_manager: Some("uv".to_string()),
                pytest_available: true,
                coverage_py_available: true,
                pep668_managed: false,
            }),
            ..Default::default()
        };
        let s = probe.summary();
        assert!(s.contains("3.14.3"), "version in summary");
        assert!(s.contains("uv"), "package manager in summary");
        assert!(s.contains("pytest"), "pytest in summary");
    }

    #[test]
    fn summary_rust_only() {
        let probe = EnvironmentProbe {
            collected_at: 0,
            rust: Some(RustProbe {
                toolchain: "stable".to_string(),
                llvm_cov: Some(PathBuf::from("/usr/bin/cargo-llvm-cov")),
                nextest_available: true,
            }),
            ..Default::default()
        };
        let s = probe.summary();
        assert!(s.contains("stable"), "toolchain in summary");
        assert!(s.contains("llvm-cov"), "llvm-cov in summary");
        assert!(s.contains("nextest"), "nextest in summary");
    }

    #[test]
    fn probe_all_rust_project_detects_rust_section() {
        let tmp = TempDir::new().unwrap();
        // Create a minimal Cargo.toml so the opportunistic detection fires.
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname=\"test\"\n").unwrap();
        let probe = probe_all(tmp.path(), Language::Rust);
        // We may or may not have rustc in PATH in CI, but the section should exist.
        assert!(probe.rust.is_some(), "Rust section should be populated for a Rust project");
    }

    #[test]
    fn probe_all_python_project_detects_python_section() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("pyproject.toml"), "[project]\nname=\"test\"\n").unwrap();
        let probe = probe_all(tmp.path(), Language::Python);
        assert!(probe.python.is_some());
    }

    #[test]
    fn probe_all_no_project_files_is_language_driven() {
        let tmp = TempDir::new().unwrap();
        // No files at all, language = Go
        let probe = probe_all(tmp.path(), Language::Go);
        // go section should still be populated (we always try for the explicit lang)
        assert!(probe.go.is_some());
        assert!(probe.rust.is_none());
        assert!(probe.python.is_none());
    }
}
