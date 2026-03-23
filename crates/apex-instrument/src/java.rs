use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    config::InstrumentTimeouts,
    error::{ApexError, Result},
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use apex_lang::java::detect_build_tool;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{info, warn};

type JacocoParseResult = (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>);

/// FNV-1a 64-bit hash of a path string.
fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Recursively search for a JaCoCo XML report in submodule build directories.
///
/// Multi-module Gradle projects put reports at `<module>/build/reports/jacoco/test/jacocoTestReport.xml`.
/// Deeply nested projects (e.g. Spring Boot: `core/spring-boot/build/reports/...`) need 4+ levels.
/// We search up to 5 levels deep to cover `group/module/build/reports/jacoco` paths while
/// still avoiding a full tree walk.
fn find_jacoco_report_recursive(target: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    collect_jacoco_reports(target, 0, 5, &mut candidates);
    // Return the largest report (most coverage data)
    candidates.sort_by(|a, b| {
        let size_a = std::fs::metadata(a).map(|m| m.len()).unwrap_or(0);
        let size_b = std::fs::metadata(b).map(|m| m.len()).unwrap_or(0);
        size_b.cmp(&size_a)
    });
    candidates.into_iter().next()
}

fn collect_jacoco_reports(dir: &Path, depth: usize, max_depth: usize, results: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    // Check this directory for a JaCoCo report
    for report_path in &[
        "build/reports/jacoco/test/jacocoTestReport.xml",
        "build/reports/jacoco/jacocoTestReport.xml",
    ] {
        let p = dir.join(report_path);
        if p.exists() {
            results.push(p);
        }
    }
    // Recurse into subdirectories (skip hidden, build, node_modules, .git)
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.')
                || name_str == "build"
                || name_str == "node_modules"
                || name_str == "target"
            {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                collect_jacoco_reports(&path, depth + 1, max_depth, results);
            }
        }
    }
}

/// Detect Kotlin Multiplatform projects (which may need Xcode for native targets).
fn is_kotlin_multiplatform(target: &Path) -> bool {
    // Check root build files
    for name in &["build.gradle.kts", "build.gradle"] {
        if let Ok(content) = std::fs::read_to_string(target.join(name)) {
            if content.contains("kotlin(\"multiplatform\")")
                || content.contains("kotlin-multiplatform")
                || content.contains("KotlinMultiplatform")
            {
                return true;
            }
        }
    }
    // Check settings.gradle for plugin declarations
    for name in &["settings.gradle.kts", "settings.gradle"] {
        if let Ok(content) = std::fs::read_to_string(target.join(name)) {
            if content.contains("kotlin(\"multiplatform\")") || content.contains("kotlin-multiplatform") {
                return true;
            }
        }
    }
    // Check build-logic/convention plugins (projects like ktor apply KMP via convention plugins)
    let build_logic = target.join("build-logic").join("src");
    if build_logic.is_dir() {
        if let Ok(entries) = glob_kmp_in_dir(&build_logic) {
            if entries {
                return true;
            }
        }
    }
    // Check if any first-level submodule applies KMP
    if let Ok(entries) = std::fs::read_dir(target) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            for name in &["build.gradle.kts", "build.gradle"] {
                if let Ok(content) = std::fs::read_to_string(path.join(name)) {
                    if content.contains("kotlin(\"multiplatform\")") || content.contains("ktorbuild.kmp") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Check if any file in dir tree contains multiplatform references.
fn glob_kmp_in_dir(dir: &Path) -> std::io::Result<bool> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if glob_kmp_in_dir(&path)? { return Ok(true); }
        } else if path.extension().and_then(|s| s.to_str()) == Some("kts")
            || path.extension().and_then(|s| s.to_str()) == Some("kt")
        {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if content.contains("multiplatform") { return Ok(true); }
            }
        }
    }
    Ok(false)
}

