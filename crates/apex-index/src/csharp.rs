use crate::types::{hash_source_files, BranchIndex, TestTrace};
use apex_core::hash::fnv1a_hash;
use apex_core::types::{BranchId, ExecutionStatus, Language};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Parse Cobertura XML coverage into branch entries.
///
/// Cobertura XML format (from `dotnet test --collect:"XPlat Code Coverage"`):
/// ```xml
/// <class filename="..."><lines><line number="10" hits="3" /></lines></class>
/// ```
fn parse_csharp_coverage(
    content: &str,
    target_root: &Path,
) -> (Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut branches = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    let mut current_file_id: u64 = 0;
    let mut in_class = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("<class ") {
            if let Some(filename) = extract_xml_attr(trimmed, "filename") {
                let rel_path = derive_relative_path(&filename, target_root);
                let file_id = fnv1a_hash(&rel_path);
                file_paths
                    .entry(file_id)
                    .or_insert_with(|| PathBuf::from(&rel_path));
                current_file_id = file_id;
                in_class = true;
            }
        } else if trimmed.starts_with("</class>") {
            in_class = false;
        } else if trimmed.starts_with("<line ") && in_class {
            let Some(line_num_str) = extract_xml_attr(trimmed, "number") else {
                continue;
            };
            let Some(hits_str) = extract_xml_attr(trimmed, "hits") else {
                continue;
            };
            let Ok(line_num) = line_num_str.parse::<u32>() else {
                continue;
            };
            let Ok(hits) = hits_str.parse::<u32>() else {
                continue;
            };

            let branch = BranchId::new(
                current_file_id,
                line_num,
                0,
                if hits > 0 { 0 } else { 1 },
            );
            branches.push(branch);
        }
    }

    (branches, file_paths)
}

/// Extract an XML attribute value from a tag string.
fn extract_xml_attr(tag: &str, attr_name: &str) -> Option<String> {
    let needle = format!("{attr_name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Derive a relative path from a Cobertura coverage path.
fn derive_relative_path(coverage_path: &str, target_root: &Path) -> String {
    let path = Path::new(coverage_path);
    if let Ok(rel) = path.strip_prefix(target_root) {
        return rel.to_string_lossy().to_string();
    }
    coverage_path.to_string()
}

/// Build test traces from `dotnet test` verbose output.
/// Each `Passed TestName` or `Failed TestName` line is a test result.
fn build_traces_from_output(stdout: &str, all_branches: &[BranchId]) -> Vec<TestTrace> {
    let mut traces = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();

        if let Some(name) = trimmed.strip_prefix("Passed ") {
            traces.push(TestTrace {
                test_name: name.to_string(),
                branches: all_branches.to_vec(),
                duration_ms: 0,
                status: ExecutionStatus::Pass,
            });
        } else if let Some(name) = trimmed.strip_prefix("Failed ") {
            traces.push(TestTrace {
                test_name: name.to_string(),
                branches: all_branches.to_vec(),
                duration_ms: 0,
                status: ExecutionStatus::Fail,
            });
        }
    }

    traces
}

/// Build a BranchIndex for a C# project by running tests with coverage.
pub async fn build_csharp_index(
    target_root: &Path,
    _parallelism: usize,
) -> std::result::Result<BranchIndex, Box<dyn std::error::Error + Send + Sync>> {
    let target_root = std::fs::canonicalize(target_root)?;
    info!(target = %target_root.display(), "building C# branch index");

    // Run: dotnet test --collect:"XPlat Code Coverage" -v n
    let output = tokio::process::Command::new("dotnet")
        .args(["test", "--collect:XPlat Code Coverage", "-v", "n"])
        .current_dir(&target_root)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(%stderr, "dotnet test --collect returned non-zero");
    }

    // Find the most recent coverage.cobertura.xml
    let coverage_xml = find_coverage_xml(&target_root)?;
    let content = std::fs::read_to_string(&coverage_xml)
        .map_err(|e| format!("failed to read coverage XML: {e}"))?;

    let (all_branches, file_paths) = parse_csharp_coverage(&content, &target_root);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let traces = build_traces_from_output(&stdout, &all_branches);

    let profiles = BranchIndex::build_profiles(&traces);
    let covered_branches = profiles.len();
    let source_hash = hash_source_files(&target_root, Language::CSharp);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let index = BranchIndex {
        traces,
        profiles,
        file_paths,
        total_branches: all_branches.len(),
        covered_branches,
        created_at: format!("{now}"),
        language: Language::CSharp,
        target_root: target_root.clone(),
        source_hash,
    };

    info!(
        total = index.total_branches,
        covered = index.covered_branches,
        "C# branch index built"
    );

    Ok(index)
}

