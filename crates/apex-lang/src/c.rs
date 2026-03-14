use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::{info, warn};

pub struct CRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl CRunner {
    pub fn new() -> Self {
        CRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> CRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        CRunner { runner }
    }

    fn detect_build_system(target: &Path) -> BuildSystem {
        if target.join("CMakeLists.txt").exists() {
            BuildSystem::CMake
        } else if target.join("Makefile").exists()
            || target.join("makefile").exists()
            || target.join("GNUmakefile").exists()
        {
            BuildSystem::Make
        } else if target.join("configure").exists() || target.join("configure.ac").exists() {
            BuildSystem::Autoconf
        } else {
            BuildSystem::None
        }
    }
}

#[derive(Debug)]
enum BuildSystem {
    CMake,
    Make,
    Autoconf,
    None,
}

impl Default for CRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for CRunner<R> {
    fn language(&self) -> Language {
        Language::C
    }

    fn detect(&self, target: &Path) -> bool {
        // C project markers
        target.join("CMakeLists.txt").exists()
            || target.join("Makefile").exists()
            || target.join("configure.ac").exists()
            || target.join("configure").exists()
            || {
                // At least one .c file in root
                std::fs::read_dir(target)
                    .map(|d| {
                        d.filter_map(|e| e.ok())
                            .any(|e| e.path().extension().is_some_and(|x| x == "c"))
                    })
                    .unwrap_or(false)
            }
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        // C projects typically ship their own deps; nothing to install.
        // Run autogen/configure if present.
        if target.join("autogen.sh").exists() {
            info!("running autogen.sh");
            let spec = CommandSpec::new("sh", target).args(["autogen.sh"]);
            let out = self
                .runner
                .run_command(&spec)
                .await
                .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
            if out.exit_code != 0 {
                warn!(stderr = %String::from_utf8_lossy(&out.stderr), "autogen.sh failed");
            }
        }

        if target.join("configure").exists() && !target.join("Makefile").exists() {
            info!("running ./configure");
            let spec = CommandSpec::new("./configure", target);
            let out = self
                .runner
                .run_command(&spec)
                .await
                .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
            if out.exit_code != 0 {
                warn!(stderr = %String::from_utf8_lossy(&out.stderr), "configure failed");
            }
        }

        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        let start = Instant::now();

        let cmake_build_dir = target.join("build_apex");
        let (cmd, args): (&str, Vec<String>) = match Self::detect_build_system(target) {
            BuildSystem::CMake => {
                // Configure
                let cmake_build_dir_str = cmake_build_dir.to_string_lossy().into_owned();
                let configure_spec = CommandSpec::new("cmake", target).args([
                    "-B",
                    &cmake_build_dir_str,
                    "-DCMAKE_BUILD_TYPE=Debug",
                ]);
                self.runner
                    .run_command(&configure_spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("cmake configure: {e}")))?;

                // Build
                let build_spec =
                    CommandSpec::new("cmake", target).args(["--build", &cmake_build_dir_str]);
                self.runner
                    .run_command(&build_spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(format!("cmake build: {e}")))?;

                (
                    "ctest",
                    vec!["--test-dir".into(), cmake_build_dir_str, "-V".into()],
                )
            }
            BuildSystem::Make | BuildSystem::Autoconf => ("make", vec!["check".into()]),
            BuildSystem::None => {
                return Err(ApexError::LanguageRunner(
                    "no recognised build system (CMake/Make/Autoconf) found".into(),
                ));
            }
        };

        let mut full_args: Vec<String> = args;
        full_args.extend_from_slice(extra_args);

