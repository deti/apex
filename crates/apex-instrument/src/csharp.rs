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
        let spec = CommandSpec::new("dotnet", target_root)
            .args(["test", "--collect:XPlat Code Coverage"]);

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
            ApexError::Instrumentation(format!(
                "failed to read {}: {e}",
                coverage_xml.display()
            ))
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
        assert_eq!(
            extract_xml_attr(tag, "name"),
            Some("Program".to_string())
        );
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
}
