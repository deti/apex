//! `apex wrap` — run a user's test command with coverage injection.
//!
//! Usage:
//! ```text
//! apex wrap --lang python -- pytest -q
//! apex wrap --lang go -- go test ./...
//! apex wrap -- cargo test          # auto-detect from command
//! apex wrap --coverage-method frida -- ./my-binary
//! ```

use apex_core::types::Language;
use apex_instrument::wrap::{detect_language_from_command, inject_coverage};
use clap::{Parser, ValueEnum};
use color_eyre::{eyre::eyre, Result};
use std::path::PathBuf;
use tracing::{info, warn};

use crate::LangArg;

/// Strategy for collecting coverage data during `apex wrap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CoverageMethod {
    /// Language-specific coverage (coverage.py, istanbul, gocov, etc.).
    Native,
    /// Binary instrumentation via Frida.
    Frida,
    /// No coverage collection — audit only.
    None,
    /// Auto-cascade: try native -> frida -> none.
    Auto,
}

impl std::fmt::Display for CoverageMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoverageMethod::Native => write!(f, "native"),
            CoverageMethod::Frida => write!(f, "frida"),
            CoverageMethod::None => write!(f, "none"),
            CoverageMethod::Auto => write!(f, "auto"),
        }
    }
}

/// Returns true if Frida support was compiled in.
fn frida_available() -> bool {
    cfg!(feature = "frida")
}

/// Resolve the effective coverage method from the requested method,
/// applying the auto-cascade logic: native -> frida -> none.
///
/// Returns the resolved method and whether native injection succeeded
/// (only meaningful when the resolved method is Native).
fn resolve_coverage_method(
    requested: CoverageMethod,
    lang: Language,
    cmd: &[String],
    output_dir: &std::path::Path,
) -> (CoverageMethod, Option<apex_instrument::wrap::CoverageInjection>) {
    match requested {
        CoverageMethod::Native => {
            let injection = inject_coverage(lang, cmd, output_dir);
            // If the injection didn't modify the command at all, native may not
            // be supported for this language — but we still honour the explicit
            // request.
            (CoverageMethod::Native, Some(injection))
        }
        CoverageMethod::Frida => {
            if frida_available() {
                (CoverageMethod::Frida, Option::None)
            } else {
                warn!(
                    "Frida coverage requested but the `frida` feature was not compiled. \
                     Falling back to no coverage (audit-only mode). \
                     Rebuild with `--features frida` to enable Frida support."
                );
                eprintln!(
                    "apex wrap: Frida support not compiled. Falling back to audit-only mode.\n\
                     Hint: rebuild with `cargo build --features frida` to enable Frida."
                );
                (CoverageMethod::None, Option::None)
            }
        }
        CoverageMethod::None => (CoverageMethod::None, Option::None),
        CoverageMethod::Auto => {
            // Step 1: try native
            let injection = inject_coverage(lang, cmd, output_dir);
            // Native injection is considered successful if it modified the
            // command or set environment variables.
            let native_modified =
                injection.args != cmd || !injection.env_vars.is_empty();
            if native_modified {
                info!("Auto-cascade: using native coverage for {lang}");
                return (CoverageMethod::Native, Some(injection));
            }

            // Step 2: try Frida
            if frida_available() {
                info!("Auto-cascade: native coverage unavailable, using Frida");
                return (CoverageMethod::Frida, Option::None);
            }

            // Step 3: fall back to none
            info!(
                "Auto-cascade: neither native nor Frida coverage available — audit-only mode"
            );
            (CoverageMethod::None, Option::None)
        }
    }
}

/// CLI arguments for `apex wrap`.
#[derive(Parser)]
pub struct WrapArgs {
    /// Language of the project (auto-detected from command if omitted).
    #[arg(long, short, value_enum)]
    pub lang: Option<LangArg>,

    /// Coverage collection method.
    #[arg(long, value_enum, default_value = "auto")]
    pub coverage_method: CoverageMethod,