        let spec = CommandSpec::new(cmd, target).args(full_args);
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("run tests: {e}")))?;

        Ok(TestRunOutput {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

// ---------------------------------------------------------------------------
// Instrumented build helpers (used by apex-instrument LLVM path + fuzz CLI)
// ---------------------------------------------------------------------------

/// Recompile the target with SanitizerCoverage flags for C.
pub async fn build_with_coverage(target: &Path, output_binary: &Path) -> Result<()> {
    let compiler = if which("clang") { "clang" } else { "cc" };

    // Gather all .c files recursively (simple heuristic).
    let c_files: Vec<String> = walkdir_c_files(target);
    if c_files.is_empty() {
        return Err(ApexError::LanguageRunner("no .c files found".into()));
    }

    info!(
        files = c_files.len(),
        output = %output_binary.display(),
        "compiling C target with SanitizerCoverage"
    );

    let mut args = vec![
        "-fsanitize-coverage=trace-pc-guard".to_string(),
        "-g".to_string(),
        "-O1".to_string(),
        "-o".to_string(),
        output_binary.to_string_lossy().into_owned(),
    ];
    args.extend(c_files);

    let out = tokio::process::Command::new(compiler)
        .args(&args)
        .current_dir(target)
        .output()
        .await
        .map_err(|e| ApexError::LanguageRunner(format!("compile: {e}")))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(ApexError::LanguageRunner(format!(
            "compilation failed:\n{stderr}"
        )));
    }

    Ok(())
}

fn walkdir_c_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Recurse one level (avoid descending into build dirs)
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !matches!(name, "build" | "build_apex" | ".git" | "target") {
                    files.extend(walkdir_c_files(&path));
                }
            } else if path.extension().is_some_and(|e| e == "c") {
                files.push(path.to_string_lossy().to_string());
            }
        }
    }
    files
}

