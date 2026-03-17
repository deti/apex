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
};
use tracing::{debug, info, warn};

pub struct CSharpInstrumentor<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl CSharpInstrumentor {
    pub fn new() -> Self {
        CSharpInstrumentor {
            runner: RealCommandRunner,
        }
    }
}

impl Default for CSharpInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: CommandRunner> CSharpInstrumentor<R> {
    pub fn with_runner(runner: R) -> Self {
        CSharpInstrumentor { runner }
    }
}

/// Parse Cobertura XML coverage into branch entries.
///
/// Cobertura XML format (simplified):
/// ```xml
/// <coverage>
///   <packages>
///     <package>
///       <classes>
///         <class filename="...">
///           <lines>
///             <line number="10" hits="3" />
///           </lines>
///         </class>
///       </classes>
///     </package>
///   </packages>
/// </coverage>
/// ```
pub fn parse_cobertura_xml(
    content: &str,
    target_root: &Path,
) -> (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut all_branches = Vec::new();
    let mut executed_branches = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    // Simple line-based XML parsing — sufficient for Cobertura format.
    let mut current_file: Option<String> = None;
    let mut current_file_id: u64 = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("<class ") {
            if let Some(filename) = extract_xml_attr(trimmed, "filename") {
                let rel_path = derive_relative_path(&filename, target_root);
                let file_id = fnv1a_hash(&rel_path);
                file_paths
                    .entry(file_id)
                    .or_insert_with(|| PathBuf::from(&rel_path));
                current_file = Some(rel_path);
                current_file_id = file_id;
            }
        } else if trimmed.starts_with("</class>") {
            current_file = None;
        } else if trimmed.starts_with("<line ") && current_file.is_some() {
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

            let branch_covered = BranchId::new(current_file_id, line_num, 0, 0);
            let branch_uncovered = BranchId::new(current_file_id, line_num, 0, 1);

            all_branches.push(branch_covered.clone());
            all_branches.push(branch_uncovered.clone());

            if hits > 0 {
                executed_branches.push(branch_covered);
            } else {
                executed_branches.push(branch_uncovered);
            }
        }
    }

    (all_branches, executed_branches, file_paths)
}

/// Extract an XML attribute value from a tag string.
fn extract_xml_attr(tag: &str, attr_name: &str) -> Option<String> {
    let needle = format!(" {attr_name}=\"");
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

#[async_trait]
impl<R: CommandRunner> Instrumentor for CSharpInstrumentor<R> {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        let target_root = &target.root;
        info!(target = %target_root.display(), "running C# coverage instrumentation");

        // Run: dotnet test --collect:"XPlat Code Coverage"
        let spec =
            CommandSpec::new("dotnet", target_root).args(["test", "--collect:XPlat Code Coverage"]);

        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("dotnet test --collect: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(exit = output.exit_code, %stderr, "dotnet test --collect returned non-zero");
        }

        // Find the Cobertura coverage.xml in TestResults
        let coverage_xml = find_coverage_xml(target_root)?;

        let content = std::fs::read_to_string(&coverage_xml).map_err(|e| {
            ApexError::Instrumentation(format!("failed to read {}: {e}", coverage_xml.display()))
        })?;

        let (all_branches, executed_branches, file_paths) =
            parse_cobertura_xml(&content, target_root);

        debug!(
            total = all_branches.len(),
            executed = executed_branches.len(),
            "parsed C# coverage"
        );

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids: all_branches,
            executed_branch_ids: executed_branches,
            file_paths,
            work_dir: target_root.to_path_buf(),
        })
    }

    fn branch_ids(&self) -> &[BranchId] {
        &[]
    }
}

