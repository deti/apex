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

            let branch = BranchId::new(current_file_id, line_num, 0, if hits > 0 { 0 } else { 1 });
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

    // Target: lines 41-50 — missing/malformed XML attribute error paths
    // These are the `continue` branches when number/hits are absent or non-numeric.
    #[test]
    fn bug_parse_csharp_coverage_line_missing_number_attr() {
        // <line> tag with no "number" attribute — should be skipped (line 41-42)
        let xml = r#"<coverage><packages><package><classes>
<class filename="A.cs"><lines>
<line hits="5" />
<line number="10" hits="1" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(xml, tmp.path());
        // Only the well-formed line should be parsed
        assert_eq!(
            branches.len(),
            1,
            "line missing 'number' attr must be skipped"
        );
        assert_eq!(branches[0].line, 10);
    }

    // Target: lines 43-44 — missing hits attribute
    #[test]
    fn bug_parse_csharp_coverage_line_missing_hits_attr() {
        // <line> tag with no "hits" attribute — should be skipped (line 43-44)
        let xml = r#"<coverage><packages><package><classes>
<class filename="B.cs"><lines>
<line number="5" />
<line number="7" hits="2" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(xml, tmp.path());
        assert_eq!(
            branches.len(),
            1,
            "line missing 'hits' attr must be skipped"
        );
        assert_eq!(branches[0].line, 7);
    }

    // Target: lines 46-48 — non-numeric line number
    #[test]
    fn bug_parse_csharp_coverage_line_nonnumeric_number() {
        // "number" attribute has non-numeric value — should be skipped (line 46-48)
        let xml = r#"<coverage><packages><package><classes>
<class filename="C.cs"><lines>
<line number="abc" hits="1" />
<line number="10" hits="1" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(xml, tmp.path());
        assert_eq!(branches.len(), 1, "non-numeric line number must be skipped");
        assert_eq!(branches[0].line, 10);
    }

    // Target: lines 49-50 — non-numeric hits value
    #[test]
    fn bug_parse_csharp_coverage_line_nonnumeric_hits() {
        // "hits" attribute has non-numeric value — should be skipped (line 49-50)
        let xml = r#"<coverage><packages><package><classes>
<class filename="D.cs"><lines>
<line number="3" hits="N/A" />
<line number="4" hits="0" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(xml, tmp.path());
        assert_eq!(branches.len(), 1, "non-numeric hits must be skipped");
        assert_eq!(branches[0].line, 4);
        assert_eq!(branches[0].direction, 1); // hits=0
    }

    // Target: lines 41-50 — lines outside any <class> block are ignored
    #[test]
    fn bug_parse_csharp_coverage_line_outside_class_ignored() {
        // <line> elements outside a <class> tag must not be indexed
        let xml = r#"<coverage>
<line number="99" hits="5" />
<packages><package><classes>
<class filename="E.cs"><lines>
<line number="1" hits="1" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(xml, tmp.path());
        assert_eq!(branches.len(), 1, "lines outside <class> must be ignored");
        assert_eq!(branches[0].line, 1);
    }

    // Target: lines 164-199 — find_coverage_xml returns error when TestResults absent
    #[test]
    fn bug_find_coverage_xml_no_test_results_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // No TestResults directory exists
        let result = find_coverage_xml(tmp.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("TestResults"),
            "error should mention TestResults, got: {msg}"
        );
    }

    // Target: lines 176-198 — TestResults exists but contains no coverage XML
    #[test]
    fn bug_find_coverage_xml_empty_test_results() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("TestResults")).unwrap();
        let result = find_coverage_xml(tmp.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("coverage.cobertura.xml"),
            "error should mention coverage file, got: {msg}"
        );
    }

    // Target: lines 178-193 — TestResults with subdirs but no coverage XML in them
    #[test]
    fn bug_find_coverage_xml_subdir_without_xml() {
        let tmp = tempfile::tempdir().unwrap();
        let test_results = tmp.path().join("TestResults");
        std::fs::create_dir(&test_results).unwrap();
        // A subdirectory exists but contains no coverage.cobertura.xml
        let run_dir = test_results.join("run-guid-abc123");
        std::fs::create_dir(&run_dir).unwrap();
        std::fs::write(run_dir.join("other.xml"), "<other/>").unwrap();

        let result = find_coverage_xml(tmp.path());
        assert!(result.is_err());
    }

    // Target: lines 178-193 — TestResults with a valid coverage.cobertura.xml
    #[test]
    fn bug_find_coverage_xml_finds_xml() {
        let tmp = tempfile::tempdir().unwrap();
        let test_results = tmp.path().join("TestResults");
        std::fs::create_dir(&test_results).unwrap();
        let run_dir = test_results.join("run-guid-abc123");
        std::fs::create_dir(&run_dir).unwrap();
        let xml_path = run_dir.join("coverage.cobertura.xml");
        std::fs::write(&xml_path, "<coverage/>").unwrap();

        let result = find_coverage_xml(tmp.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), xml_path);
    }

    // Target: lines 184-192 — newest XML wins when multiple runs exist
    #[test]
    fn bug_find_coverage_xml_picks_newest() {
        let tmp = tempfile::tempdir().unwrap();
        let test_results = tmp.path().join("TestResults");
        std::fs::create_dir(&test_results).unwrap();

        let run1 = test_results.join("run-001");
        std::fs::create_dir(&run1).unwrap();
        let xml1 = run1.join("coverage.cobertura.xml");
        std::fs::write(&xml1, "<coverage version='1'/>").unwrap();

        // Small sleep to ensure different mtime
        std::thread::sleep(std::time::Duration::from_millis(10));

        let run2 = test_results.join("run-002");
        std::fs::create_dir(&run2).unwrap();
        let xml2 = run2.join("coverage.cobertura.xml");
        std::fs::write(&xml2, "<coverage version='2'/>").unwrap();

        let result = find_coverage_xml(tmp.path()).unwrap();
        assert_eq!(result, xml2, "newest coverage XML should be returned");
    }

    // Target: lines 71-77 — derive_relative_path strips target_root prefix
    #[test]
    fn derive_relative_path_strips_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let full = tmp.path().join("src/Program.cs");
        let rel = derive_relative_path(full.to_str().unwrap(), tmp.path());
        assert_eq!(rel, "src/Program.cs");
    }

    // Target: derive_relative_path returns path as-is when no prefix match
    #[test]
    fn derive_relative_path_no_match_returns_original() {
        let rel = derive_relative_path("unrelated/path.cs", std::path::Path::new("/other/root"));
        assert_eq!(rel, "unrelated/path.cs");
    }

    // Target: build_traces_from_output with no matching lines
    #[test]
    fn build_traces_empty_output() {
        let branches = vec![BranchId::new(1, 1, 0, 0)];
        let traces =
            build_traces_from_output("  Running test suite...\n  Build succeeded.\n", &branches);
        assert!(traces.is_empty());
    }

    // Target: build_traces_from_output with empty branches slice
    #[test]
    fn build_traces_empty_branches() {
        let stdout = "  Passed SomeTest\n";
        let traces = build_traces_from_output(stdout, &[]);
        assert_eq!(traces.len(), 1);
        assert!(traces[0].branches.is_empty());
    }

    // Target: lines 108-158 — parse_csharp_coverage with absolute path matching target_root
    #[test]
    fn parse_csharp_coverage_absolute_path_stripped() {
        let tmp = tempfile::tempdir().unwrap();
        let abs_path = tmp.path().join("src/Foo.cs").to_string_lossy().to_string();
        let xml = format!(
            r#"<coverage><packages><package><classes>
<class filename="{abs_path}"><lines>
<line number="1" hits="1" />
</lines></class>
</classes></package></packages></coverage>"#
        );
        let (branches, file_paths) = parse_csharp_coverage(&xml, tmp.path());
        assert_eq!(branches.len(), 1);
        // Path should be stored as relative
        let stored = file_paths.values().next().unwrap();
        assert!(
            !stored.to_string_lossy().starts_with('/'),
            "path should be relative, got: {stored:?}"
        );
    }

    // Target: multiple classes, same file registered only once
    // NOTE: The parser is line-based; each element must be on its own line.
    // Elements crammed on one line are not parsed — this is by design (Cobertura
    // from dotnet always emits one element per line).
    #[test]
    fn parse_csharp_coverage_duplicate_file_deduplicated() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="Dup.cs">
  <lines>
    <line number="1" hits="1" />
  </lines>