fn which(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;
    use apex_core::traits::LanguageRunner;

    mockall::mock! {
        Cmd {}
        #[async_trait]
        impl CommandRunner for Cmd {
            async fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput>;
        }
    }

    #[test]
    fn detect_cmake() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();
        assert!(CRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_makefile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();
        assert!(CRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_configure_ac() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure.ac"), "").unwrap();
        assert!(CRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_configure_script() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();
        assert!(CRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_c_file_in_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.c"), "int main() {}").unwrap();
        assert!(CRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_empty_dir_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!CRunner::new().detect(dir.path()));
    }

    #[test]
    fn build_system_cmake() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::CMake
        ));
    }

    #[test]
    fn build_system_make() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::Make
        ));
    }

    #[test]
    fn build_system_make_lowercase() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("makefile"), "").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::Make
        ));
    }

    #[test]
    fn build_system_gnu_makefile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("GNUmakefile"), "").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::Make
        ));
    }

    #[test]
    fn build_system_autoconf() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::Autoconf
        ));
    }

    #[test]
    fn build_system_autoconf_ac() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure.ac"), "").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::Autoconf
        ));
    }

    #[test]
    fn build_system_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::None
        ));
    }

    #[test]
    fn build_system_cmake_takes_priority() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::CMake
        ));
    }

    #[test]
    fn language_is_c() {
        assert_eq!(CRunner::new().language(), Language::C);
    }

    #[test]
    fn default_creates_runner() {
        let runner = CRunner::default();
        assert_eq!(runner.language(), Language::C);
    }

    #[test]
    fn detect_empty_dir_no_c_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("header.h"), "#ifndef H\n#endif").unwrap();
        assert!(!CRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_nonexistent_dir_returns_false() {
        let runner = CRunner::new();
        assert!(!runner.detect(Path::new("/nonexistent/path/that/does/not/exist")));
    }

    #[test]
    fn detect_multiple_markers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();
        std::fs::write(dir.path().join("main.c"), "int main(){}").unwrap();
        assert!(CRunner::new().detect(dir.path()));
    }

    #[test]
    fn build_system_make_takes_priority_over_autoconf() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::Make
        ));
    }

    #[test]
    fn build_system_none_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.c"), "int main(){}").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::None
        ));
    }

    // ------------------------------------------------------------------
    // walkdir_c_files
    // ------------------------------------------------------------------

    #[test]
    fn walkdir_c_files_finds_root_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.c"), "int main(){}").unwrap();
        std::fs::write(dir.path().join("util.c"), "void util(){}").unwrap();
        std::fs::write(dir.path().join("header.h"), "").unwrap();

        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.contains("main.c")));
        assert!(files.iter().any(|f| f.contains("util.c")));
    }

    #[test]
    fn walkdir_c_files_recurses_into_src() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("foo.c"), "").unwrap();

        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("foo.c"));
    }

    #[test]
    fn walkdir_c_files_skips_build_dirs() {
        let dir = tempfile::tempdir().unwrap();
        for skip_dir in &["build", "build_apex", ".git", "target"] {
            let d = dir.path().join(skip_dir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("hidden.c"), "").unwrap();
        }
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("visible.c"), "").unwrap();

        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("visible.c"));
    }

    #[test]
    fn walkdir_c_files_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let files = walkdir_c_files(dir.path());
        assert!(files.is_empty());
    }

    #[test]
    fn walkdir_c_files_nonexistent_dir() {
        let files = walkdir_c_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
    }

    // ------------------------------------------------------------------
    // which helper
    // ------------------------------------------------------------------

    #[test]
    fn which_finds_common_binary() {
        assert!(which("ls"));
    }

    #[test]
    fn which_returns_false_for_nonexistent() {
        assert!(!which("this_binary_does_not_exist_apex_test_sentinel"));
    }

    // ------------------------------------------------------------------
    // Additional walkdir tests
    // ------------------------------------------------------------------

    #[test]
    fn walkdir_c_files_skips_all_excluded_names() {
        for name in &["build", "build_apex", ".git", "target"] {
            let dir = tempfile::tempdir().unwrap();
            let excluded = dir.path().join(name);
            std::fs::create_dir_all(&excluded).unwrap();
            std::fs::write(excluded.join("secret.c"), "").unwrap();
            let files = walkdir_c_files(dir.path());
            assert!(
                files.is_empty(),
                "{name} should be excluded but files found: {files:?}"
            );
        }
    }

    #[test]
    fn walkdir_c_files_two_levels_deep() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let ab = a.join("b");
        std::fs::create_dir_all(&ab).unwrap();
        std::fs::write(ab.join("deep.c"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("deep.c"));
    }

    #[test]
    fn walkdir_c_files_ignores_non_c_extensions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.cpp"), "").unwrap();
        std::fs::write(dir.path().join("b.h"), "").unwrap();
        std::fs::write(dir.path().join("c.o"), "").unwrap();
        std::fs::write(dir.path().join("d.c"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("d.c"));
    }

    #[test]
    fn build_system_autoconf_configure_ac_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure.ac"), "AC_INIT").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::Autoconf
        ));
    }

    #[test]
    fn build_system_cmake_over_autoconf() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();
        std::fs::write(dir.path().join("configure.ac"), "").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::CMake
        ));
    }

    #[test]
    fn build_system_make_over_autoconf_configure_ac() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("GNUmakefile"), "").unwrap();
        std::fs::write(dir.path().join("configure.ac"), "").unwrap();
        assert!(matches!(
            CRunner::<RealCommandRunner>::detect_build_system(dir.path()),
            BuildSystem::Make
        ));
    }

    #[test]
    fn detect_only_header_files_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("foo.h"), "").unwrap();
        std::fs::write(dir.path().join("bar.hpp"), "").unwrap();
        assert!(!CRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_only_cpp_files_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.cpp"), "int main(){}").unwrap();
        std::fs::write(dir.path().join("util.cc"), "").unwrap();
        assert!(!CRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_c_file_plus_non_c_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "").unwrap();
        std::fs::write(dir.path().join("lib.py"), "").unwrap();
        std::fs::write(dir.path().join("helper.c"), "void f(){}").unwrap();
        assert!(CRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_does_not_scan_subdirs_for_c_files() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("src");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("deep.c"), "").unwrap();
        assert!(!CRunner::new().detect(dir.path()));
    }

    // ---- Mock-based tests ----

    #[tokio::test]
    async fn install_deps_plain_dir_noop() {
        let dir = tempfile::tempdir().unwrap();

        let mock = MockCmd::new();
        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_runs_autogen() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("autogen.sh"), "#!/bin/sh").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "sh" && spec.args.contains(&"autogen.sh".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_runs_configure_when_no_makefile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./configure")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"configure error".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        // configure failure is just a warning, still returns Ok
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_skips_configure_if_makefile_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();
        std::fs::write(dir.path().join("Makefile"), "all:\n\techo ok").unwrap();

        // No expectations -- configure should NOT be called
        let mock = MockCmd::new();
        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn run_tests_make_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "check:\n\techo ok").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "make" && spec.args.contains(&"check".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"All tests passed".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("All tests passed"));
    }

    #[tokio::test]
    async fn run_tests_cmake_runs_three_commands() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();

        let mut mock = MockCmd::new();
        // cmake configure
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"-B".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"configured".to_vec())));
        // cmake build
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"--build".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"built".to_vec())));
        // ctest
        mock.expect_run_command()
            .withf(|spec| spec.program == "ctest")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"100% tests passed".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("100% tests passed"));
    }

    #[tokio::test]
    async fn run_tests_no_build_system_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.c"), "int main(){}").unwrap();

        let mock = MockCmd::new();
        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("no recognised build system"),
            "unexpected: {msg}"
        );
    }

    #[tokio::test]
    async fn run_tests_command_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn make: not found".into(),
            })
        });

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_tests_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.iter().any(|a| a == "VERBOSE=1"))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["VERBOSE=1".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — Autoconf path
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_autoconf_runs_make_check() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure.ac"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "make" && spec.args.contains(&"check".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"PASS: all tests".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("PASS"));
    }

    #[tokio::test]
    async fn run_tests_autoconf_configure_script_runs_make_check() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "make" && spec.args.contains(&"check".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"FAIL: test_foo".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        // Non-zero exit is not an Err; it is surfaced through exit_code
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("FAIL"));
    }

    // ------------------------------------------------------------------
    // run_tests — make failure (nonzero exit is not Err)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_make_failure_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(2, b"test failed".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("test failed"));
    }

    // ------------------------------------------------------------------
    // run_tests — cmake configure error propagates as Err
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_cmake_configure_error_propagates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();

        let mut mock = MockCmd::new();
        // cmake configure fails with a command error
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"-B".to_string()))
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "cmake not found".into(),
                })
            });

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("cmake configure"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // run_tests — cmake build error propagates as Err
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_cmake_build_error_propagates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();

        let mut mock = MockCmd::new();
        // cmake configure succeeds
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"-B".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"configured".to_vec())));
        // cmake build fails with a command error
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"--build".to_string()))
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "build failed hard".into(),
                })
            });

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("cmake build"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // install_deps — autogen.sh command error propagates as Err
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_autogen_command_error_propagates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("autogen.sh"), "#!/bin/sh").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "sh" && spec.args.contains(&"autogen.sh".to_string()))
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "sh not found".into(),
                })
            });

        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // install_deps — autogen succeeds then configure runs (no Makefile yet)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_autogen_then_configure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("autogen.sh"), "#!/bin/sh").unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();
        // No Makefile — configure should run

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "sh" && spec.args.contains(&"autogen.sh".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "./configure")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // install_deps — configure command error propagates as Err
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_configure_command_error_propagates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();
        // No Makefile — configure should be attempted

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./configure")
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "configure spawn failed".into(),
                })
            });

        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // run_tests — ctest command error propagates as Err
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_ctest_command_error_propagates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"-B".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"--build".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "ctest")
            .times(1)
            .returning(|_| {
                Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "ctest not found".into(),
                })
            });

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("run tests"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // run_tests — autoconf make command error propagates
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_autoconf_make_error_propagates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure.ac"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "make not found".into(),
            })
        });

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("run tests"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // run_tests — duration is populated
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_duration_populated() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        // duration_ms is a u64; any value is valid (just shouldn't panic)
        let _ = result.duration_ms;
    }

    // ------------------------------------------------------------------
    // run_tests — GNUmakefile triggers Make path
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_gnu_makefile_uses_make() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("GNUmakefile"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "make" && spec.args.contains(&"check".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"done".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — lowercase makefile triggers Make path
    // ------------------------------------------------------------------

    #[test]
    fn walkdir_c_files_ignores_files_without_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "all:").unwrap();
        std::fs::write(dir.path().join("LICENSE"), "MIT").unwrap();
        std::fs::write(dir.path().join("README"), "hello").unwrap();
        std::fs::write(dir.path().join("real.c"), "int x;").unwrap();
        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("real.c"));
    }

    #[test]
    fn walkdir_c_files_handles_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".hidden.c"), "").unwrap();
        std::fs::write(dir.path().join("visible.c"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        // .hidden.c is a regular file with .c extension, should be found
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn walkdir_c_files_does_not_follow_into_allowed_subdirs() {
        // Verify that non-excluded subdirs are recursed into
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("helper.c"), "").unwrap();
        let include = dir.path().join("include");
        std::fs::create_dir_all(&include).unwrap();
        std::fs::write(include.join("header.h"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("helper.c"));
    }

    #[test]
    fn which_empty_string_returns_false() {
        assert!(!which(""));
    }

    #[test]
    fn walkdir_c_files_many_files() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..20 {
            std::fs::write(dir.path().join(format!("file_{i}.c")), "").unwrap();
        }
        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 20);
    }

    #[test]
    fn detect_build_system_nonexistent_path() {
        let result = CRunner::<RealCommandRunner>::detect_build_system(Path::new(
            "/nonexistent/path/that/does/not/exist",
        ));
        assert!(matches!(result, BuildSystem::None));
    }

    #[tokio::test]
    async fn run_tests_lowercase_makefile_uses_make() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("makefile"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "make" && spec.args.contains(&"check".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"done".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // install_deps — autogen nonzero exit is a warning, not an error
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_autogen_nonzero_exit_still_ok() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("autogen.sh"), "#!/bin/sh\nexit 1").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "sh" && spec.args.contains(&"autogen.sh".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"autogen error".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(
            result.is_ok(),
            "nonzero autogen exit should be a warning, not an error"
        );
    }

    // ------------------------------------------------------------------
    // install_deps — configure nonzero exit is a warning, not an error
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_configure_nonzero_exit_still_ok() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh\nexit 1").unwrap();
        // No Makefile so configure runs

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./configure")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"configure warning".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(
            result.is_ok(),
            "nonzero configure exit should be a warning, not an error"
        );
    }

    // ------------------------------------------------------------------
    // run_tests — cmake with extra args
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_cmake_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"-B".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"--build".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "ctest" && spec.args.iter().any(|a| a == "VERBOSE=1"))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"all pass".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["VERBOSE=1".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // install_deps — autogen nonzero then configure runs
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_autogen_nonzero_then_configure_still_runs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("autogen.sh"), "#!/bin/sh").unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();

        let mut mock = MockCmd::new();
        // autogen fails with nonzero
        mock.expect_run_command()
            .withf(|spec| spec.program == "sh")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"autogen warn".to_vec())));
        // configure still runs
        mock.expect_run_command()
            .withf(|spec| spec.program == "./configure")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // with_runner constructor
    // ------------------------------------------------------------------

    #[test]
    fn with_runner_creates_runner() {
        let mock = MockCmd::new();
        let runner = CRunner::with_runner(mock);
        assert_eq!(runner.language(), Language::C);
    }

    // ------------------------------------------------------------------
    // walkdir_c_files — path with file_name() returning None (edge case)
    // ------------------------------------------------------------------

    #[test]
    fn walkdir_c_files_ignores_entries_without_extension_mixed() {
        let dir = tempfile::tempdir().unwrap();
        // Mix of files with no extension, other extensions, and .c
        std::fs::write(dir.path().join("no_ext"), "").unwrap();
        std::fs::write(dir.path().join("script.sh"), "").unwrap();
        std::fs::write(dir.path().join("prog.c"), "int main(){}").unwrap();
        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("prog.c"));
    }

    // ------------------------------------------------------------------
    // install_deps — setup with both autogen.sh AND configure AND Makefile
    // (configure should be skipped because Makefile exists)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_autogen_runs_but_configure_skipped_because_makefile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("autogen.sh"), "#!/bin/sh").unwrap();
        std::fs::write(dir.path().join("configure"), "#!/bin/sh").unwrap();
        std::fs::write(dir.path().join("Makefile"), "all:").unwrap();

        let mut mock = MockCmd::new();
        // autogen.sh runs
        mock.expect_run_command()
            .withf(|spec| spec.program == "sh" && spec.args.contains(&"autogen.sh".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"".to_vec())));
        // configure should NOT run because Makefile exists

        let runner = CRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // run_tests — ctest receives -V flag
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_cmake_ctest_has_verbose_flag() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"-B".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"--build".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "ctest" && spec.args.contains(&"-V".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"all pass".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — cmake nonzero exit from ctest (not an Err)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_ctest_nonzero_exit_not_err() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"-B".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"--build".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "ctest")
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(8, b"8 tests failed".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 8);
    }

    // ------------------------------------------------------------------
    // build_with_coverage — no .c files returns error
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn build_with_coverage_no_c_files_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("output_bin");
        let result = build_with_coverage(dir.path(), &out).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("no .c files found"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn build_with_coverage_no_c_files_only_headers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.h"), "#pragma once").unwrap();
        std::fs::write(dir.path().join("util.hpp"), "").unwrap();
        let out = dir.path().join("output_bin");
        let result = build_with_coverage(dir.path(), &out).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("no .c files found"), "unexpected: {msg}");
    }

    #[tokio::test]
    async fn build_with_coverage_no_c_files_only_cpp() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.cpp"), "int main(){}").unwrap();
        let out = dir.path().join("output_bin");
        let result = build_with_coverage(dir.path(), &out).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn build_with_coverage_nonexistent_dir() {
        let out = Path::new("/tmp/apex_test_nonexistent_output");
        let result =
            build_with_coverage(Path::new("/nonexistent/dir/that/does/not/exist"), &out).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("no .c files found"), "unexpected: {msg}");
    }

    // ------------------------------------------------------------------
    // build_with_coverage — with .c files (compiler invocation)
    // This tests that the function gets past the walkdir check
    // and attempts to invoke the compiler. On CI without clang/cc
    // this may fail at compile step, which is fine — we test the error path.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn build_with_coverage_with_c_files_attempts_compile() {
        let dir = tempfile::tempdir().unwrap();
        // Write a trivially invalid C file so compilation fails
        std::fs::write(dir.path().join("bad.c"), "THIS IS NOT VALID C CODE !!!").unwrap();
        let out = dir.path().join("output_bin");
        let result = build_with_coverage(dir.path(), &out).await;
        // Either compilation fails (expected) or somehow succeeds — both are OK
        // We primarily care that we got past the "no .c files" check
        if let Err(e) = &result {
            let msg = format!("{e}");
            // Should NOT be "no .c files found" — should be a compilation error
            assert!(
                !msg.contains("no .c files found"),
                "should have found .c files"
            );
        }
    }

    #[tokio::test]
    async fn build_with_coverage_with_valid_c_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.c"), "int main() { return 0; }").unwrap();
        let out = dir.path().join("output_bin");
        let result = build_with_coverage(dir.path(), &out).await;
        // On systems with cc/clang this may succeed; on others it may fail.
        // We just verify it doesn't return "no .c files found".
        if let Err(e) = &result {
            let msg = format!("{e}");
            assert!(
                !msg.contains("no .c files found"),
                "should have found .c files"
            );
        }
    }

    #[tokio::test]
    async fn build_with_coverage_c_files_in_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.c"), "void f() {}").unwrap();
        let out = dir.path().join("output_bin");
        let result = build_with_coverage(dir.path(), &out).await;
        if let Err(e) = &result {
            let msg = format!("{e}");
            assert!(
                !msg.contains("no .c files found"),
                "should have found .c files in subdir"
            );
        }
    }

    // ------------------------------------------------------------------
    // which — additional edge cases
    // ------------------------------------------------------------------

    #[test]
    fn which_finds_sh() {
        // sh should exist on all unix systems
        assert!(which("sh"));
    }

    #[test]
    fn which_special_characters_returns_false() {
        assert!(!which("no-such-binary-!@#$%"));
    }

    // ------------------------------------------------------------------
    // walkdir_c_files — deeply nested allowed directories
    // ------------------------------------------------------------------

    #[test]
    fn walkdir_c_files_three_levels_deep() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("deep.c"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].contains("deep.c"));
    }

    #[test]
    fn walkdir_c_files_mixed_skipped_and_allowed_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        // Create allowed subdir with .c file
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("good.c"), "").unwrap();
        // Create excluded subdir with .c file
        let build = dir.path().join("build");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("generated.c"), "").unwrap();
        // Root .c file
        std::fs::write(dir.path().join("main.c"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.contains("good.c")));
        assert!(files.iter().any(|f| f.contains("main.c")));
        assert!(!files.iter().any(|f| f.contains("generated.c")));
    }

    #[test]
    fn walkdir_c_files_build_apex_inside_allowed_subdir() {
        // build_apex nested inside an allowed subdir should still be skipped
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("src").join("build_apex");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("hidden.c"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        assert!(files.is_empty());
    }

    #[test]
    fn walkdir_c_files_git_inside_allowed_subdir_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("lib").join(".git");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("hooks.c"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        assert!(files.is_empty());
    }

    #[test]
    fn walkdir_c_files_target_inside_allowed_subdir_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("vendor").join("target");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("gen.c"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        assert!(files.is_empty());
    }

    // ------------------------------------------------------------------
    // run_tests — stderr is captured from command output
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_make_captures_stderr() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Ok(CommandOutput {
                exit_code: 1,
                stdout: b"some output".to_vec(),
                stderr: b"warning: something".to_vec(),
            })
        });

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "some output");
        assert_eq!(result.stderr, "warning: something");
    }

    // ------------------------------------------------------------------
    // run_tests — multiple extra args are passed through
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_make_multiple_extra_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "make"
                    && spec.args.contains(&"check".to_string())
                    && spec.args.contains(&"VERBOSE=1".to_string())
                    && spec.args.contains(&"--jobs=4".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["VERBOSE=1".into(), "--jobs=4".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — cmake ctest receives --test-dir with correct path
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_cmake_ctest_receives_test_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();
        let expected_build_dir = dir.path().join("build_apex").to_string_lossy().to_string();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"-B".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"--build".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let build_dir_clone = expected_build_dir.clone();
        mock.expect_run_command()
            .withf(move |spec| {
                spec.program == "ctest"
                    && spec.args.contains(&"--test-dir".to_string())
                    && spec.args.contains(&build_dir_clone)
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"pass".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — cmake configure receives -DCMAKE_BUILD_TYPE=Debug
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_cmake_configure_has_debug_flag() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| {
                spec.program == "cmake"
                    && spec
                        .args
                        .contains(&"-DCMAKE_BUILD_TYPE=Debug".to_string())
            })
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"--build".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "ctest")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // BuildSystem Debug formatting
    // ------------------------------------------------------------------

    #[test]
    fn build_system_debug_format() {
        assert_eq!(format!("{:?}", BuildSystem::CMake), "CMake");
        assert_eq!(format!("{:?}", BuildSystem::Make), "Make");
        assert_eq!(format!("{:?}", BuildSystem::Autoconf), "Autoconf");
        assert_eq!(format!("{:?}", BuildSystem::None), "None");
    }

    // ------------------------------------------------------------------
    // install_deps — autogen success with stdout
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn install_deps_autogen_success_with_output() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("autogen.sh"), "#!/bin/sh\necho gen").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "sh")
            .times(1)
            .returning(|_| {
                Ok(CommandOutput {
                    exit_code: 0,
                    stdout: b"generating...".to_vec(),
                    stderr: Vec::new(),
                })
            });

        let runner = CRunner::with_runner(mock);
        assert!(runner.install_deps(dir.path()).await.is_ok());
    }

    // ------------------------------------------------------------------
    // run_tests — make with empty extra_args
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_make_empty_extra_args_only_check() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Makefile"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "make" && spec.args == vec!["check".to_string()])
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ------------------------------------------------------------------
    // run_tests — output contains both stdout and stderr from command
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_tests_cmake_output_contains_both_streams() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"-B".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "cmake" && spec.args.contains(&"--build".to_string()))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));
        mock.expect_run_command()
            .withf(|spec| spec.program == "ctest")
            .times(1)
            .returning(|_| {
                Ok(CommandOutput {
                    exit_code: 0,
                    stdout: b"test output here".to_vec(),
                    stderr: b"test warnings here".to_vec(),
                })
            });

        let runner = CRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.stdout, "test output here");
        assert_eq!(result.stderr, "test warnings here");
    }

    // ------------------------------------------------------------------
    // detect — verify short-circuit: CMakeLists.txt checked first
    // ------------------------------------------------------------------

    #[test]
    fn detect_cmake_without_c_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "cmake_minimum_required(VERSION 3.0)").unwrap();
        // No .c files at all - should still detect because CMakeLists.txt present
        assert!(CRunner::new().detect(dir.path()));
    }

    // ------------------------------------------------------------------
    // walkdir_c_files — only non-c files in subdirs
    // ------------------------------------------------------------------

    #[test]
    fn walkdir_c_files_subdir_with_only_non_c_files() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("docs");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("readme.md"), "").unwrap();
        std::fs::write(sub.join("notes.txt"), "").unwrap();
        let files = walkdir_c_files(dir.path());
        assert!(files.is_empty());
    }

    // ------------------------------------------------------------------
    // walkdir_c_files — empty subdir
    // ------------------------------------------------------------------

    #[test]
    fn walkdir_c_files_empty_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("empty_src");
        std::fs::create_dir_all(&sub).unwrap();
        let files = walkdir_c_files(dir.path());
        assert!(files.is_empty());
    }

    // ------------------------------------------------------------------
    // walkdir_c_files — multiple subdirs some excluded some not
    // ------------------------------------------------------------------

    #[test]
    fn walkdir_c_files_multiple_parallel_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        for name in &["src", "lib", "test", "examples"] {
            let sub = dir.path().join(name);
            std::fs::create_dir_all(&sub).unwrap();
            std::fs::write(sub.join(format!("{name}.c")), "").unwrap();
        }
        let files = walkdir_c_files(dir.path());
        assert_eq!(files.len(), 4);
    }
}
