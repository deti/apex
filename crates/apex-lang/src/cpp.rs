use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::{info, warn};

pub struct CppRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl CppRunner {
    pub fn new() -> Self {
        CppRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> CppRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        CppRunner { runner }
    }

    fn detect_build_system(target: &Path) -> CppBuildSystem {
        if target.join("xmake.lua").exists() {
            CppBuildSystem::Xmake
        } else if target.join("CMakeLists.txt").exists() {
            CppBuildSystem::CMake
        } else if target.join("Makefile").exists()
            || target.join("makefile").exists()
            || target.join("GNUmakefile").exists()
        {
            CppBuildSystem::Make
        } else if target.join("meson.build").exists() {
            CppBuildSystem::Meson
        } else {
            CppBuildSystem::None
        }
    }

    fn has_cpp_sources(target: &Path) -> bool {
        std::fs::read_dir(target)
            .map(|d| {
                d.filter_map(|e| e.ok()).any(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|x| x == "cpp" || x == "cxx" || x == "cc")
                })
            })
            .unwrap_or(false)
    }

    fn has_googletest(target: &Path) -> bool {
        // Check CMakeLists.txt for GTest references
        if let Ok(content) = std::fs::read_to_string(target.join("CMakeLists.txt")) {
            let lower = content.to_lowercase();
            return lower.contains("gtest") || lower.contains("googletest");
        }
        false
    }
}

#[derive(Debug)]
enum CppBuildSystem {
    Xmake,
    CMake,
    Make,
    Meson,
    None,
}

impl Default for CppRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for CppRunner<R> {
    fn language(&self) -> Language {
        Language::Cpp
    }

    fn detect(&self, target: &Path) -> bool {
        // C++ project: CMakeLists.txt with C++ sources, or Makefile with C++ sources
        let has_build = target.join("xmake.lua").exists()
            || target.join("CMakeLists.txt").exists()
            || target.join("Makefile").exists()
            || target.join("meson.build").exists();

        has_build && Self::has_cpp_sources(target) || Self::has_cpp_sources(target)
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        match Self::detect_build_system(target) {
            CppBuildSystem::Xmake => {
                info!("running xmake build");
                let spec = CommandSpec::new("xmake", target).args(["build"]);
                let out = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
                if out.exit_code != 0 {
                    warn!(stderr = %String::from_utf8_lossy(&out.stderr), "xmake build failed");
                }
            }
            CppBuildSystem::CMake => {
                info!("running cmake -B build");
                let spec = CommandSpec::new("cmake", target).args(["-B", "build"]);
                let out = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
                if out.exit_code != 0 {
                    warn!(stderr = %String::from_utf8_lossy(&out.stderr), "cmake configure failed");
                }

                info!("running cmake --build build");
                let spec = CommandSpec::new("cmake", target).args(["--build", "build"]);
                let out = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
                if out.exit_code != 0 {
                    warn!(stderr = %String::from_utf8_lossy(&out.stderr), "cmake build failed");
                }
            }
            CppBuildSystem::Make => {
                info!("running make");
                let spec = CommandSpec::new("make", target);
                let out = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
                if out.exit_code != 0 {
                    warn!(stderr = %String::from_utf8_lossy(&out.stderr), "make failed");
                }
            }
            CppBuildSystem::Meson => {
                info!("running meson setup build");
                let spec = CommandSpec::new("meson", target).args(["setup", "build"]);
                let out = self
                    .runner
                    .run_command(&spec)
                    .await
                    .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;
                if out.exit_code != 0 {
                    warn!(stderr = %String::from_utf8_lossy(&out.stderr), "meson setup failed");
                }
            }
            CppBuildSystem::None => {
                info!("no build system detected for C++ project");
            }
        }
        Ok(())
    }

    async fn run_tests(&self, target: &Path, _extra_args: &[String]) -> Result<TestRunOutput> {
        let start = Instant::now();

        let (spec, test_framework) = match Self::detect_build_system(target) {
            CppBuildSystem::Xmake => (CommandSpec::new("xmake", target).args(["test"]), "xmake-test"),
            CppBuildSystem::CMake => {
                if Self::has_googletest(target) {
                    info!("detected GoogleTest; running ctest");
                }
                (
                    CommandSpec::new("ctest", target).args([
                        "--test-dir",
                        "build",
                        "--output-on-failure",
                    ]),
                    "ctest",
                )
            }
            CppBuildSystem::Make => (CommandSpec::new("make", target).args(["test"]), "make-test"),
            CppBuildSystem::Meson => (
                CommandSpec::new("meson", target).args(["test", "-C", "build"]),
                "meson-test",
            ),
            CppBuildSystem::None => {
                return Ok(TestRunOutput {
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: "no build system detected".into(),
                    duration_ms: 0,
                });
            }
        };

        info!(framework = test_framework, "running C++ tests");
        let out = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(e.to_string()))?;

        let elapsed = start.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        Ok(TestRunOutput {
            exit_code: out.exit_code,
            stdout,
            stderr,
            duration_ms: elapsed,
        })
    }
}