/// Detect whether the root project is a non-Java umbrella (applies only `base`,
/// `build-scan`, etc.) with subprojects that contain the real Java code.
///
/// Returns `true` when the root build file does NOT apply java/java-library/kotlin
/// plugins but `settings.gradle[.kts]` includes subprojects.  In this case we must
/// run `subprojects { test }` style invocations rather than bare `test`.
fn is_umbrella_project(target: &Path) -> bool {
    let has_java_root = {
        let mut found = false;
        for name in &["build.gradle", "build.gradle.kts"] {
            if let Ok(content) = std::fs::read_to_string(target.join(name)) {
                // Heuristic: if the root build file applies java/java-library/kotlin,
                // it is a self-contained project (or the root *is* the Java project).
                if content.contains("'java'")
                    || content.contains("\"java\"")
                    || content.contains("java-library")
                    || content.contains("org.jetbrains.kotlin.jvm")
                    || content.contains("application")
                    || content.contains("'java-library'")
                {
                    found = true;
                }
            }
        }
        found
    };
    if has_java_root {
        return false;
    }
    // Check for subproject includes in settings.gradle
    for name in &["settings.gradle", "settings.gradle.kts"] {
        if let Ok(content) = std::fs::read_to_string(target.join(name)) {
            if content.contains("include ") || content.contains("include(") {
                return true;
            }
        }
    }
    false
}

/// Detect KMP submodules that should be excluded from JVM test runs.
///
/// Scans for modules whose build files reference platform-specific toolchains
/// (Rust/Cargo, WebRTC native) that cause compilation errors on JVM-only runs.
fn find_excludable_kmp_modules(target: &Path) -> Vec<String> {
    let mut excluded = Vec::new();
    collect_excludable_modules(target, target, &mut excluded);
    excluded
}

fn collect_excludable_modules(root: &Path, dir: &Path, results: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.')
            || matches!(name_str.as_ref(), "build" | "node_modules" | "target" | "buildSrc" | "gradle")
        {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let has_build = path.join("build.gradle.kts").exists() || path.join("build.gradle").exists();
        if has_build && should_exclude_module(&path) {
            if let Ok(rel) = path.strip_prefix(root) {
                let module_path = format!(
                    ":{}",
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join(":")
                );
                results.push(module_path);
            }
        }
        collect_excludable_modules(root, &path, results);
    }
}

fn should_exclude_module(module_dir: &Path) -> bool {
    if let Ok(props) = std::fs::read_to_string(module_dir.join("gradle.properties")) {
        if props.contains("rustCompilation") || props.contains("cargo") {
            return true;
        }
    }
    for name in &["build.gradle.kts", "build.gradle"] {
        if let Ok(content) = std::fs::read_to_string(module_dir.join(name)) {
            if content.contains("cargo") && content.contains("rust") {
                return true;
            }
            if content.contains("webrtc") && content.contains("native") {
                return true;
            }
        }
    }
    false
}

/// Check whether a Gradle build file already applies the JaCoCo plugin.
fn gradle_has_jacoco(target: &Path) -> bool {
    for name in &["build.gradle", "build.gradle.kts"] {
        if let Ok(content) = std::fs::read_to_string(target.join(name)) {
            if content.contains("jacoco") {
                return true;
            }
        }
    }
    false
}

/// Content for a Gradle init script that applies the JaCoCo plugin globally.
///
/// Uses `plugins.apply` with `afterEvaluate` to handle projects that don't have
/// the Java plugin applied yet (Kotlin, Android, etc.).  The `jacocoTestReport`
/// task is configured to produce XML output (which APEX parses).
///
/// Key design decisions for multi-module (e.g., Spring Boot) compatibility:
///   - Uses `allprojects.afterEvaluate` so convention plugins have already run.
///   - Explicitly sets `executionData` on `jacocoTestReport` from all Test tasks,
///     because some frameworks register custom test tasks after the jacoco plugin.
///   - Wires `jacocoTestReport.dependsOn test` so a single `jacocoTestReport` task
///     name triggers the full chain.
///   - Guards against projects whose `test` task exists but has no sources (common
///     in umbrella/platform modules) by using `ignoreFailures = true` on the report.
const JACOCO_INIT_GRADLE: &str = r#"
allprojects {
    afterEvaluate {
        def hasJava = plugins.hasPlugin('java') ||
                      plugins.hasPlugin('java-library') ||
                      plugins.hasPlugin('org.jetbrains.kotlin.jvm') ||
                      plugins.hasPlugin('org.jetbrains.kotlin.multiplatform')
        if (!hasJava) return

        // Apply JaCoCo if not already present.
        if (!plugins.hasPlugin('jacoco')) {
            apply plugin: 'jacoco'
        }

        // Enable the JaCoCo agent on every Test task.
        tasks.withType(Test) {
            jacoco {
                enabled = true
            }
        }

        // Ensure a jacocoTestReport task exists and is correctly wired.
        // The jacoco plugin creates it lazily; force-resolve it.
        def reportTask = tasks.findByName('jacocoTestReport')
        if (reportTask == null) {
            // Some convention plugins suppress task creation — create our own.
            reportTask = tasks.create('jacocoTestReport', JacocoReport) {
                group = 'verification'
                description = 'Generate JaCoCo XML coverage report (APEX-injected)'
            }
        }

        reportTask.configure {
            reports {
                xml.required = true
                html.required = false
            }
            // Wire execution data from ALL test tasks so multi-variant
            // builds (unit + integration) are captured.
            def testTasks = tasks.withType(Test)
            dependsOn testTasks
            executionData.setFrom(files(testTasks.collect { t ->
                t.extensions.findByType(JacocoTaskExtension)?.destinationFile
            }.findAll { it != null }))
            // Wire source sets for XML class-name resolution.
            if (project.hasProperty('sourceSets')) {
                def main = project.sourceSets.findByName('main')
                if (main != null) {
                    sourceDirectories.setFrom(main.allJava.srcDirs)
                    classDirectories.setFrom(main.output)
                }
            }
        }
    }
}
"#;