/// Find the most recent coverage.cobertura.xml under TestResults.
fn find_coverage_xml(
    target_root: &Path,
) -> std::result::Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let test_results = target_root.join("TestResults");
    if !test_results.exists() {
        return Err("TestResults directory not found".into());
    }

    let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;

    if let Ok(entries) = std::fs::read_dir(&test_results) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let xml = path.join("coverage.cobertura.xml");
                if xml.exists() {
                    if let Ok(meta) = xml.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if newest.as_ref().is_none_or(|(_, t)| modified > *t) {
                                newest = Some((xml, modified));
                            }
                        }
                    }
                }
            }
        }
    }

    newest
        .map(|(p, _)| p)
        .ok_or_else(|| "coverage.cobertura.xml not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<coverage version="1">
  <packages>
    <package>
      <classes>
        <class filename="Program.cs">
          <lines>
            <line number="10" hits="3" />
            <line number="14" hits="0" />
            <line number="20" hits="1" />
          </lines>
        </class>
      </classes>
    </package>
  </packages>
</coverage>"#;

    #[test]
    fn parse_csharp_coverage_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_csharp_coverage(FIXTURE, tmp.path());
        assert_eq!(branches.len(), 3);
        assert_eq!(file_paths.len(), 1);
    }

    #[test]
    fn parse_csharp_coverage_direction() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(FIXTURE, tmp.path());
        // hits=3 -> dir 0, hits=0 -> dir 1, hits=1 -> dir 0
        assert_eq!(branches[0].direction, 0);
        assert_eq!(branches[1].direction, 1);
        assert_eq!(branches[2].direction, 0);
    }

    #[test]
    fn parse_csharp_coverage_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_csharp_coverage(
            "<coverage><packages><package><classes></classes></package></packages></coverage>",
            tmp.path(),
        );
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }

    #[test]
    fn parse_csharp_coverage_file_id_consistent() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="Foo.cs"><lines>
<line number="1" hits="1" />
<line number="2" hits="0" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(xml, tmp.path());
        assert_eq!(branches[0].file_id, branches[1].file_id);
    }

    #[test]
    fn build_traces_parses_pass_fail() {
        let stdout = "  Passed MyNamespace.Tests.TestAdd\n  Failed MyNamespace.Tests.TestSub\n";
        let branches = vec![BranchId::new(1, 10, 0, 0)];
        let traces = build_traces_from_output(stdout, &branches);
        assert_eq!(traces.len(), 2);
        assert_eq!(traces[0].test_name, "MyNamespace.Tests.TestAdd");
        assert_eq!(traces[0].status, ExecutionStatus::Pass);
        assert_eq!(traces[1].test_name, "MyNamespace.Tests.TestSub");
        assert_eq!(traces[1].status, ExecutionStatus::Fail);
    }

    #[test]
    fn extract_xml_attr_works() {
        let tag = r#"<class filename="Program.cs" name="Program">"#;
        assert_eq!(
            extract_xml_attr(tag, "filename"),
            Some("Program.cs".to_string())
        );
        assert_eq!(extract_xml_attr(tag, "missing"), None);
    }
}
