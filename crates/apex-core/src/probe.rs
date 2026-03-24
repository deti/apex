//! Environment probe types for APEX.
//!
//! `EnvironmentProbe` captures what runtimes, tools, and package managers are
//! present in a target project.  The probe result is written to
//! `.apex/environment.json` so subsequent commands skip re-detection when the
//! cache is fresh (less than 1 hour old).

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::types::Language;

// ---------------------------------------------------------------------------
// Top-level probe
// ---------------------------------------------------------------------------

/// Complete environment probe result for a target project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentProbe {
    /// ISO 8601 timestamp of when the probe was captured.
    pub detected_at: String,
    pub target_root: PathBuf,
    pub primary_language: Option<String>,
    pub python: Option<PythonEnv>,
    pub javascript: Option<JsEnv>,
    pub rust: Option<RustEnv>,
    pub go: Option<GoEnv>,
    pub java: Option<JvmEnv>,
    pub ruby: Option<RubyEnv>,
    pub c_cpp: Option<CCppEnv>,
    pub swift: Option<SwiftEnv>,
    pub csharp: Option<DotnetEnv>,
}

impl EnvironmentProbe {
    /// Create an empty probe for the given target directory.
    pub fn empty(target: &Path) -> Self {
        EnvironmentProbe {
            detected_at: chrono_now(),
            target_root: target.to_path_buf(),
            primary_language: None,
            python: None,
            javascript: None,
            rust: None,
            go: None,
            java: None,
            ruby: None,
            c_cpp: None,
            swift: None,
            csharp: None,
        }
    }

    /// Return `true` if the probe contains data for the given language.
    pub fn has_language(&self, lang: Language) -> bool {
        match lang {
            Language::Python => self.python.is_some(),
            Language::JavaScript => self.javascript.is_some(),
            Language::Rust => self.rust.is_some(),
            Language::Go => self.go.is_some(),
            Language::Java | Language::Kotlin => self.java.is_some(),
            Language::Ruby => self.ruby.is_some(),
            Language::C | Language::Cpp => self.c_cpp.is_some(),
            Language::Swift => self.swift.is_some(),
            Language::CSharp => self.csharp.is_some(),
            Language::Wasm => false,
        }
    }

    /// One-line human-readable summary of detected languages and tools.
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(ref py) = self.python {
            parts.push(format!("python {}", py.version));
        }
        if let Some(ref js) = self.javascript {
            parts.push(format!("{} {}", js.runtime, js.version));
        }
        if let Some(ref rs) = self.rust {
            parts.push(format!("rust {}", rs.version));
        }
        if let Some(ref go) = self.go {
            parts.push(format!("go {}", go.version));
        }
        if let Some(ref jvm) = self.java {
            if let Some(ref v) = jvm.java_version {
                parts.push(format!("java {v}"));
            }
            if let Some(ref v) = jvm.kotlin_version {
                parts.push(format!("kotlin {v}"));
            }
        }
        if let Some(ref rb) = self.ruby {
            parts.push(format!("ruby {}", rb.version));
        }
        if let Some(ref cc) = self.c_cpp {
            parts.push(format!("{} {}", cc.compiler, cc.version));
        }
        if let Some(ref sw) = self.swift {
            parts.push(format!("swift {}", sw.version));
        }
        if let Some(ref cs) = self.csharp {
            parts.push(format!("dotnet {}", cs.version));
        }