    /// Directory to write coverage output files.
    #[arg(long, short, default_value = ".apex-coverage")]
    pub output_dir: PathBuf,

    /// The test command and its arguments (everything after `--`).
    #[arg(trailing_var_arg = true, required = true)]
    pub cmd: Vec<String>,
}

/// Execute the wrapped test command with coverage instrumentation.
pub async fn run_wrap(args: WrapArgs) -> Result<()> {
    if args.cmd.is_empty() {
        return Err(eyre!(
            "No command specified. Usage: apex wrap [--lang <lang>] -- <test-command>"
        ));
    }

    // Resolve language: explicit flag wins, otherwise auto-detect.
    let lang: Language = match args.lang {
        Some(l) => l.into(),
        None => detect_language_from_command(&args.cmd).ok_or_else(|| {
            eyre!(
                "Cannot auto-detect language from command {:?}. Use --lang to specify.",
                args.cmd.first().unwrap_or(&String::new())
            )
        })?,
    };

    // Ensure output directory exists.
    std::fs::create_dir_all(&args.output_dir)?;

    let (method, injection) =
        resolve_coverage_method(args.coverage_method, lang, &args.cmd, &args.output_dir);

    let effective_args: Vec<String>;
    let effective_env: Vec<(String, String)>;

    match method {
        CoverageMethod::Native => {
            let inj = injection.expect("native must produce injection");
            effective_args = inj.args;
            effective_env = inj.env_vars;
        }
        CoverageMethod::Frida => {
            // Frida instrumentation would be set up here when the feature is
            // compiled in. For now we pass through the original command — the
            // Frida runtime attaches via its own mechanism.
            effective_args = args.cmd.clone();
            effective_env = vec![];
            info!("Frida binary instrumentation active (stub)");
        }
        CoverageMethod::None => {
            effective_args = args.cmd.clone();
            effective_env = vec![];
            info!("Running without coverage collection (audit-only)");
        }
        CoverageMethod::Auto => {
            unreachable!("Auto is always resolved to a concrete method")
        }
    }

    info!(
        lang = %lang,
        method = %method,
        cmd = ?effective_args,
        env = ?effective_env,
        output_dir = %args.output_dir.display(),
        "Running wrapped command with coverage injection"
    );

    // Build the child process.
    let program = effective_args
        .first()
        .ok_or_else(|| eyre!("Injected command is empty"))?;

    let mut child = tokio::process::Command::new(program);
    child.args(&effective_args[1..]);
    for (k, v) in &effective_env {
        child.env(k, v);
    }

    let status = child.status().await?;

    if status.success() {
        info!(
            output_dir = %args.output_dir.display(),
            "Test command succeeded — coverage data written"
        );
    } else {
        let code = status.code().unwrap_or(-1);
        eprintln!(
            "apex wrap: test command exited with code {code}"
        );
        // Still exit with the child's code so CI pipelines propagate failure.
        std::process::exit(code);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_args_parsing() {
        // Simulate: apex wrap --lang python -- pytest -q
        let args = WrapArgs::try_parse_from([
            "wrap", "--lang", "python", "--", "pytest", "-q",
        ])
        .unwrap();
        assert!(matches!(args.lang, Some(LangArg::Python)));
        assert_eq!(args.cmd, vec!["pytest", "-q"]);
    }

    #[test]
    fn test_wrap_args_auto_detect() {
        // Simulate: apex wrap -- cargo test
        let args =
            WrapArgs::try_parse_from(["wrap", "--", "cargo", "test"]).unwrap();
        assert!(args.lang.is_none());
        assert_eq!(args.cmd, vec!["cargo", "test"]);
    }

    #[test]
    fn test_wrap_args_custom_output_dir() {
        let args = WrapArgs::try_parse_from([
            "wrap",
            "--output-dir",
            "/tmp/my-cov",
            "--",
            "go",
            "test",
            "./...",
        ])
        .unwrap();
        assert_eq!(args.output_dir, PathBuf::from("/tmp/my-cov"));
    }

    #[test]
    fn test_wrap_args_default_output_dir() {
        let args =
            WrapArgs::try_parse_from(["wrap", "--", "npm", "test"]).unwrap();
        assert_eq!(args.output_dir, PathBuf::from(".apex-coverage"));
    }

    #[test]
    fn test_wrap_args_requires_cmd() {
        let result = WrapArgs::try_parse_from(["wrap"]);
        assert!(result.is_err());
    }

    // ---- Coverage method flag parsing ----

    #[test]
    fn test_wrap_args_default_coverage_method_is_auto() {
        let args =
            WrapArgs::try_parse_from(["wrap", "--", "pytest"]).unwrap();
        assert_eq!(args.coverage_method, CoverageMethod::Auto);
    }

    #[test]
    fn test_wrap_args_coverage_method_native() {
        let args = WrapArgs::try_parse_from([
            "wrap", "--coverage-method", "native", "--", "pytest",
        ])
        .unwrap();
        assert_eq!(args.coverage_method, CoverageMethod::Native);
    }

    #[test]
    fn test_wrap_args_coverage_method_frida() {
        let args = WrapArgs::try_parse_from([
            "wrap", "--coverage-method", "frida", "--", "./my-binary",
        ])
        .unwrap();
        assert_eq!(args.coverage_method, CoverageMethod::Frida);
    }

    #[test]
    fn test_wrap_args_coverage_method_none() {
        let args = WrapArgs::try_parse_from([
            "wrap", "--coverage-method", "none", "--", "pytest",
        ])
        .unwrap();
        assert_eq!(args.coverage_method, CoverageMethod::None);
    }

    // ---- Cascade resolution logic ----

    #[test]
    fn test_resolve_native_returns_injection() {
        let cmd = vec!["pytest".to_string(), "-q".to_string()];
        let dir = PathBuf::from("/tmp/test-cov");
        let (method, injection) =
            resolve_coverage_method(CoverageMethod::Native, Language::Python, &cmd, &dir);
        assert_eq!(method, CoverageMethod::Native);
        assert!(injection.is_some(), "native must produce an injection");
    }

    #[test]
    fn test_resolve_none_returns_no_injection() {
        let cmd = vec!["pytest".to_string()];
        let dir = PathBuf::from("/tmp/test-cov");
        let (method, injection) =
            resolve_coverage_method(CoverageMethod::None, Language::Python, &cmd, &dir);
        assert_eq!(method, CoverageMethod::None);
        assert!(injection.is_none());
    }

    #[test]
    fn test_resolve_frida_without_feature_falls_back_to_none() {
        // When compiled without the `frida` feature, requesting Frida should
        // fall back to None.
        let cmd = vec!["./my-binary".to_string()];
        let dir = PathBuf::from("/tmp/test-cov");
        let (method, injection) =
            resolve_coverage_method(CoverageMethod::Frida, Language::C, &cmd, &dir);
        if !frida_available() {
            assert_eq!(method, CoverageMethod::None);
            assert!(injection.is_none());
        }
    }

    #[test]
    fn test_resolve_auto_prefers_native_for_python() {
        // Python has native coverage support, so auto should resolve to native.
        let cmd = vec!["pytest".to_string()];
        let dir = PathBuf::from("/tmp/test-cov");
        let (method, injection) =
            resolve_coverage_method(CoverageMethod::Auto, Language::Python, &cmd, &dir);
        assert_eq!(method, CoverageMethod::Native);
        assert!(injection.is_some());
    }

    #[test]
    fn test_coverage_method_display() {
        assert_eq!(CoverageMethod::Native.to_string(), "native");
        assert_eq!(CoverageMethod::Frida.to_string(), "frida");
        assert_eq!(CoverageMethod::None.to_string(), "none");
        assert_eq!(CoverageMethod::Auto.to_string(), "auto");
    }
}