</class>
<class filename="Dup.cs">
  <lines>
    <line number="2" hits="0" />
  </lines>
</class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_csharp_coverage(xml, tmp.path());
        assert_eq!(branches.len(), 2);
        assert_eq!(
            file_paths.len(),
            1,
            "same file should appear once in file_paths"
        );
    }

    // WRONG: The line-based parser cannot handle elements crammed on one line.
    // When <class ...><lines><line .../></lines></class> is all on one line,
    // the <line> element is never seen and branches are silently dropped.
    // Cobertura from `dotnet test` always emits one element per line, but
    // compact/minimised XML (e.g. from third-party tools) will be mis-parsed.
    #[test]
    fn bug_parse_csharp_coverage_inline_elements_silently_dropped() {
        // All on one line — parser sees only the <class> open tag, not the <line>
        let xml = "<coverage><packages><package><classes><class filename=\"Inline.cs\"><lines><line number=\"1\" hits=\"1\" /></lines></class></classes></package></packages></coverage>";
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(xml, tmp.path());
        // WRONG: branches is empty, but should be 1 for compact XML
        // This documents the known limitation of the line-based parser.
        assert_eq!(
            branches.len(),
            0,
            "documented limitation: inline XML elements are silently dropped"
        );
    }

    // Target: <class> without filename attribute — should not crash, just skips
    #[test]
    fn bug_parse_csharp_coverage_class_missing_filename() {
        let xml = r#"<coverage><packages><package><classes>
<class name="NoFilename"><lines><line number="1" hits="1" /></lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, file_paths) = parse_csharp_coverage(xml, tmp.path());
        // No filename -> no branches indexed, no crash
        assert!(branches.is_empty());
        assert!(file_paths.is_empty());
    }

    // -----------------------------------------------------------------------
    // extract_xml_attr — additional cases for apex-index variant
    // -----------------------------------------------------------------------

    // Target: extract_xml_attr (apex-index) — attribute needle not in tag returns None.
    #[test]
    fn extract_xml_attr_absent_returns_none() {
        let tag = r#"<line number="5" hits="2" />"#;
        assert_eq!(extract_xml_attr(tag, "name"), None);
    }

    // Target: extract_xml_attr — value contains special chars.
    #[test]
    fn extract_xml_attr_value_with_special_chars() {
        let tag = r#"<class filename="src/My Program.cs" />"#;
        assert_eq!(
            extract_xml_attr(tag, "filename"),
            Some("src/My Program.cs".to_string())
        );
    }

    // Target: extract_xml_attr — attribute occurs at the very end (no trailing content).
    #[test]
    fn extract_xml_attr_at_end_no_trailing() {
        let tag = r#"<line number="3" hits="0"/>"#;
        assert_eq!(extract_xml_attr(tag, "hits"), Some("0".to_string()));
    }

    // -----------------------------------------------------------------------
    // parse_csharp_coverage — hits=0 direction and in_class=false guard
    // -----------------------------------------------------------------------

    // Target: line 53 — hits=0 produces direction=1 in BranchId.
    #[test]
    fn parse_csharp_coverage_zero_hits_direction_one() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="Z.cs"><lines>
