use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{info, warn};

type JacocoParseResult = (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>);

/// Detect whether the project uses Gradle or Maven.
fn detect_build_tool(target: &Path) -> &'static str {
    if target.join("build.gradle").exists() || target.join("build.gradle.kts").exists() {
        "gradle"
    } else {
        "maven"
    }
}

/// Run JaCoCo instrumented tests and return the path to the produced XML report.
async fn run_jacoco(target: &Path, runner: &dyn CommandRunner) -> Result<PathBuf> {
    let build_tool = detect_build_tool(target);

    info!(
        target = %target.display(),
        build_tool,
        "running JaCoCo instrumented tests"
    );

    if build_tool == "gradle" {
        let spec = CommandSpec::new("./gradlew", target).args(["jacocoTestReport", "--quiet"]);
        let output = runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("spawn gradlew: {e}")))?;

        if output.exit_code != 0 {
            warn!(
                exit = output.exit_code,
                "gradlew jacocoTestReport returned non-zero (coverage data may still be valid)"
            );
        }

        // Try primary Gradle report path first, then fallback.
        let primary = target.join("build/reports/jacoco/test/jacocoTestReport.xml");
        if primary.exists() {
            return Ok(primary);
        }
        let fallback = target.join("build/reports/jacoco/jacocoTestReport.xml");
        if fallback.exists() {
            return Ok(fallback);
        }

        Err(ApexError::Instrumentation(
            "JaCoCo XML report not found after gradlew jacocoTestReport; \
             ensure the jacocoTestReport task is configured"
                .into(),
        ))
    } else {
        // Maven
        let spec = CommandSpec::new("mvn", target).args([
            "-q",
            "org.jacoco:jacoco-maven-plugin:0.8.11:prepare-agent",
            "test",
            "org.jacoco:jacoco-maven-plugin:0.8.11:report",
            "-Djacoco.skip=false",
        ]);
        let output = runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("spawn mvn: {e}")))?;

        if output.exit_code != 0 {
            warn!(
                exit = output.exit_code,
                "mvn jacoco run returned non-zero (coverage data may still be valid)"
            );
        }

        let report = target.join("target/site/jacoco/jacoco.xml");
        if report.exists() {
            return Ok(report);
        }

        Err(ApexError::Instrumentation(
            "JaCoCo XML report not found at target/site/jacoco/jacoco.xml; \
             ensure JaCoCo plugin is configured in pom.xml"
                .into(),
        ))
    }
}

/// Extract the value of an XML attribute from a tag string.
/// Finds the pattern `name="value"` and returns `value`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = tag.find(key.as_str())?;
    let after_quote = start + key.len();
    let end = tag[after_quote..].find('"')?;
    Some(&tag[after_quote..after_quote + end])
}

/// Parse a JaCoCo XML report using simple string scanning (no external XML crate).
///
/// Returns `(all_branches, executed_branches, file_paths)`.
fn parse_jacoco_xml(
    xml_content: &str,
    _source_root: &Path,
    _repo_root: &Path,
) -> Result<JacocoParseResult> {
    let mut all_branches: Vec<BranchId> = Vec::new();
    let mut executed_branches: Vec<BranchId> = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    let mut current_package = String::new();
    let mut current_file_id: u64 = 0;

    // Split on '<' to get individual tag fragments.
    for raw in xml_content.split('<') {
        let fragment = raw.trim();
        if fragment.is_empty() {
            continue;
        }

        if fragment.starts_with("package ") {
            if let Some(pkg) = attr(fragment, "name") {
                current_package = pkg.to_string();
            }
        } else if fragment.starts_with("sourcefile ") {
            if let Some(filename) = attr(fragment, "name") {
                let rel_path = if current_package.is_empty() {
                    filename.to_string()
                } else {
                    format!("{current_package}/{filename}")
                };
                current_file_id = fnv1a_hash(&rel_path);
                file_paths.insert(current_file_id, PathBuf::from(&rel_path));
            }
        } else if fragment.starts_with("line ") {
            // Only process if we are inside a sourcefile.
            if current_file_id == 0 {
                continue;
            }

            let nr: u32 = match attr(fragment, "nr").and_then(|v| v.parse().ok()) {
                Some(n) => n,
                None => continue,
            };
            let mb: u32 = attr(fragment, "mb")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let cb: u32 = attr(fragment, "cb")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);

            let total = mb + cb;
            if total == 0 {
                continue;
            }

            // First `cb` arms are covered, remaining `mb` arms are uncovered.
            for arm_idx in 0..total {
                let branch = BranchId::new(current_file_id, nr, 0, arm_idx as u8);
                all_branches.push(branch.clone());
                if arm_idx < cb {
                    executed_branches.push(branch);
                }
            }
        } else if fragment.starts_with("/sourcefile") {
            current_file_id = 0;
        } else if fragment.starts_with("/package") {
            current_package.clear();
            current_file_id = 0;
        }
    }

    Ok((all_branches, executed_branches, file_paths))
}

