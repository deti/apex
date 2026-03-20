//! Kotlin per-test branch indexing via JaCoCo and Gradle.
//!
//! Strategy:
//! 1. `./gradlew test` — run all tests with JaCoCo instrumentation
//! 2. Parse Gradle verbose output to identify test names
//! 3. Parse JaCoCo XML report into BranchIds
//! 4. Aggregate into BranchIndex
//!
//! Kotlin uses the same JaCoCo coverage toolchain as Java and is typically
//! built with Gradle, so this indexer mirrors the Java approach.

use crate::{
    java::parse_jacoco_xml,
    types::{hash_source_files, BranchIndex, TestTrace},
};
use apex_core::types::{BranchId, ExecutionStatus, Language};
use std::path::Path;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse Gradle test output to extract qualified test names.
///
/// Gradle verbose output format:
/// ```text
/// com.example.FooTest > testBar PASSED
/// com.example.FooTest > testBaz FAILED
/// ```
///
/// Returns fully-qualified test names like `"com.example.FooTest.testBar"`.
pub fn parse_gradle_test_list(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| l.contains("PASSED") || l.contains("FAILED"))
        .filter_map(|l| {
            let parts: Vec<&str> = l.split(" > ").collect();
            if parts.len() >= 2 {
                let class = parts[0].trim();
                let method = parts[1].split_whitespace().next()?;
                Some(format!("{class}.{method}"))
            } else {
                None
            }
        })
        .collect()
}

/// Build a BranchIndex for a Kotlin project using Gradle + JaCoCo.
///
/// Runs `./gradlew test jacocoTestReport` and parses the XML report.
/// Per-test traces are synthesised from the Gradle test output.
pub async fn build_kotlin_index(
    target_root: &Path,
    _parallelism: usize,
) -> std::result::Result<BranchIndex, Box<dyn std::error::Error + Send + Sync>> {
    let target_root = std::fs::canonicalize(target_root)?;
    info!(target = %target_root.display(), "building Kotlin branch index");

    // Run: ./gradlew test jacocoTestReport --info
    let output = tokio::process::Command::new("./gradlew")
        .args(["test", "jacocoTestReport", "--info"])
        .current_dir(&target_root)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(%stderr, "gradlew test returned non-zero");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse test names from Gradle output.
    let test_names = parse_gradle_test_list(&stdout);

    // Locate JaCoCo XML report (typical Gradle path).
    let jacoco_xml = target_root
        .join("build")
        .join("reports")
        .join("jacoco")
        .join("test")
        .join("jacocoTestReport.xml");

    let xml_content = std::fs::read_to_string(&jacoco_xml)
        .map_err(|e| format!("failed to read JaCoCo XML at {}: {e}", jacoco_xml.display()))?;

    let coverage =
        parse_jacoco_xml(&xml_content).map_err(|e| format!("JaCoCo XML parse error: {e}"))?;

    // Build synthetic traces — one per test, each covering all branches found.
    // For full per-test granularity, each test would need its own JaCoCo run.
    // This implementation provides a best-effort aggregate trace per test.
    let covered_branches: Vec<BranchId> = coverage
        .branches
        .iter()
        .filter(|b| b.discriminator == 0) // discriminator=0 → covered (see java.rs)
        .cloned()
        .collect();

    let traces: Vec<TestTrace> = test_names
        .iter()
        .map(|name| TestTrace {
            test_name: name.clone(),
            branches: covered_branches.clone(),
            duration_ms: 0,
            status: ExecutionStatus::Pass,
        })
        .collect();

    let profiles = BranchIndex::build_profiles(&traces);
    let covered_count = profiles.len();
    let source_hash = hash_source_files(&target_root, Language::Kotlin);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let index = BranchIndex {
        traces,
        profiles,
        file_paths: coverage.file_paths,
        total_branches: coverage.total_branches,
        covered_branches: covered_count,
        created_at: format!("{now}"),
        language: Language::Kotlin,
        target_root: target_root.clone(),
        source_hash,
    };

    info!(
        total = index.total_branches,
        covered = index.covered_branches,
        "Kotlin branch index built"
    );

    Ok(index)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jacoco_test_name() {
        // JaCoCo test names from Gradle: "com.example.FooTest > testBar PASSED"
        let names = parse_gradle_test_list(
            "com.example.FooTest > testBar PASSED\ncom.example.FooTest > testBaz PASSED\n",
        );
        assert_eq!(
            names,
            vec!["com.example.FooTest.testBar", "com.example.FooTest.testBaz"]
        );
    }

    #[test]
    fn parse_gradle_test_list_includes_failed() {
        let names = parse_gradle_test_list(
            "com.example.FooTest > testPass PASSED\ncom.example.FooTest > testFail FAILED\n",
        );
        assert_eq!(
            names,
            vec![
                "com.example.FooTest.testPass",
                "com.example.FooTest.testFail"
            ]
        );
    }

    #[test]
    fn parse_gradle_test_list_empty_input() {
        let names = parse_gradle_test_list("");
        assert!(names.is_empty());
    }

    #[test]
    fn parse_gradle_test_list_no_test_lines() {
        // Lines without PASSED or FAILED are ignored
        let names = parse_gradle_test_list("> Task :test\nBUILD SUCCESSFUL in 3s\n");
        assert!(names.is_empty());
    }

    #[test]
    fn parse_gradle_test_list_mixed_lines() {
        let output = r#"> Task :test
com.example.BarTest > testAdd PASSED
BUILD SUCCESSFUL in 2s
"#;
        let names = parse_gradle_test_list(output);
        assert_eq!(names, vec!["com.example.BarTest.testAdd"]);
    }

    #[test]
    fn parse_gradle_test_list_multiple_classes() {
        let output = "\
            com.example.FooTest > testOne PASSED\n\
            com.example.FooTest > testTwo PASSED\n\
            com.example.BarTest > testA FAILED\n\
            com.example.BarTest > testB PASSED\n";
        let names = parse_gradle_test_list(output);
        assert_eq!(names.len(), 4);
        assert!(names.contains(&"com.example.FooTest.testOne".to_string()));
        assert!(names.contains(&"com.example.BarTest.testA".to_string()));
    }

    #[test]
    fn parse_gradle_test_list_strips_extra_whitespace() {
        // Class name may have leading spaces in some Gradle output formats
        let output = "  com.example.MyTest > testFoo PASSED\n";
        let names = parse_gradle_test_list(output);
        assert_eq!(names, vec!["com.example.MyTest.testFoo"]);
    }

    #[test]
    fn parse_gradle_test_list_ignores_malformed_lines() {
        // Lines that have PASSED but no " > " separator are ignored
        let output = "SomethingPASSED\ncom.example.X > testY PASSED\n";
        let names = parse_gradle_test_list(output);
        assert_eq!(names, vec!["com.example.X.testY"]);
    }
}