/// Build a `CommandSpec` with JVM-friendly defaults (configurable timeout, JAVA_HOME).
fn jvm_command(program: &str, target: &Path, timeout_ms: u64) -> CommandSpec {
    let mut spec = CommandSpec::new(program, target).timeout(timeout_ms);
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        spec = spec.env("JAVA_HOME", java_home);
    }
    spec
}

/// Run JaCoCo instrumented tests and return the path to the produced XML report.
async fn run_jacoco(target: &Path, runner: &dyn CommandRunner, timeout_ms: u64) -> Result<PathBuf> {
    let build_tool = detect_build_tool(target);

    info!(
        target = %target.display(),
        build_tool,
        "running JaCoCo instrumented tests"
    );

    if build_tool == "gradle" {
        run_jacoco_gradle(target, runner, timeout_ms).await
    } else {
        run_jacoco_maven(target, runner, timeout_ms).await
    }
}

/// Gradle path: auto-inject JaCoCo via init script when the project lacks the plugin.
async fn run_jacoco_gradle(
    target: &Path,
    runner: &dyn CommandRunner,
    timeout_ms: u64,
) -> Result<PathBuf> {
    let needs_init = !gradle_has_jacoco(target);
    let umbrella = is_umbrella_project(target);

    let mut args: Vec<String> = Vec::new();

    if needs_init {
        // Write a temporary init.gradle that applies JaCoCo to all sub-projects.
        let init_path = target.join(".apex-jacoco-init.gradle");
        std::fs::write(&init_path, JACOCO_INIT_GRADLE)
            .map_err(|e| ApexError::Instrumentation(format!("write init.gradle: {e}")))?;
        info!("injecting JaCoCo via init script (project lacks jacoco plugin)");
        args.push("--init-script".into());
        args.push(init_path.to_string_lossy().into_owned());
    }

    // For Kotlin Multiplatform projects, only run JVM tests to avoid
    // requiring Xcode (for Kotlin/Native) or a browser (for Kotlin/JS).
    let is_kmp = is_kotlin_multiplatform(target);
    let test_task = if is_kmp { "jvmTest" } else { "test" };

    if umbrella {
        // Umbrella projects (e.g., Spring Boot) have a root build.gradle that
        // applies only `base` — no `test` or `jacocoTestReport` task at root.
        // We must target subprojects explicitly via the `:*:task` Gradle syntax.
        //
        // Using `subprojects:test` would be ideal but Gradle doesn't support that
        // syntax. Instead we pass the task names and add `--exclude-task :test`
        // and `--exclude-task :jacocoTestReport` to skip the (nonexistent) root
        // tasks while still running them in all subprojects.
        info!("umbrella project detected — excluding root-level tasks");
        args.extend([
            test_task.into(),
            "jacocoTestReport".into(),
            "--quiet".into(),
            "--continue".into(),
            "-x".into(),
            format!(":{test_task}"),
            "-x".into(),
            ":jacocoTestReport".into(),
        ]);
    } else {
        args.extend([
            test_task.into(),
            "jacocoTestReport".into(),
            "--quiet".into(),
            "--continue".into(), // Don't stop on first submodule failure
        ]);
    }

    if is_kmp {
        // Also request jacocoJvmTestReport (KMP auto-creates this task name)
        args.push("jacocoJvmTestReport".into());

        // Exclude native/JS/Wasm compilation tasks that require platform-specific toolchains
        for exclude in &[
            "compileKotlinNative",
            "compileKotlinJs",
            "compileKotlinWasmJs",
            "compileKotlinWasmWasi",
        ] {
            args.push("-x".into());
            args.push((*exclude).into());
        }

        // Detect and exclude modules that cause compilation errors (e.g. WebRTC with Rust/Cargo)
        let excluded = find_excludable_kmp_modules(target);
        for module_path in &excluded {
            args.push("-x".into());
            args.push(format!("{module_path}:jvmTest"));
            args.push("-x".into());
            args.push(format!("{module_path}:jacocoTestReport"));
            args.push("-x".into());
            args.push(format!("{module_path}:jacocoJvmTestReport"));
        }

        info!(
            excluded_modules = excluded.len(),
            "Kotlin Multiplatform detected — running jvmTest only"
        );
    }

    let spec = jvm_command("./gradlew", target, timeout_ms).args(args);
    let output = runner
        .run_command(&spec)
        .await
        .map_err(|e| ApexError::Instrumentation(format!("spawn gradlew: {e}")))?;

    // Clean up the temporary init script.
    let _ = std::fs::remove_file(target.join(".apex-jacoco-init.gradle"));

    if output.exit_code != 0 {
        warn!(
            exit = output.exit_code,
            "gradlew jacocoTestReport returned non-zero (coverage data may still be valid)"
        );
    }

    // Try primary Gradle report path first, then fallback, then search submodules.
    let primary = target.join("build/reports/jacoco/test/jacocoTestReport.xml");
    if primary.exists() {
        return Ok(primary);
    }
    let fallback = target.join("build/reports/jacoco/jacocoTestReport.xml");
    if fallback.exists() {
        return Ok(fallback);
    }

    // Multi-module projects: search for any JaCoCo XML report in subdirectories.
    if let Some(found) = find_jacoco_report_recursive(target) {
        info!(report = %found.display(), "found JaCoCo report in submodule");
        return Ok(found);
    }

    Err(ApexError::Instrumentation(
        "JaCoCo XML report not found after gradlew jacocoTestReport; \
         ensure the jacocoTestReport task is configured"
            .into(),
    ))
}