<line number="1" hits="0" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(xml, tmp.path());
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].direction, 1);
    }

    // Target: in_class reset after </class> — subsequent lines not indexed.
    #[test]
    fn parse_csharp_coverage_in_class_reset_on_close() {
        let xml = "<coverage><packages><package><classes>\n\
<class filename=\"A.cs\"><lines>\n\
<line number=\"1\" hits=\"1\" />\n\
</lines>\n\
</class>\n\
<line number=\"99\" hits=\"1\" />\n\
</classes></package></packages></coverage>";
        let tmp = tempfile::tempdir().unwrap();
        let (branches, _) = parse_csharp_coverage(xml, tmp.path());
        // Only line 1 inside the class should be indexed; line 99 after </class> is dropped.
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].line, 1);
    }

    // -----------------------------------------------------------------------
    // find_coverage_xml — multiple subdirs, picks newest
    // -----------------------------------------------------------------------

    // Target: lines 176-198 — find_coverage_xml picks newer of two valid XMLs.
    #[test]
    fn find_coverage_xml_two_runs_picks_newer() {
        let tmp = tempfile::tempdir().unwrap();
        let tr = tmp.path().join("TestResults");
        std::fs::create_dir(&tr).unwrap();

        let r1 = tr.join("run-a");
        std::fs::create_dir(&r1).unwrap();
        let xml1 = r1.join("coverage.cobertura.xml");
        std::fs::write(&xml1, "<cov1/>").unwrap();

        // Ensure mtime difference
        std::thread::sleep(std::time::Duration::from_millis(15));

        let r2 = tr.join("run-b");
        std::fs::create_dir(&r2).unwrap();
        let xml2 = r2.join("coverage.cobertura.xml");
        std::fs::write(&xml2, "<cov2/>").unwrap();

        let found = find_coverage_xml(tmp.path()).unwrap();
        assert_eq!(found, xml2);
    }

    // Target: find_coverage_xml — subdirectory that is not a dir (a file) is skipped.
    #[test]
    fn find_coverage_xml_file_entry_in_test_results_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let tr = tmp.path().join("TestResults");
        std::fs::create_dir(&tr).unwrap();

        // A regular file in TestResults (not a subdir) — is_dir() false → skipped.
        std::fs::write(tr.join("notadir.xml"), "<x/>").unwrap();

        let result = find_coverage_xml(tmp.path());
        assert!(result.is_err(), "no subdir coverage XML should be an error");
    }

    // -----------------------------------------------------------------------
    // build_traces_from_output — branches propagated
    // -----------------------------------------------------------------------

    // Target: lines 87-100 — branches slice is correctly cloned into each trace.
    #[test]
    fn build_traces_branches_correctly_propagated() {
        let branches = vec![BranchId::new(10, 5, 0, 0), BranchId::new(10, 7, 0, 1)];
        let stdout = "  Passed MyTest.Run\n  Failed MyTest.Fail\n";
        let traces = build_traces_from_output(stdout, &branches);
        assert_eq!(traces.len(), 2);
        for trace in &traces {
            assert_eq!(trace.branches.len(), 2);
        }
    }
}