/// Find the most recent coverage.cobertura.xml under TestResults.
fn find_coverage_xml(target_root: &Path) -> Result<PathBuf> {
    let test_results = target_root.join("TestResults");
    if !test_results.exists() {
        return Err(ApexError::Instrumentation(
            "TestResults directory not found".to_string(),
        ));
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
        .ok_or_else(|| ApexError::Instrumentation("coverage.cobertura.xml not found".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_COBERTURA: &str = r#"<?xml version="1.0" encoding="utf-8"?>
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
        <class filename="Helper.cs">
          <lines>
            <line number="5" hits="0" />
          </lines>
        </class>
      </classes>
    </package>
  </packages>
</coverage>"#;

    #[test]
    fn parse_cobertura_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_cobertura_xml(FIXTURE_COBERTURA, tmp.path());

        // 4 lines -> 4 * 2 directions = 8 branches total
        assert_eq!(all.len(), 8);
        assert_eq!(executed.len(), 4);
        assert_eq!(file_paths.len(), 2);
    }

    #[test]
    fn parse_cobertura_counts_covered() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, executed, _) = parse_cobertura_xml(FIXTURE_COBERTURA, tmp.path());

        let dirs: Vec<u8> = executed.iter().map(|b| b.direction).collect();
        // hits=3 -> dir 0, hits=0 -> dir 1, hits=1 -> dir 0, hits=0 -> dir 1
        assert_eq!(dirs, vec![0, 1, 0, 1]);
    }

    #[test]
    fn parse_cobertura_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_cobertura_xml(
            "<coverage><packages><package><classes></classes></package></packages></coverage>",
            tmp.path(),
        );
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    #[test]
    fn parse_cobertura_line_number() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="Foo.cs"><lines>
<line number="42" hits="5" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_cobertura_xml(xml, tmp.path());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].line, 42);
    }

    #[test]
    fn extract_xml_attr_works() {
        let tag = r#"<class filename="Program.cs" name="Program">"#;
        assert_eq!(
            extract_xml_attr(tag, "filename"),
            Some("Program.cs".to_string())
        );
        assert_eq!(extract_xml_attr(tag, "name"), Some("Program".to_string()));
        assert_eq!(extract_xml_attr(tag, "missing"), None);
    }

    #[test]
    fn parse_cobertura_file_id_deterministic() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="Foo.cs"><lines>
<line number="1" hits="1" />
<line number="2" hits="0" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_cobertura_xml(xml, tmp.path());
        // Same file -> same file_id
        assert_eq!(all[0].file_id, all[2].file_id);
    }

    // --- New tests targeting uncovered regions ---

    // Target: derive_relative_path — strip_prefix succeeds path
    #[test]
    fn derive_relative_path_strips_prefix() {
        let root = Path::new("/project/src");
        let result = derive_relative_path("/project/src/Foo.cs", root);
        assert_eq!(result, "Foo.cs");
    }

    // Target: derive_relative_path — no prefix match, returns original
    #[test]
    fn derive_relative_path_no_match_returns_original() {
        let root = Path::new("/project/src");
        let result = derive_relative_path("Program.cs", root);
        assert_eq!(result, "Program.cs");
    }

    // Target: derive_relative_path — absolute path that does not share prefix
    #[test]
    fn derive_relative_path_unrelated_absolute() {
        let root = Path::new("/project/src");
        let result = derive_relative_path("/other/path/Foo.cs", root);
        assert_eq!(result, "/other/path/Foo.cs");
    }

    // Target: extract_xml_attr — attribute with empty value
    #[test]
    fn extract_xml_attr_empty_value() {
        let tag = r#"<line number="" hits="0" />"#;
        assert_eq!(extract_xml_attr(tag, "number"), Some(String::new()));
    }

    // Target: extract_xml_attr — attribute is last in tag with no trailing space
    #[test]
    fn extract_xml_attr_last_attribute() {
        let tag = r#"<class filename="X.cs" name="X">"#;
        assert_eq!(extract_xml_attr(tag, "name"), Some("X".to_string()));
    }

    // Target: parse_cobertura_xml — line tag outside any class context is ignored
    #[test]
    fn parse_cobertura_line_outside_class_ignored() {
        let xml = r#"<coverage>
<line number="1" hits="5" />
<packages><package><classes>
<class filename="A.cs"><lines>
<line number="2" hits="1" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_cobertura_xml(xml, tmp.path());
        // Only the line inside the class should be counted
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].line, 2);
    }

    // Target: parse_cobertura_xml — </class> resets current_file;
    // lines emitted after </class> and before next <class> are ignored.
    // Note: </class> must appear on its own line for the line-based parser to detect it.
    #[test]
    fn parse_cobertura_lines_after_class_close_ignored() {
        let xml = "<coverage><packages><package><classes>\n\
<class filename=\"A.cs\"><lines>\n\
<line number=\"1\" hits=\"1\" />\n\
</lines>\n\
</class>\n\
<line number=\"99\" hits=\"1\" />\n\
<class filename=\"B.cs\"><lines>\n\
<line number=\"2\" hits=\"0\" />\n\
</lines>\n\
</class>\n\
</classes></package></packages></coverage>";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, file_paths) = parse_cobertura_xml(xml, tmp.path());
        // 2 valid lines (one per class), orphan line between classes is ignored
        assert_eq!(all.len(), 4);
        assert_eq!(file_paths.len(), 2);
    }

    // Target: parse_cobertura_xml — malformed line number (not a u32) is skipped
    #[test]
    fn parse_cobertura_skips_malformed_line_number() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="A.cs"><lines>