/// Parse ctest summary output for test pass/fail counts.
/// Used in tests and may be used for reporting in future.
#[allow(dead_code)]
fn parse_ctest_summary(output: &str) -> (usize, usize) {
    // ctest output: "100% tests passed, 0 tests failed out of 5"
    let mut passed = 0;
    let mut failed = 0;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("tests passed") && trimmed.contains("tests failed") {
            // "100% tests passed, 0 tests failed out of 5"
            if let Some(f_str) = trimmed.split("tests failed").next() {
                // Get last comma-separated segment: " 0 "
                let segments: Vec<&str> = f_str.split(',').collect();
                if let Some(last_seg) = segments.last() {
                    if let Some(num) = last_seg
                        .split_whitespace()
                        .next()
                        .and_then(|n| n.parse::<usize>().ok())
                    {
                        failed = num;
                    }
                }
            }
            // Get total from "out of N"
            if let Some(idx) = trimmed.rfind("out of") {
                let after = trimmed[idx + 6..].trim();
                if let Ok(total) = after.parse::<usize>() {
                    passed = total.saturating_sub(failed);
                }
            }
        }
    }
    (passed, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpp_runner_language() {
        let runner = CppRunner::new();
        assert_eq!(runner.language(), Language::Cpp);
    }

    #[test]
    fn cpp_runner_default() {
        let runner = CppRunner::default();
        assert_eq!(runner.language(), Language::Cpp);
    }

    #[test]
    fn detect_no_cpp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = CppRunner::new();
        assert!(!runner.detect(tmp.path()));
    }

    #[test]
    fn detect_cmake_with_cpp_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("CMakeLists.txt"), "project(test)").unwrap();
        std::fs::write(tmp.path().join("main.cpp"), "int main() {}").unwrap();
        let runner = CppRunner::new();
        assert!(runner.detect(tmp.path()));
    }

    #[test]
    fn detect_makefile_with_cxx_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Makefile"), "all:").unwrap();
        std::fs::write(tmp.path().join("main.cxx"), "int main() {}").unwrap();
        let runner = CppRunner::new();
        assert!(runner.detect(tmp.path()));
    }

    #[test]
    fn detect_standalone_cc_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.cc"), "int main() {}").unwrap();
        let runner = CppRunner::new();
        assert!(runner.detect(tmp.path()));
    }

    #[test]
    fn detect_cmake_without_cpp_is_false() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("CMakeLists.txt"), "project(test)").unwrap();
        // Only .c files, no .cpp
        std::fs::write(tmp.path().join("main.c"), "int main() {}").unwrap();
        let runner = CppRunner::new();
        assert!(!runner.detect(tmp.path()));
    }

    #[test]
    fn parse_ctest_summary_typical() {
        let output = "100% tests passed, 0 tests failed out of 5\n";
        let (passed, failed) = parse_ctest_summary(output);
        assert_eq!(passed, 5);
        assert_eq!(failed, 0);
    }

    #[test]
    fn parse_ctest_summary_failures() {
        let output = "80% tests passed, 1 tests failed out of 5\n";
        let (passed, failed) = parse_ctest_summary(output);
        assert_eq!(passed, 4);
        assert_eq!(failed, 1);
    }

    #[test]
    fn parse_ctest_summary_empty() {
        let (passed, failed) = parse_ctest_summary("");
        assert_eq!(passed, 0);
        assert_eq!(failed, 0);
    }

    #[test]
    fn has_googletest_detection() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("CMakeLists.txt"),
            "find_package(GTest REQUIRED)\nadd_executable(test_main test.cpp)",
        )
        .unwrap();
        assert!(CppRunner::<RealCommandRunner>::has_googletest(tmp.path()));
    }

    #[test]
    fn detect_build_system_xmake() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("xmake.lua"), "target('hello')\n").unwrap();
        assert!(matches!(
            CppRunner::<RealCommandRunner>::detect_build_system(tmp.path()),
            CppBuildSystem::Xmake
        ));
    }

    #[test]
    fn detect_build_system_cmake_when_no_xmake() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("CMakeLists.txt"), "project(test)").unwrap();
        assert!(matches!(
            CppRunner::<RealCommandRunner>::detect_build_system(tmp.path()),
            CppBuildSystem::CMake
        ));
    }

    #[test]
    fn xmake_build_command() {
        // Verify xmake.lua produces the Xmake build-system variant.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("xmake.lua"), "target('hello')\n").unwrap();
        let build_sys = CppRunner::<RealCommandRunner>::detect_build_system(tmp.path());
        assert!(matches!(build_sys, CppBuildSystem::Xmake));
    }

    #[test]
    fn xmake_takes_priority_over_cmake_cpp() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("xmake.lua"), "target('hello')\n").unwrap();
        std::fs::write(tmp.path().join("CMakeLists.txt"), "project(test)").unwrap();
        assert!(matches!(
            CppRunner::<RealCommandRunner>::detect_build_system(tmp.path()),
            CppBuildSystem::Xmake
        ));
    }

    #[test]
    fn has_googletest_false_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("CMakeLists.txt"),
            "project(mylib)\nadd_library(mylib lib.cpp)",
        )
        .unwrap();
        assert!(!CppRunner::<RealCommandRunner>::has_googletest(tmp.path()));
    }
}