// ---------------------------------------------------------------------------
// Instrumentor implementation
// ---------------------------------------------------------------------------

pub struct JavaInstrumentor {
    runner: Arc<dyn CommandRunner>,
}

impl JavaInstrumentor {
    pub fn new() -> Self {
        JavaInstrumentor {
            runner: Arc::new(RealCommandRunner),
        }
    }

    /// Create a new instrumentor with a custom command runner (for testing).
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        JavaInstrumentor { runner }
    }
}

impl Default for JavaInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Instrumentor for JavaInstrumentor {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        let xml_path = run_jacoco(&target.root, self.runner.as_ref()).await?;

        let xml_content = std::fs::read_to_string(&xml_path)
            .map_err(|e| ApexError::Instrumentation(format!("read JaCoCo XML: {e}")))?;

        // Use target root as both source_root and repo_root for path normalisation.
        let (branch_ids, executed_branch_ids, file_paths) =
            parse_jacoco_xml(&xml_content, &target.root, &target.root)?;

        info!(
            all = branch_ids.len(),
            executed = executed_branch_ids.len(),
            files = file_paths.len(),
            "parsed JaCoCo XML report"
        );

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids,
            executed_branch_ids,
            file_paths,
            work_dir: target.root.clone(),
        })
    }

    fn branch_ids(&self) -> &[BranchId] {
        // Stateless instrumentor -- branch ids live in the returned InstrumentedTarget.
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;

    /// A test-only CommandRunner that returns a configurable output.
    struct FakeRunner {
        exit_code: i32,
        fail: bool,
    }

    impl FakeRunner {
        fn success() -> Self {
            FakeRunner {
                exit_code: 0,
                fail: false,
            }
        }

        fn failure(exit_code: i32) -> Self {
            FakeRunner {
                exit_code,
                fail: false,
            }
        }

        fn spawn_error() -> Self {
            FakeRunner {
                exit_code: -1,
                fail: true,
            }
        }
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run_command(
            &self,
            _spec: &CommandSpec,
        ) -> apex_core::error::Result<CommandOutput> {
            if self.fail {
                return Err(ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn failed".into(),
                });
            }
            Ok(CommandOutput {
                exit_code: self.exit_code,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
    }

    // ------------------------------------------------------------------
    // attr() tests
    // ------------------------------------------------------------------

    #[test]
    fn test_attr_basic() {
        assert_eq!(attr(r#"line nr="42" mb="1" cb="3"/>"#, "nr"), Some("42"));
        assert_eq!(attr(r#"line nr="42" mb="1" cb="3"/>"#, "mb"), Some("1"));
        assert_eq!(attr(r#"line nr="42" mb="1" cb="3"/>"#, "cb"), Some("3"));
    }

    #[test]
    fn test_attr_missing() {
        assert_eq!(attr(r#"line nr="42"/>"#, "mb"), None);
    }

    #[test]
    fn test_attr_empty_value() {
        assert_eq!(attr(r#"package name=""/>"#, "name"), Some(""));
    }

    // ------------------------------------------------------------------
    // parse_jacoco_xml() tests
    // ------------------------------------------------------------------

    fn sample_jacoco_xml() -> &'static str {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<report name="test">
  <package name="com/example">
    <sourcefile name="App.java">
      <line nr="10" mi="0" ci="5" mb="1" cb="3"/>
      <line nr="15" mi="2" ci="3" mb="0" cb="0"/>
      <line nr="20" mi="0" ci="1" mb="2" cb="0"/>
    </sourcefile>
    <sourcefile name="Util.java">
      <line nr="5" mi="0" ci="1" mb="0" cb="2"/>
    </sourcefile>
  </package>
  <package name="com/example/inner">
    <sourcefile name="Helper.java">
      <line nr="8" mi="0" ci="3" mb="1" cb="1"/>
    </sourcefile>
  </package>
</report>"#
    }

    #[test]
    fn test_parse_jacoco_xml_branch_totals() {
        let (all, exec, fps) =
            parse_jacoco_xml(sample_jacoco_xml(), Path::new("."), Path::new(".")).unwrap();

        assert_eq!(all.len(), 10);
        assert_eq!(exec.len(), 6);
        assert_eq!(fps.len(), 3);
    }

    #[test]
    fn test_parse_jacoco_xml_file_paths() {
        let (_, _, fps) =
            parse_jacoco_xml(sample_jacoco_xml(), Path::new("."), Path::new(".")).unwrap();

        let paths: Vec<String> = fps
            .values()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"com/example/App.java".to_string()));
        assert!(paths.contains(&"com/example/Util.java".to_string()));
        assert!(paths.contains(&"com/example/inner/Helper.java".to_string()));
    }

    #[test]
    fn test_parse_jacoco_xml_line_numbers() {
        let (all, _, _) =
            parse_jacoco_xml(sample_jacoco_xml(), Path::new("."), Path::new(".")).unwrap();

        let lines: Vec<u32> = all.iter().map(|b| b.line).collect();
        assert!(lines.contains(&10));
        assert!(lines.contains(&20));
        assert!(!lines.contains(&15)); // mb=0, cb=0 -> no branches
    }

    #[test]
    fn test_parse_jacoco_xml_arm_indices() {
        let (all, exec, _) =
            parse_jacoco_xml(sample_jacoco_xml(), Path::new("."), Path::new(".")).unwrap();

        let app_file_id = fnv1a_hash("com/example/App.java");
        let line10: Vec<_> = all
            .iter()
            .filter(|b| b.file_id == app_file_id && b.line == 10)
            .collect();
        assert_eq!(line10.len(), 4);
        assert_eq!(
            line10.iter().map(|b| b.direction).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );

        let line10_exec: Vec<_> = exec
            .iter()
            .filter(|b| b.file_id == app_file_id && b.line == 10)
            .collect();
        assert_eq!(line10_exec.len(), 3); // cb=3
    }

    #[test]
    fn test_parse_jacoco_xml_empty_report() {
        let xml = r#"<?xml version="1.0"?><report name="empty"></report>"#;
        let (all, exec, fps) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        assert_eq!(all.len(), 0);
        assert_eq!(exec.len(), 0);
        assert_eq!(fps.len(), 0);
    }

    #[test]
    fn test_parse_jacoco_xml_no_branches_on_lines() {
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <package name="com/foo">
    <sourcefile name="Bar.java">
      <line nr="1" mi="5" ci="0" mb="0" cb="0"/>
      <line nr="2" mi="0" ci="5" mb="0" cb="0"/>
    </sourcefile>
  </package>
</report>"#;
        let (all, exec, fps) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        assert_eq!(all.len(), 0);
        assert_eq!(exec.len(), 0);
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn test_detect_build_tool_defaults_to_maven() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(detect_build_tool(tmp.path()), "maven");
    }

    #[test]
    fn test_detect_build_tool_gradle() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("build.gradle"), "").unwrap();
        assert_eq!(detect_build_tool(tmp.path()), "gradle");
    }

    #[test]
    fn test_detect_build_tool_gradle_kts() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("build.gradle.kts"), "").unwrap();
        assert_eq!(detect_build_tool(tmp.path()), "gradle");
    }

    #[test]
    fn test_new_and_default() {
        let inst = JavaInstrumentor::new();
        assert!(inst.branch_ids().is_empty());
        let inst2 = JavaInstrumentor::default();
        assert!(inst2.branch_ids().is_empty());
    }

    #[test]
    fn test_fnv1a_deterministic() {
        assert_eq!(
            fnv1a_hash("com/example/App.java"),
            fnv1a_hash("com/example/App.java")
        );
    }

    #[test]
    fn test_fnv1a_different() {
        assert_ne!(fnv1a_hash("A.java"), fnv1a_hash("B.java"));
    }

    #[test]
    fn test_attr_no_closing_quote() {
        assert_eq!(attr(r#"line nr="42"#, "nr"), None);
    }

    #[test]
    fn test_parse_jacoco_xml_sourcefile_close_resets_file_id() {
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <package name="com/foo">
    <sourcefile name="A.java">
      <line nr="1" mi="0" ci="1" mb="1" cb="1"/>
    </sourcefile>
    <line nr="99" mi="0" ci="1" mb="2" cb="0"/>
  </package>
</report>"#;
        let (all, _, fps) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        assert!(!all.iter().any(|b| b.line == 99));
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn test_parse_jacoco_xml_package_close_resets() {
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <package name="com/a">
    <sourcefile name="X.java">
      <line nr="1" mi="0" ci="1" mb="0" cb="2"/>
    </sourcefile>
  </package>
  <package name="com/b">
    <sourcefile name="Y.java">
      <line nr="5" mi="0" ci="1" mb="1" cb="1"/>
    </sourcefile>
  </package>
</report>"#;
        let (all, exec, fps) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        assert_eq!(fps.len(), 2);
        assert_eq!(all.len(), 4);
        assert_eq!(exec.len(), 3);
    }

    #[test]
    fn test_parse_jacoco_xml_sourcefile_no_package() {
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <sourcefile name="Standalone.java">
    <line nr="1" mi="0" ci="1" mb="0" cb="1"/>
  </sourcefile>
</report>"#;
        let (all, _, fps) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        assert_eq!(all.len(), 1);
        let fid = fnv1a_hash("Standalone.java");
        assert!(fps.contains_key(&fid));
    }

    #[test]
    fn test_parse_jacoco_xml_line_no_nr() {
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <package name="com/x">
    <sourcefile name="A.java">
      <line mi="0" ci="1" mb="1" cb="0"/>
    </sourcefile>
  </package>
</report>"#;
        let (all, _, _) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        assert!(all.is_empty());
    }

    // -----------------------------------------------------------------------
    // Mock-based instrument() tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_instrument_gradle_success_with_mock() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Make it look like a Gradle project
        std::fs::write(repo_root.join("build.gradle"), "").unwrap();

        // Create the JaCoCo XML report at the primary path
        let report_dir = repo_root.join("build/reports/jacoco/test");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("jacocoTestReport.xml"), sample_jacoco_xml()).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 10);
        assert_eq!(result.executed_branch_ids.len(), 6);
        assert_eq!(result.file_paths.len(), 3);
    }

    #[tokio::test]
    async fn test_instrument_maven_success_with_mock() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Maven project (no build.gradle)
        let report_dir = repo_root.join("target/site/jacoco");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("jacoco.xml"), sample_jacoco_xml()).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 10);
        assert_eq!(result.executed_branch_ids.len(), 6);
    }

    #[tokio::test]
    async fn test_instrument_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        let runner = Arc::new(FakeRunner::spawn_error());
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_instrument_missing_report() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        // Maven project, but no report file created

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("JaCoCo XML report not found"));
    }

    #[tokio::test]
    async fn test_instrument_gradle_fallback_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Gradle project
        std::fs::write(repo_root.join("build.gradle"), "").unwrap();

        // Create the report at the fallback path (not primary)
        let fallback_dir = repo_root.join("build/reports/jacoco");
        std::fs::create_dir_all(&fallback_dir).unwrap();
        std::fs::write(
            fallback_dir.join("jacocoTestReport.xml"),
            sample_jacoco_xml(),
        )
        .unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 10);
    }

    #[tokio::test]
    async fn test_instrument_nonzero_exit_still_reads_report() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Maven project
        let report_dir = repo_root.join("target/site/jacoco");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("jacoco.xml"), sample_jacoco_xml()).unwrap();

        let runner = Arc::new(FakeRunner::failure(1));
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        // Non-zero exit is a warning, coverage may still be valid
        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 10);
    }

    // -----------------------------------------------------------------------
    // Additional coverage tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_attr_with_special_characters() {
        assert_eq!(
            attr(r#"package name="com/example/inner"/>"#, "name"),
            Some("com/example/inner")
        );
    }

    #[test]
    fn test_attr_multiple_attributes() {
        let tag = r#"line nr="10" mi="0" ci="5" mb="1" cb="3"/>"#;
        assert_eq!(attr(tag, "nr"), Some("10"));
        assert_eq!(attr(tag, "mi"), Some("0"));
        assert_eq!(attr(tag, "ci"), Some("5"));
        assert_eq!(attr(tag, "mb"), Some("1"));
        assert_eq!(attr(tag, "cb"), Some("3"));
    }

    #[test]
    fn test_attr_not_present_at_all() {
        assert_eq!(attr(r#"line nr="42"/>"#, "missing"), None);
    }

    #[tokio::test]
    async fn test_instrument_gradle_no_report_at_either_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Gradle project but no report at either path
        std::fs::write(repo_root.join("build.gradle"), "").unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("JaCoCo XML report not found"));
    }

    #[tokio::test]
    async fn test_instrument_gradle_kts_project() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        std::fs::write(repo_root.join("build.gradle.kts"), "").unwrap();

        let report_dir = repo_root.join("build/reports/jacoco/test");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("jacocoTestReport.xml"), sample_jacoco_xml()).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 10);
    }

    #[test]
    fn test_parse_jacoco_xml_missing_mb_cb_defaults_to_zero() {
        // Lines without mb/cb attributes should default to 0
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <package name="com/foo">
    <sourcefile name="A.java">
      <line nr="1" mi="0" ci="1"/>
    </sourcefile>
  </package>
</report>"#;
        let (all, _, fps) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        // mb=0 + cb=0 = 0 total branches, so no branches created
        assert_eq!(all.len(), 0);
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn test_parse_jacoco_xml_all_covered() {
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <package name="com/foo">
    <sourcefile name="All.java">
      <line nr="1" mi="0" ci="5" mb="0" cb="4"/>
    </sourcefile>
  </package>
</report>"#;
        let (all, exec, _) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(exec.len(), 4);
    }

    #[test]
    fn test_parse_jacoco_xml_all_missed() {
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <package name="com/foo">
    <sourcefile name="None.java">
      <line nr="1" mi="5" ci="0" mb="3" cb="0"/>
    </sourcefile>
  </package>
</report>"#;
        let (all, exec, _) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(exec.len(), 0);
    }

    #[test]
    fn test_parse_jacoco_xml_multiple_lines_in_sourcefile() {
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <package name="pkg">
    <sourcefile name="Multi.java">
      <line nr="1" mi="0" ci="1" mb="1" cb="1"/>
      <line nr="2" mi="0" ci="1" mb="0" cb="2"/>
      <line nr="3" mi="0" ci="1" mb="3" cb="0"/>
      <line nr="4" mi="0" ci="1" mb="0" cb="0"/>
    </sourcefile>
  </package>
</report>"#;
        let (all, exec, _) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        // line 1: 2 branches (1 exec), line 2: 2 (2 exec), line 3: 3 (0 exec), line 4: 0
        assert_eq!(all.len(), 7);
        assert_eq!(exec.len(), 3);
    }

    #[test]
    fn test_fnv1a_empty_string() {
        assert_eq!(fnv1a_hash(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn test_parse_jacoco_xml_interleaved_packages() {
        // After closing a package, opening a new one should use the new package name
        let xml = r#"<?xml version="1.0"?>
<report name="test">
  <package name="first">
    <sourcefile name="A.java">
      <line nr="1" mi="0" ci="1" mb="1" cb="1"/>
    </sourcefile>
  </package>
  <package name="second">
    <sourcefile name="B.java">
      <line nr="1" mi="0" ci="1" mb="0" cb="1"/>
    </sourcefile>
  </package>
</report>"#;
        let (_, _, fps) = parse_jacoco_xml(xml, Path::new("."), Path::new(".")).unwrap();
        let paths: Vec<String> = fps
            .values()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"first/A.java".to_string()));
        assert!(paths.contains(&"second/B.java".to_string()));
    }

    #[tokio::test]
    async fn test_instrument_maven_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        // No build.gradle => maven

        let runner = Arc::new(FakeRunner::spawn_error());
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_instrument_gradle_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        std::fs::write(repo_root.join("build.gradle"), "").unwrap();

        let runner = Arc::new(FakeRunner::spawn_error());
        let inst = JavaInstrumentor::with_runner(runner);

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await;
        assert!(result.is_err());
    }
}