<line number="abc" hits="1" />
<line number="5" hits="2" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_cobertura_xml(xml, tmp.path());
        // Only line 5 is valid
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].line, 5);
    }

    // Target: parse_cobertura_xml — malformed hits (not a u32) is skipped
    #[test]
    fn parse_cobertura_skips_malformed_hits() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="A.cs"><lines>
<line number="3" hits="NaN" />
<line number="4" hits="1" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_cobertura_xml(xml, tmp.path());
        // Only line 4 is valid
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].line, 4);
    }

    // Target: parse_cobertura_xml — <line> with missing number attribute is skipped
    #[test]
    fn parse_cobertura_skips_missing_number_attr() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="A.cs"><lines>
<line hits="1" />
<line number="7" hits="1" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_cobertura_xml(xml, tmp.path());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].line, 7);
    }

    // Target: parse_cobertura_xml — <line> with missing hits attribute is skipped
    #[test]
    fn parse_cobertura_skips_missing_hits_attr() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="A.cs"><lines>
<line number="3" />
<line number="4" hits="0" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_cobertura_xml(xml, tmp.path());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].line, 4);
    }

    // Target: parse_cobertura_xml — same filename in two class entries shares file_id
    #[test]
    fn parse_cobertura_duplicate_filename_same_file_id() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="Shared.cs"><lines>
<line number="1" hits="1" />
</lines></class>
<class filename="Shared.cs"><lines>
<line number="2" hits="0" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, file_paths) = parse_cobertura_xml(xml, tmp.path());
        // Both branches belong to the same file_id
        assert_eq!(all[0].file_id, all[2].file_id);
        // file_paths deduplication: only one entry
        assert_eq!(file_paths.len(), 1);
    }

    // Target: parse_cobertura_xml — hits=0 produces direction=1 in executed
    #[test]
    fn parse_cobertura_zero_hits_direction_one() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="X.cs"><lines>