        if parts.is_empty() {
            "no languages detected".to_string()
        } else {
            parts.join(", ")
        }
    }

    // -----------------------------------------------------------------------
    // Cache persistence
    // -----------------------------------------------------------------------

    /// Load a cached probe from `<target>/.apex/environment.json`.
    /// Returns `None` if the file does not exist or cannot be parsed.
    pub fn load_cached(target: &Path) -> Option<Self> {
        let cache_path = target.join(".apex").join("environment.json");
        let data = std::fs::read_to_string(&cache_path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Save this probe to `<target>/.apex/environment.json`.
    pub fn save_cache(&self, target: &Path) -> Result<()> {
        let cache_dir = target.join(".apex");
        std::fs::create_dir_all(&cache_dir)?;
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(cache_dir.join("environment.json"), data)?;
        Ok(())
    }

    /// Return `true` if the probe was captured less than 1 hour ago.
    pub fn is_fresh(&self) -> bool {
        // Parse ISO 8601 with a simple heuristic: compare the timestamp string
        // to the current system time.  We use std::time to avoid extra deps.
        // Format: "YYYY-MM-DDTHH:MM:SSZ"
        parse_iso8601_secs(&self.detected_at)
            .map(|probe_secs| {
                let now = system_time_secs();
                now.saturating_sub(probe_secs) < 3600
            })
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Language environment structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonEnv {
    pub interpreter: PathBuf,
    pub version: String,
    pub venv: Option<PathBuf>,
    pub coverage_tool: Option<String>,
    pub test_runner: Option<String>,
    pub package_manager: Option<String>,
    /// True when the system Python is PEP 668 externally-managed.
    pub pep668_managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsEnv {
    /// Runtime name: "node", "bun", or "deno".
    pub runtime: String,
    pub version: String,
    /// Package manager: "npm", "yarn", "pnpm", or "bun".
    pub package_manager: String,
    pub test_runner: Option<String>,
    pub coverage_tool: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustEnv {
    /// Full toolchain identifier, e.g. "stable-aarch64-apple-darwin".
    pub toolchain: String,
    pub version: String,
    /// Installed `cargo-llvm-cov` version, if present.
    pub llvm_cov: Option<String>,
    /// Installed `cargo-nextest` version, if present.
    pub nextest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoEnv {
    pub version: String,
    /// True when the built-in `go test -cover` tool is available.
    pub go_cover: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JvmEnv {
    pub java_version: Option<String>,
    pub kotlin_version: Option<String>,
    /// Build tool: "gradle" or "maven".
    pub build_tool: Option<String>,
    /// Coverage tool: "jacoco" or "kover".
    pub coverage_tool: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubyEnv {
    pub version: String,
    pub test_runner: Option<String>,
    pub coverage_tool: Option<String>,
    /// Version manager: "mise", "rbenv", or "rvm".
    pub version_manager: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CCppEnv {
    /// Compiler name: "gcc" or "clang".
    pub compiler: String,
    pub version: String,
    /// Build system: "cmake", "xmake", or "make".
    pub build_system: Option<String>,
    /// Coverage tool: "gcov" or "llvm-cov".
    pub coverage_tool: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwiftEnv {
    pub version: String,
    /// True when Swift Package Manager is available.
    pub spm: bool,
    /// True when code-coverage flags are supported.
    pub coverage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DotnetEnv {
    pub version: String,
    /// Coverage tool, e.g. "coverlet".
    pub coverage_tool: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the current UTC time as an ISO 8601 string (second precision).
fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    secs_to_iso8601(secs)
}

/// Convert Unix seconds to a minimal ISO 8601 UTC string.
fn secs_to_iso8601(secs: u64) -> String {
    // Calculate year/month/day/hour/minute/second from Unix seconds.
    let mut remaining = secs;
    let second = remaining % 60;
    remaining /= 60;
    let minute = remaining % 60;
    remaining /= 60;
    let hour = remaining % 24;
    remaining /= 24;

    // Days since epoch (1970-01-01)
    let mut days = remaining as u32;
    let mut year = 1970u32;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let month_days: [u32; 12] = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = days + 1;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn is_leap(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// Return current Unix time in seconds.
fn system_time_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Parse a minimal ISO 8601 UTC string ("YYYY-MM-DDTHH:MM:SSZ") into Unix seconds.
/// Returns `None` on parse failure.
fn parse_iso8601_secs(s: &str) -> Option<u64> {
    // Expected format: "YYYY-MM-DDTHH:MM:SSZ"
    let s = s.trim_end_matches('Z');
    let (date_part, time_part) = s.split_once('T')?;
    let mut date_iter = date_part.splitn(3, '-');
    let year: u32 = date_iter.next()?.parse().ok()?;
    let month: u32 = date_iter.next()?.parse().ok()?;
    let day: u32 = date_iter.next()?.parse().ok()?;

    let mut time_iter = time_part.splitn(3, ':');
    let hour: u64 = time_iter.next()?.parse().ok()?;
    let minute: u64 = time_iter.next()?.parse().ok()?;
    let second: u64 = time_iter.next()?.parse().ok()?;

    // Days from epoch to start of given year
    let mut total_days: u64 = 0;
    for y in 1970..year {
        total_days += if is_leap(y) { 366 } else { 365 };
    }

    // Days from start of year to start of given month
    let month_days: [u32; 12] = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for &md in month_days.iter().take((month as usize).saturating_sub(1)) {
        total_days += md as u64;
    }

    // Add days within month (1-based, subtract 1)
    total_days += (day as u64).saturating_sub(1);

    let total_secs = total_days * 86400 + hour * 3600 + minute * 60 + second;
    Some(total_secs)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Language;
    use std::path::PathBuf;

    fn sample_probe(target: &Path) -> EnvironmentProbe {
        let mut probe = EnvironmentProbe::empty(target);
        probe.python = Some(PythonEnv {
            interpreter: PathBuf::from("/usr/bin/python3"),
            version: "3.11.0".into(),
            venv: Some(PathBuf::from(".venv")),
            coverage_tool: Some("coverage.py".into()),
            test_runner: Some("pytest".into()),
            package_manager: Some("pip".into()),
            pep668_managed: false,
        });
        probe.rust = Some(RustEnv {
            toolchain: "stable-aarch64-apple-darwin".into(),
            version: "1.76.0".into(),
            llvm_cov: Some("0.6.0".into()),
            nextest: Some("0.9.67".into()),
        });
        probe
    }

    // -----------------------------------------------------------------------
    // Serialize / deserialize roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn serialize_deserialize_roundtrip() {
        let target = PathBuf::from("/tmp/project");
        let probe = sample_probe(&target);
        let json = serde_json::to_string(&probe).unwrap();
        let back: EnvironmentProbe = serde_json::from_str(&json).unwrap();
        assert_eq!(back.target_root, target);
        assert!(back.python.is_some());
        assert!(back.rust.is_some());
        assert!(back.javascript.is_none());
    }

    // -----------------------------------------------------------------------
    // empty() has no languages
    // -----------------------------------------------------------------------

    #[test]
    fn empty_probe_has_no_languages() {
        let probe = EnvironmentProbe::empty(Path::new("/tmp/x"));
        assert!(probe.python.is_none());
        assert!(probe.javascript.is_none());
        assert!(probe.rust.is_none());
        assert!(probe.go.is_none());
        assert!(probe.java.is_none());
        assert!(probe.ruby.is_none());
        assert!(probe.c_cpp.is_none());
        assert!(probe.swift.is_none());
        assert!(probe.csharp.is_none());
        assert!(probe.primary_language.is_none());
    }

    // -----------------------------------------------------------------------
    // has_language
    // -----------------------------------------------------------------------

    #[test]
    fn has_language_reflects_populated_fields() {
        let probe = sample_probe(Path::new("/tmp/x"));
        assert!(probe.has_language(Language::Python));
        assert!(probe.has_language(Language::Rust));
        assert!(!probe.has_language(Language::Go));
        assert!(!probe.has_language(Language::JavaScript));
        assert!(!probe.has_language(Language::Java));
        assert!(!probe.has_language(Language::Ruby));
        assert!(!probe.has_language(Language::C));
        assert!(!probe.has_language(Language::Cpp));
        assert!(!probe.has_language(Language::Swift));
        assert!(!probe.has_language(Language::CSharp));
    }

    #[test]
    fn has_language_kotlin_uses_java_field() {
        let mut probe = EnvironmentProbe::empty(Path::new("/tmp/x"));
        probe.java = Some(JvmEnv {
            java_version: None,
            kotlin_version: Some("1.9.0".into()),
            build_tool: None,
            coverage_tool: None,
        });
        assert!(probe.has_language(Language::Kotlin));
        assert!(probe.has_language(Language::Java));
    }

    #[test]
    fn has_language_wasm_always_false() {
        let probe = sample_probe(Path::new("/tmp/x"));
        assert!(!probe.has_language(Language::Wasm));
    }

    // -----------------------------------------------------------------------
    // summary()
    // -----------------------------------------------------------------------

    #[test]
    fn summary_produces_readable_output() {
        let probe = sample_probe(Path::new("/tmp/x"));
        let s = probe.summary();
        assert!(s.contains("python"), "summary was: {s}");
        assert!(s.contains("rust"), "summary was: {s}");
    }

    #[test]
    fn summary_empty_probe_says_no_languages() {
        let probe = EnvironmentProbe::empty(Path::new("/tmp/x"));
        assert_eq!(probe.summary(), "no languages detected");
    }

    // -----------------------------------------------------------------------
    // PythonEnv: pep668_managed flag
    // -----------------------------------------------------------------------

    #[test]
    fn python_env_pep668_managed_flag() {
        let py = PythonEnv {
            interpreter: PathBuf::from("/usr/bin/python3"),
            version: "3.12.0".into(),
            venv: None,
            coverage_tool: None,
            test_runner: None,
            package_manager: None,
            pep668_managed: true,
        };
        assert!(py.pep668_managed);
        let json = serde_json::to_string(&py).unwrap();
        let back: PythonEnv = serde_json::from_str(&json).unwrap();
        assert!(back.pep668_managed);
    }

    // -----------------------------------------------------------------------
    // Cache: save + load roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn cache_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path();
        let probe = sample_probe(target);
        probe.save_cache(target).unwrap();

        let loaded = EnvironmentProbe::load_cached(target).unwrap();
        assert_eq!(loaded.target_root, probe.target_root);
        assert!(loaded.python.is_some());
        assert!(loaded.rust.is_some());
    }

    // -----------------------------------------------------------------------
    // Cache: load from nonexistent path returns None
    // -----------------------------------------------------------------------

    #[test]
    fn load_cached_nonexistent_returns_none() {
        let result = EnvironmentProbe::load_cached(Path::new("/nonexistent/path/xyz"));
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Cache: is_fresh within 1 hour → true
    // -----------------------------------------------------------------------

    #[test]
    fn is_fresh_recent_timestamp_returns_true() {
        // A probe with detected_at set to "now" should be fresh.
        let mut probe = EnvironmentProbe::empty(Path::new("/tmp/x"));
        probe.detected_at = chrono_now();
        assert!(probe.is_fresh());
    }

    #[test]
    fn is_fresh_old_timestamp_returns_false() {
        // A probe detected 2 hours ago should not be fresh.
        let mut probe = EnvironmentProbe::empty(Path::new("/tmp/x"));
        // Subtract 2 hours (7200 seconds) from now
        let secs = system_time_secs().saturating_sub(7200);
        probe.detected_at = secs_to_iso8601(secs);
        assert!(!probe.is_fresh());
    }

    // -----------------------------------------------------------------------
    // ISO 8601 helpers
    // -----------------------------------------------------------------------

    #[test]
    fn iso8601_roundtrip() {
        let secs: u64 = 1_700_000_000; // a known Unix timestamp
        let s = secs_to_iso8601(secs);
        let back = parse_iso8601_secs(&s).unwrap();
        assert_eq!(back, secs);
    }

    #[test]
    fn parse_iso8601_invalid_returns_none() {
        assert!(parse_iso8601_secs("not-a-date").is_none());
        assert!(parse_iso8601_secs("").is_none());
    }
}