/// Maven path: invoke JaCoCo goals directly (no pom.xml changes needed).
async fn run_jacoco_maven(
    target: &Path,
    runner: &dyn CommandRunner,
    timeout_ms: u64,
) -> Result<PathBuf> {
    let spec = jvm_command("mvn", target, timeout_ms).args([
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
    timeouts: InstrumentTimeouts,
}

impl JavaInstrumentor {
    pub fn new() -> Self {
        JavaInstrumentor {
            runner: Arc::new(RealCommandRunner),
            timeouts: InstrumentTimeouts::default(),
        }
    }

    /// Create a new instrumentor with a custom command runner (for testing).
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        JavaInstrumentor {
            runner,
            timeouts: InstrumentTimeouts::default(),
        }
    }

    pub fn with_timeouts(mut self, timeouts: InstrumentTimeouts) -> Self {
        self.timeouts = timeouts;
        self
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
        let xml_path = run_jacoco(
            &target.root,
            self.runner.as_ref(),
            self.timeouts.jvm_build_ms,
        )
        .await?;

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

    /// A test-only CommandRunner that returns a configurable output and records
    /// the last spec it received (for assertion).
    struct FakeRunner {
        exit_code: i32,
        fail: bool,
        last_spec: std::sync::Mutex<Option<CommandSpec>>,
    }

    impl FakeRunner {
        fn success() -> Self {
            FakeRunner {
                exit_code: 0,
                fail: false,
                last_spec: std::sync::Mutex::new(None),
            }
        }

        fn failure(exit_code: i32) -> Self {
            FakeRunner {
                exit_code,
                fail: false,
                last_spec: std::sync::Mutex::new(None),
            }
        }

        fn spawn_error() -> Self {
            FakeRunner {
                exit_code: -1,
                fail: true,
                last_spec: std::sync::Mutex::new(None),
            }
        }

        /// Return the last CommandSpec that was passed to run_command.
        fn last_spec(&self) -> Option<CommandSpec> {
            self.last_spec.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run_command(&self, spec: &CommandSpec) -> apex_core::error::Result<CommandOutput> {
            *self.last_spec.lock().unwrap() = Some(spec.clone());
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

    // -----------------------------------------------------------------------
    // JaCoCo auto-injection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gradle_has_jacoco_with_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("build.gradle"), "apply plugin: 'jacoco'\n").unwrap();
        assert!(gradle_has_jacoco(tmp.path()));
    }

    #[test]
    fn test_gradle_has_jacoco_kts_with_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("build.gradle.kts"),
            r#"plugins { id("jacoco") }"#,
        )
        .unwrap();
        assert!(gradle_has_jacoco(tmp.path()));
    }

    #[test]
    fn test_gradle_has_jacoco_without_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("build.gradle"), "apply plugin: 'java'\n").unwrap();
        assert!(!gradle_has_jacoco(tmp.path()));
    }

    #[test]
    fn test_gradle_has_jacoco_no_build_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!gradle_has_jacoco(tmp.path()));
    }

    #[tokio::test]
    async fn test_gradle_injects_init_script_when_no_jacoco() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Gradle project WITHOUT jacoco plugin
        std::fs::write(repo_root.join("build.gradle"), "apply plugin: 'java'\n").unwrap();

        // Create the JaCoCo XML report at the primary path
        let report_dir = repo_root.join("build/reports/jacoco/test");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("jacocoTestReport.xml"), sample_jacoco_xml()).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner.clone());

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 10);

        // Verify the init-script arg was passed
        let spec = runner.last_spec().expect("should have recorded a spec");
        assert!(
            spec.args.contains(&"--init-script".to_string()),
            "expected --init-script in args: {:?}",
            spec.args
        );
        // Also verify `test` comes before `jacocoTestReport`
        let test_pos = spec.args.iter().position(|a| a == "test").unwrap();
        let jacoco_pos = spec
            .args
            .iter()
            .position(|a| a == "jacocoTestReport")
            .unwrap();
        assert!(test_pos < jacoco_pos);
    }

    #[tokio::test]
    async fn test_gradle_skips_init_script_when_jacoco_present() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Gradle project WITH jacoco plugin
        std::fs::write(repo_root.join("build.gradle"), "apply plugin: 'jacoco'\n").unwrap();

        let report_dir = repo_root.join("build/reports/jacoco/test");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("jacocoTestReport.xml"), sample_jacoco_xml()).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner.clone());

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 10);

        // Verify --init-script was NOT passed
        let spec = runner.last_spec().expect("should have recorded a spec");
        assert!(
            !spec.args.contains(&"--init-script".to_string()),
            "should not inject init script when jacoco is present: {:?}",
            spec.args
        );
    }

    #[tokio::test]
    async fn test_jvm_timeout_is_10_minutes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Maven project
        let report_dir = repo_root.join("target/site/jacoco");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("jacoco.xml"), sample_jacoco_xml()).unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner.clone());

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let _ = inst.instrument(&target).await.unwrap();

        let spec = runner.last_spec().expect("should have recorded a spec");
        assert_eq!(
            spec.timeout_ms, 600_000,
            "JVM builds should have a 10-minute timeout"
        );
    }

    #[tokio::test]
    async fn test_gradle_init_script_cleaned_up_on_success() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Gradle project WITHOUT jacoco
        std::fs::write(repo_root.join("build.gradle"), "apply plugin: 'java'\n").unwrap();

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

        let _ = inst.instrument(&target).await.unwrap();

        // The temp init script should be cleaned up
        assert!(
            !repo_root.join(".apex-jacoco-init.gradle").exists(),
            "init script should be cleaned up after run"
        );
    }

    // -----------------------------------------------------------------------
    // Umbrella project detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_umbrella_no_java_with_subprojects() {
        let tmp = tempfile::tempdir().unwrap();
        // Root applies only `base`, not java/java-library/kotlin
        std::fs::write(
            tmp.path().join("build.gradle"),
            "plugins { id 'base' }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("settings.gradle"),
            "include 'core:spring-boot'\ninclude 'module:spring-boot-web'\n",
        )
        .unwrap();
        assert!(is_umbrella_project(tmp.path()));
    }

    #[test]
    fn test_is_umbrella_java_root_not_umbrella() {
        let tmp = tempfile::tempdir().unwrap();
        // Root applies java plugin => not umbrella even with subprojects
        std::fs::write(
            tmp.path().join("build.gradle"),
            "apply plugin: 'java'\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("settings.gradle"),
            "include 'sub'\n",
        )
        .unwrap();
        assert!(!is_umbrella_project(tmp.path()));
    }

    #[test]
    fn test_is_umbrella_java_library_root_not_umbrella() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("build.gradle"),
            "plugins { id 'java-library' }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("settings.gradle"),
            "include 'sub'\n",
        )
        .unwrap();
        assert!(!is_umbrella_project(tmp.path()));
    }

    #[test]
    fn test_is_umbrella_no_settings() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("build.gradle"),
            "plugins { id 'base' }\n",
        )
        .unwrap();
        // No settings.gradle => no subprojects => not umbrella
        assert!(!is_umbrella_project(tmp.path()));
    }

    #[test]
    fn test_is_umbrella_kts_settings() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("build.gradle.kts"),
            "plugins { base }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("settings.gradle.kts"),
            "include(\"sub-a\")\ninclude(\"sub-b\")\n",
        )
        .unwrap();
        assert!(is_umbrella_project(tmp.path()));
    }

    #[tokio::test]
    async fn test_gradle_umbrella_excludes_root_tasks() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Umbrella project: root has `base` only, settings has includes
        std::fs::write(
            repo_root.join("build.gradle"),
            "plugins { id 'base' }\n",
        )
        .unwrap();
        std::fs::write(
            repo_root.join("settings.gradle"),
            "include 'core:my-lib'\n",
        )
        .unwrap();

        // Create the JaCoCo XML report in a submodule path
        let report_dir = repo_root.join("core/my-lib/build/reports/jacoco/test");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(
            report_dir.join("jacocoTestReport.xml"),
            sample_jacoco_xml(),
        )
        .unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner.clone());

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let result = inst.instrument(&target).await.unwrap();
        assert_eq!(result.branch_ids.len(), 10);

        // Verify -x :test and -x :jacocoTestReport are in args (umbrella exclusion)
        let spec = runner.last_spec().expect("should have recorded a spec");
        assert!(
            spec.args.contains(&"-x".to_string()),
            "expected -x in args: {:?}",
            spec.args
        );
        assert!(
            spec.args.contains(&":test".to_string()),
            "expected :test exclusion in args: {:?}",
            spec.args
        );
        assert!(
            spec.args.contains(&":jacocoTestReport".to_string()),
            "expected :jacocoTestReport exclusion in args: {:?}",
            spec.args
        );
        // Also verify init script was injected (no jacoco in build.gradle)
        assert!(
            spec.args.contains(&"--init-script".to_string()),
            "expected --init-script in args: {:?}",
            spec.args
        );
    }

    #[tokio::test]
    async fn test_gradle_non_umbrella_no_task_exclusion() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();

        // Standard Java project (not umbrella)
        std::fs::write(
            repo_root.join("build.gradle"),
            "apply plugin: 'java'\n",
        )
        .unwrap();

        let report_dir = repo_root.join("build/reports/jacoco/test");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(
            report_dir.join("jacocoTestReport.xml"),
            sample_jacoco_xml(),
        )
        .unwrap();

        let runner = Arc::new(FakeRunner::success());
        let inst = JavaInstrumentor::with_runner(runner.clone());

        let target = Target {
            root: repo_root.to_path_buf(),
            language: apex_core::types::Language::Java,
            test_command: Vec::new(),
        };

        let _ = inst.instrument(&target).await.unwrap();

        let spec = runner.last_spec().expect("should have recorded a spec");
        // Non-umbrella should NOT have :test exclusion
        assert!(
            !spec.args.contains(&":test".to_string()),
            "non-umbrella should not exclude :test: {:?}",
            spec.args
        );
    }

    #[test]
    fn test_find_jacoco_report_deep_submodule() {
        // Simulate Spring Boot structure: core/spring-boot/build/reports/jacoco/test/
        let tmp = tempfile::tempdir().unwrap();
        let deep_dir = tmp
            .path()
            .join("core/spring-boot/build/reports/jacoco/test");
        std::fs::create_dir_all(&deep_dir).unwrap();
        std::fs::write(
            deep_dir.join("jacocoTestReport.xml"),
            sample_jacoco_xml(),
        )
        .unwrap();

        let found = find_jacoco_report_recursive(tmp.path());
        assert!(found.is_some(), "should find report in deep submodule");
        assert!(
            found.unwrap().ends_with("jacocoTestReport.xml"),
            "should find the XML report"
        );
    }
}