<line number="1" hits="0" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (_, executed, _) = parse_cobertura_xml(xml, tmp.path());
        assert_eq!(executed.len(), 1);
        assert_eq!(executed[0].direction, 1);
    }

    // Target: parse_cobertura_xml — pure whitespace/blank input
    #[test]
    fn parse_cobertura_blank_input() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, file_paths) = parse_cobertura_xml("   \n\t\n  ", tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(file_paths.is_empty());
    }

    // Target: parse_cobertura_xml — unicode filename
    #[test]
    fn parse_cobertura_unicode_filename() {
        let xml = "<coverage><packages><package><classes>\n<class filename=\"\u{4e2d}\u{6587}/\u{6587}\u{4ef6}.cs\"><lines>\n<line number=\"1\" hits=\"1\" />\n</lines></class>\n</classes></package></packages></coverage>";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, file_paths) = parse_cobertura_xml(xml, tmp.path());
        assert_eq!(all.len(), 2);
        assert_eq!(file_paths.len(), 1);
    }

    // Target: extract_xml_attr — no space before attribute name returns None
    #[test]
    fn extract_xml_attr_no_leading_space_returns_none() {
        // The attribute search requires a leading space
        let tag = r#"<linefilename="no-space.cs">"#;
        assert_eq!(extract_xml_attr(tag, "filename"), None);
    }

    // -----------------------------------------------------------------------
    // parse_cobertura_xml — current_file reset on </class> (line 82-83)
    // -----------------------------------------------------------------------

    // Target: line 83 — current_file set to None on </class>; subsequent <line> outside
    // a class is not indexed (current_file.is_some() guard on line 84).
    #[test]
    fn parse_cobertura_current_file_cleared_after_class_close() {
        let xml = "<coverage><packages><package><classes>\n\
<class filename=\"A.cs\"><lines>\n\
<line number=\"1\" hits=\"1\" />\n\
</lines>\n\
</class>\n\
<line number=\"99\" hits=\"5\" />\n\
<class filename=\"B.cs\"><lines>\n\
<line number=\"2\" hits=\"0\" />\n\
</lines>\n\
</class>\n\
</classes></package></packages></coverage>";
        let tmp = tempfile::tempdir().unwrap();
        let (all, _, _) = parse_cobertura_xml(xml, tmp.path());
        // Lines 1 and 2 from A.cs and B.cs — each produces 2 branches (covered+uncovered)
        // Line 99 is outside any class and must be ignored.
        assert_eq!(all.len(), 4, "lines outside <class> must be ignored");
        let lines: Vec<u32> = all.iter().map(|b| b.line).collect();
        assert!(!lines.contains(&99), "orphan line 99 must not be indexed");
    }

    // -----------------------------------------------------------------------
    // parse_cobertura_xml — all_branches always paired (lines 98-108)
    // -----------------------------------------------------------------------

    // Target: lines 98-108 — each valid <line> always pushes exactly 2 BranchIds
    // (branch_covered dir=0 and branch_uncovered dir=1) to all_branches.
    #[test]
    fn parse_cobertura_all_branches_always_paired() {
        let xml = r#"<coverage><packages><package><classes>
<class filename="P.cs"><lines>
<line number="5" hits="10" />
<line number="6" hits="0" />
</lines></class>
</classes></package></packages></coverage>"#;
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, _) = parse_cobertura_xml(xml, tmp.path());
        // 2 lines → 4 branches (2 per line: dir=0 and dir=1)
        assert_eq!(all.len(), 4);
        // Directions: [0,1] for line 5 and [0,1] for line 6 (order may vary slightly)
        let dirs: Vec<u8> = all.iter().map(|b| b.direction).collect();
        assert_eq!(dirs.iter().filter(|&&d| d == 0).count(), 2);
        assert_eq!(dirs.iter().filter(|&&d| d == 1).count(), 2);
        // executed: hits>0 → dir=0, hits=0 → dir=1
        assert_eq!(executed.len(), 2);
        assert!(executed.iter().any(|b| b.direction == 0 && b.line == 5));
        assert!(executed.iter().any(|b| b.direction == 1 && b.line == 6));
    }

    // -----------------------------------------------------------------------
    // find_coverage_xml — file in root TestResults (not a subdir) is skipped
    // -----------------------------------------------------------------------

    // Target: lines 197-210 — path.is_dir() guard skips regular files in TestResults.
    #[test]
    fn find_coverage_xml_skips_files_in_test_results() {
        let tmp = tempfile::tempdir().unwrap();
        let tr = tmp.path().join("TestResults");
        std::fs::create_dir(&tr).unwrap();
        // Place a coverage.cobertura.xml directly in TestResults (not in a subdir).
        std::fs::write(tr.join("coverage.cobertura.xml"), "<cov/>").unwrap();
        // It's a file, not a subdir, so is_dir() = false → skipped.
        let result = find_coverage_xml(tmp.path());
        assert!(result.is_err(), "file at TestResults root must be skipped");
    }

    // Target: lines 200-209 — happy path: valid coverage XML found under subdir.
    #[test]
    fn find_coverage_xml_happy_path() {
        let tmp = tempfile::tempdir().unwrap();
        let tr = tmp.path().join("TestResults");
        std::fs::create_dir(&tr).unwrap();
        let run_dir = tr.join("guid-run-001");
        std::fs::create_dir(&run_dir).unwrap();
        let xml = run_dir.join("coverage.cobertura.xml");
        std::fs::write(&xml, "<coverage/>").unwrap();

        let result = find_coverage_xml(tmp.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), xml);
    }

    // Target: error when TestResults dir missing (line 187-190).
    #[test]
    fn find_coverage_xml_no_test_results_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let err = find_coverage_xml(tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("TestResults"),
            "error must mention TestResults: {err}"
        );
    }

    // Target: error when TestResults has no XML (line 213-215).
    #[test]
    fn find_coverage_xml_empty_test_results_errors() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("TestResults")).unwrap();
        let err = find_coverage_xml(tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("coverage.cobertura.xml"),
            "error must mention the xml filename: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // derive_relative_path — both branches (strip_prefix success and fallback)
    // -----------------------------------------------------------------------

    // Target: lines 127-128 — strip_prefix succeeds.
    #[test]
    fn derive_relative_path_strips_prefix_new() {
        let root = std::path::Path::new("/project/src");
        let result = derive_relative_path("/project/src/Foo.cs", root);
        assert_eq!(result, "Foo.cs");
    }

    // Target: lines 130 — strip_prefix fails, returns original.
    #[test]
    fn derive_relative_path_fallback_to_original() {
        let root = std::path::Path::new("/project/src");
        let result = derive_relative_path("relative/file.cs", root);
        assert_eq!(result, "relative/file.cs");
    }

    // -----------------------------------------------------------------------
    // extract_xml_attr — more edge cases
    // -----------------------------------------------------------------------

    // Target: extract_xml_attr — attribute value with space inside.
    #[test]
    fn extract_xml_attr_value_with_space() {
        let tag = r#" <class filename="My Files/Test.cs" name="Test">"#;
        assert_eq!(
            extract_xml_attr(tag, "filename"),
            Some("My Files/Test.cs".to_string())
        );
    }

    // Target: extract_xml_attr — empty value.
    #[test]
    fn extract_xml_attr_empty_value_new() {
        let tag = r#"<line number="" hits="0" />"#;
        assert_eq!(extract_xml_attr(tag, "number"), Some(String::new()));
    }
}
