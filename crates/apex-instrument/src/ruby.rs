use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    hash::fnv1a_hash,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use async_trait::async_trait;
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tracing::{debug, info, warn};

pub struct RubyInstrumentor {
    runner: Arc<dyn CommandRunner>,
}

impl RubyInstrumentor {
    pub fn new() -> Self {
        RubyInstrumentor { runner: Arc::new(RealCommandRunner) }
    }
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        RubyInstrumentor { runner }
    }
}

impl Default for RubyInstrumentor {
    fn default() -> Self { Self::new() }
}

// SimpleCov JSON format
#[derive(Debug, Deserialize)]
struct SimpleCovJson {
    coverage: HashMap<String, FileCoverage>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FileCoverage {
    Lines(LineCoverage),
    Detailed(DetailedCoverage),
}

#[derive(Debug, Deserialize)]
struct LineCoverage {
    lines: Vec<Option<u64>>,
}

#[derive(Debug, Deserialize)]
struct DetailedCoverage {
    lines: Vec<Option<u64>>,
}

/// Parse SimpleCov JSON output into branch IDs.
pub fn parse_simplecov_json(json: &str) -> (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut all_branches = Vec::new();
    let mut executed = Vec::new();
    let mut file_paths = HashMap::new();

    let data: SimpleCovJson = match serde_json::from_str(json) {
        Ok(d) => d,
        Err(e) => {
            warn!(error = %e, "failed to parse SimpleCov JSON");
            return (all_branches, executed, file_paths);
        }
    };

    for (file_path, coverage) in &data.coverage {
        let file_id = fnv1a_hash(file_path);
        file_paths.insert(file_id, PathBuf::from(file_path));

        let lines = match coverage {
            FileCoverage::Lines(lc) => &lc.lines,
            FileCoverage::Detailed(dc) => &dc.lines,
        };

        for (i, count) in lines.iter().enumerate() {
            if let Some(c) = count {
                let line = (i + 1) as u32;
                let branch = BranchId::new(file_id, line, 0, 0);
                all_branches.push(branch.clone());
                if *c > 0 {
                    executed.push(branch);
                }
            }
            // None = non-executable line, skip
        }
    }

    (all_branches, executed, file_paths)
}

#[async_trait]
impl Instrumentor for RubyInstrumentor {
    fn branch_ids(&self) -> &[BranchId] { &[] }

    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        let target_path = &target.root;
        info!(target = %target_path.display(), "instrumenting Ruby project with SimpleCov");

        // Run tests with SimpleCov via RUBYOPT
        let spec = CommandSpec::new("ruby", target_path)
            .args(["-e", "require 'simplecov'; SimpleCov.start; SimpleCov.formatter = SimpleCov::Formatter::JSONFormatter; load 'Rakefile'; Rake::Task[:test].invoke"])
            .env("RUBYOPT", "-rsimplecov");

        let _output = self.runner.run_command(&spec).await
            .map_err(|e| ApexError::Instrumentation(format!("ruby simplecov: {e}")))?;

        // Try to read SimpleCov JSON output
        let json_path = target_path.join("coverage").join(".resultset.json");
        let alt_path = target_path.join("coverage").join("coverage.json");

        let json_content = if json_path.exists() {
            std::fs::read_to_string(&json_path)
                .map_err(|e| ApexError::Instrumentation(e.to_string()))?
        } else if alt_path.exists() {
            std::fs::read_to_string(&alt_path)
                .map_err(|e| ApexError::Instrumentation(e.to_string()))?
        } else {
            debug!("no SimpleCov JSON found, returning empty instrumentation");
            return Ok(InstrumentedTarget {
                target: target.clone(),
                branch_ids: Vec::new(),
                executed_branch_ids: Vec::new(),
                file_paths: HashMap::new(),
                work_dir: target_path.to_path_buf(),
            });
        };

        let (branch_ids, executed_branch_ids, file_paths) = parse_simplecov_json(&json_content);

        info!(
            branches = branch_ids.len(),
            executed = executed_branch_ids.len(),
            "Ruby instrumentation complete"
        );

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids,
            executed_branch_ids,
            file_paths,
            work_dir: target_path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simplecov_basic() {
        let json = r#"{"coverage":{"app/models/user.rb":{"lines":[null,1,1,0,null,1]}}}"#;
        let (all, exec, files) = parse_simplecov_json(json);
        assert_eq!(all.len(), 4); // 4 executable lines (non-null)
        assert_eq!(exec.len(), 3); // 3 executed (count > 0)
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn parse_simplecov_multiple_files() {
        let json = r#"{"coverage":{"a.rb":{"lines":[1,0]},"b.rb":{"lines":[1,1]}}}"#;
        let (all, exec, files) = parse_simplecov_json(json);
        assert_eq!(all.len(), 4);
        assert_eq!(exec.len(), 3);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn parse_simplecov_empty() {
        let json = r#"{"coverage":{}}"#;
        let (all, exec, files) = parse_simplecov_json(json);
        assert!(all.is_empty());
        assert!(exec.is_empty());
        assert!(files.is_empty());
    }

    #[test]
    fn parse_simplecov_invalid_json() {
        let (all, _, _) = parse_simplecov_json("not json");
        assert!(all.is_empty());
    }

    #[test]
    fn parse_simplecov_all_null_lines() {
        let json = r#"{"coverage":{"x.rb":{"lines":[null,null,null]}}}"#;
        let (all, _, _) = parse_simplecov_json(json);
        assert!(all.is_empty()); // All non-executable
    }

    #[test]
    fn parse_simplecov_file_id_deterministic() {
        let json = r#"{"coverage":{"app/user.rb":{"lines":[1]}}}"#;
        let (a1, _, _) = parse_simplecov_json(json);
        let (a2, _, _) = parse_simplecov_json(json);
        assert_eq!(a1[0].file_id, a2[0].file_id);
    }
}
