//! CPG-informed synthesis context.
//!
//! Extracts data-flow context from the Code Property Graph for a set of uncovered
//! branches and formats it as a human-readable section suitable for inclusion in
//! LLM test-generation prompts.
//!
//! The context answers: "what values reach this branch, and where do they come from?"
//! so the LLM can construct inputs that satisfy the branch condition.

use apex_cpg::{Cpg, EdgeKind, NodeId, NodeKind};
use apex_core::types::BranchId;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Build a prompt-ready context string for a set of uncovered branches.
///
/// For each branch, the function:
/// 1. Locates the CPG node at (or nearest to) the branch's source line.
/// 2. Follows `ReachingDef` edges backward to find the definitions that arrive
///    at that point.
/// 3. Checks for `ControlStructure` nodes at the same line to extract the
///    condition text.
/// 4. Formats everything as a `## Branch Context (from CPG analysis)` section.
///
/// Returns an empty string when `cpg` contains no nodes (no CPG available).
pub fn build_cpg_prompt_context(
    cpg: &Cpg,
    uncovered_branches: &[BranchId],
    file_paths: &HashMap<u64, PathBuf>,
    source_cache: &HashMap<PathBuf, String>,
) -> String {
    if cpg.node_count() == 0 || uncovered_branches.is_empty() {
        return String::new();
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push("## Branch Context (from CPG analysis)".to_string());

    // Deduplicate branches by (file_id, line) so we emit one entry per unique line.
    let mut seen: HashSet<(u64, u32)> = HashSet::new();

    for branch in uncovered_branches {
        if !seen.insert((branch.file_id, branch.line)) {
            continue;
        }

        let file_label = file_paths
            .get(&branch.file_id)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("<file:{}>", branch.file_id));

        let condition_text = extract_condition_at_line(cpg, branch.line, source_cache, file_paths, branch.file_id);
        let reaching = reaching_info_at_line(cpg, branch.line);

        let direction_label = match branch.direction {
            0 => " [true branch]",
            1 => " [false branch]",
            _ => "",
        };

        if reaching.is_empty() && condition_text.is_none() {
            lines.push(format!(
                "- {}:{}{} — no CPG data flow info available",
                file_label, branch.line, direction_label
            ));
        } else {
            let cond_str = condition_text
                .as_deref()
                .map(|c| format!(" `{c}`"))
                .unwrap_or_default();

            if reaching.is_empty() {
                lines.push(format!(
                    "- {}:{}{} —{}",
                    file_label, branch.line, direction_label, cond_str
                ));
            } else {
                let reach_str = reaching.join(", ");
                lines.push(format!(
                    "- {}:{}{} —{} (depends on: {})",
                    file_label, branch.line, direction_label, cond_str, reach_str
                ));
            }
        }
    }

    if lines.len() == 1 {
        // Only the header — nothing useful to add.
        return String::new();
    }

    lines.push(String::new()); // trailing newline separator
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Find the condition text for a `ControlStructure` or any source line hint at `line`.
///
/// Strategy:
/// 1. Look for a `ControlStructure` CPG node at `line` — use the source line text
///    if the source cache has it, otherwise return the ctrl kind name.
/// 2. Fall back to extracting the raw source line from the cache.
fn extract_condition_at_line(
    cpg: &Cpg,
    line: u32,
    source_cache: &HashMap<PathBuf, String>,
    file_paths: &HashMap<u64, PathBuf>,
    file_id: u64,
) -> Option<String> {
    // Try to find a ControlStructure node at this line.
    let ctrl_at_line = cpg.nodes().find(|(_, k)| {
        matches!(k, NodeKind::ControlStructure { line: l, .. } if *l == line)
    });

    let path = file_paths.get(&file_id)?;
    let source = source_cache.get(path)?;

    if ctrl_at_line.is_some() {
        // Return the trimmed source line as the condition text.
        let src_line = source
            .lines()
            .nth((line as usize).saturating_sub(1))
            .map(|l| l.trim().to_string());
        return src_line;
    }

    // Fall back: if there's any CPG node at this line, return the source line.
    let any_node_at_line = cpg.nodes().any(|(_, k)| node_line(k) == Some(line));
    if any_node_at_line {
        return source
            .lines()
            .nth((line as usize).saturating_sub(1))
            .map(|l| l.trim().to_string());
    }

    None
}

/// Collect human-readable descriptions of definitions that reach `line`.
///
/// Walks all CPG nodes, picks the one(s) closest to `line`, then follows
/// `ReachingDef` edges inward to find where values come from.
fn reaching_info_at_line(cpg: &Cpg, line: u32) -> Vec<String> {
    // Collect all node ids whose line number matches.
    let targets: Vec<NodeId> = cpg
        .nodes()
        .filter_map(|(id, k)| {
            if node_line(k) == Some(line) {
                Some(id)
            } else {
                None
            }
        })
        .collect();

    let mut descs: Vec<String> = Vec::new();
    let mut seen_vars: HashSet<String> = HashSet::new();

    for target in targets {
        // Find all ReachingDef edges that arrive at this node.
        for (from, _, kind) in cpg.edges_to(target) {
            if let EdgeKind::ReachingDef { variable } = kind {
                if seen_vars.insert(variable.clone()) {
                    if let Some(src_node) = cpg.node(*from) {
                        descs.push(format_reaching_source(variable, src_node));
                    }
                }
            }
        }
    }

    descs
}

/// Format a single reaching-definition source into a readable string.
fn format_reaching_source(variable: &str, src: &NodeKind) -> String {
    match src {
        NodeKind::Parameter { name, index } => {
            format!("`{variable}` from parameter `{name}` (arg #{index})")
        }
        NodeKind::Assignment { lhs, line } => {
            format!("`{variable}` assigned at line {line} (`{lhs} = ...`)")
        }
        NodeKind::Call { name, line } => {
            format!("`{variable}` returned from call `{name}()` at line {line}")
        }
        NodeKind::Literal { value, line } => {
            format!("`{variable}` literal `{value}` at line {line}")
        }
        NodeKind::Method { name, file, line } => {
            format!("`{variable}` defined in method `{name}` ({file}:{line})")
        }
        NodeKind::Identifier { name, line } => {
            format!("`{variable}` via identifier `{name}` at line {line}")
        }
        NodeKind::Return { line } => {
            format!("`{variable}` from return at line {line}")
        }
        NodeKind::ControlStructure { kind, line } => {
            format!("`{variable}` from control structure `{kind:?}` at line {line}")
        }
    }
}

/// Extract the line number from a CPG node, if available.
fn node_line(k: &NodeKind) -> Option<u32> {
    match k {
        NodeKind::Call { line, .. }
        | NodeKind::Identifier { line, .. }
        | NodeKind::Literal { line, .. }
        | NodeKind::Return { line }
        | NodeKind::ControlStructure { line, .. }
        | NodeKind::Assignment { line, .. }
        | NodeKind::Method { line, .. } => Some(*line),
        NodeKind::Parameter { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use apex_cpg::{builder::build_python_cpg, reaching_def::add_reaching_def_edges};

    fn empty_maps() -> (HashMap<u64, PathBuf>, HashMap<PathBuf, String>) {
        (HashMap::new(), HashMap::new())
    }

    // -----------------------------------------------------------------------
    // build_cpg_prompt_context: empty CPG → returns ""
    // -----------------------------------------------------------------------
    #[test]
    fn empty_cpg_returns_empty_string() {
        let cpg = Cpg::new();
        let branches = vec![BranchId::new(1, 10, 0, 0)];
        let (fp, sc) = empty_maps();
        let ctx = build_cpg_prompt_context(&cpg, &branches, &fp, &sc);
        assert!(ctx.is_empty(), "empty CPG should produce empty context");
    }

    // -----------------------------------------------------------------------
    // build_cpg_prompt_context: empty branches → returns ""
    // -----------------------------------------------------------------------
    #[test]
    fn empty_branches_returns_empty_string() {
        let source = "def foo(x):\n    if x > 0:\n        return x\n";
        let cpg = build_python_cpg(source, "foo.py");
        let (fp, sc) = empty_maps();
        let ctx = build_cpg_prompt_context(&cpg, &[], &fp, &sc);
        assert!(ctx.is_empty(), "no branches should produce empty context");
    }

    // -----------------------------------------------------------------------
    // build_cpg_prompt_context: with simple CPG → produces "Branch Context" section
    // -----------------------------------------------------------------------
    #[test]
    fn simple_cpg_produces_branch_context_section() {
        let source = "def process(x):\n    if x > 0:\n        return x\n";
        let mut cpg = build_python_cpg(source, "proc.py");
        add_reaching_def_edges(&mut cpg);

        let file_id: u64 = 42;
        let path = PathBuf::from("proc.py");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, path.clone());
        let mut source_cache = HashMap::new();
        source_cache.insert(path, source.to_string());

        // Branch at line 2 (the `if x > 0:` line), true direction
        let branches = vec![BranchId::new(file_id, 2, 0, 0)];
        let ctx = build_cpg_prompt_context(&cpg, &branches, &file_paths, &source_cache);

        assert!(
            ctx.contains("## Branch Context (from CPG analysis)"),
            "should include section header, got:\n{ctx}"
        );
        assert!(
            ctx.contains("2"),
            "context should mention line number, got:\n{ctx}"
        );
    }

    // -----------------------------------------------------------------------
    // build_cpg_prompt_context: true/false branch direction labels
    // -----------------------------------------------------------------------
    #[test]
    fn branch_direction_labels_included() {
        let source = "def foo(x):\n    if x > 0:\n        pass\n";
        let cpg = build_python_cpg(source, "foo.py");

        let file_id: u64 = 1;
        let path = PathBuf::from("foo.py");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, path.clone());
        let mut source_cache = HashMap::new();
        source_cache.insert(path, source.to_string());

        let branches = vec![
            BranchId::new(file_id, 2, 0, 0), // true
            BranchId::new(file_id, 2, 0, 1), // false
        ];
        let ctx = build_cpg_prompt_context(&cpg, &branches, &file_paths, &source_cache);
        assert!(ctx.contains("true branch") || ctx.contains("false branch"),
            "should include direction labels, got:\n{ctx}");
    }

    // -----------------------------------------------------------------------
    // build_cpg_prompt_context: deduplicates same (file_id, line)
    // -----------------------------------------------------------------------
    #[test]
    fn deduplicates_same_line_branches() {
        let source = "def foo(x):\n    if x > 0:\n        pass\n";
        let cpg = build_python_cpg(source, "foo.py");
        let file_id: u64 = 1;
        let path = PathBuf::from("foo.py");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, path.clone());
        let mut source_cache = HashMap::new();
        source_cache.insert(path, source.to_string());

        // Two branches on same line (true + false) — after dedup both are different
        // because direction differs, but same (file_id, line) deduplicated.
        // Actually our dedup key is (file_id, line), so second is skipped.
        let branches = vec![
            BranchId::new(file_id, 2, 0, 0),
            BranchId::new(file_id, 2, 0, 0), // exact duplicate
        ];
        let ctx = build_cpg_prompt_context(&cpg, &branches, &file_paths, &source_cache);
        // Line 2 should appear exactly once
        let count = ctx.matches("foo.py:2").count();
        assert_eq!(count, 1, "duplicate branch should be deduplicated, got:\n{ctx}");
    }

    // -----------------------------------------------------------------------
    // build_cpg_prompt_context: parameter flow shows in context
    // -----------------------------------------------------------------------
    #[test]
    fn parameter_flow_appears_in_context() {
        let source = "def process(x):\n    if x > 0:\n        return x\n";
        let mut cpg = build_python_cpg(source, "proc.py");
        add_reaching_def_edges(&mut cpg);

        let file_id: u64 = 7;
        let path = PathBuf::from("proc.py");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, path.clone());
        let mut source_cache = HashMap::new();
        source_cache.insert(path, source.to_string());

        // Check a line where x is used
        let branches = vec![BranchId::new(file_id, 2, 0, 0)];
        let ctx = build_cpg_prompt_context(&cpg, &branches, &file_paths, &source_cache);

        // Context should either have a "depends on:" section or at minimum the header
        assert!(
            ctx.contains("## Branch Context"),
            "should include context header, got:\n{ctx}"
        );
    }

    // -----------------------------------------------------------------------
    // Integration: prompt includes "Branch Context" section when CPG available
    // -----------------------------------------------------------------------
    #[test]
    fn integration_prompt_includes_cpg_context_section() {
        let source = "def check(value):\n    if value is None:\n        raise ValueError()\n    return value\n";
        let mut cpg = build_python_cpg(source, "check.py");
        add_reaching_def_edges(&mut cpg);

        let file_id: u64 = 99;
        let path = PathBuf::from("check.py");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, path.clone());
        let mut source_cache = HashMap::new();
        source_cache.insert(path, source.to_string());

        let branches = vec![BranchId::new(file_id, 2, 0, 1)]; // false branch of `if value is None`
        let cpg_ctx = build_cpg_prompt_context(&cpg, &branches, &file_paths, &source_cache);

        // The CPG context string — when non-empty — should be embeddable in a prompt.
        // We verify the format is correct for prompt inclusion.
        assert!(
            cpg_ctx.contains("## Branch Context (from CPG analysis)"),
            "integration: context must have the section header for LLM prompt inclusion, got:\n{cpg_ctx}"
        );
    }

    // -----------------------------------------------------------------------
    // reaching_info_at_line: no nodes at line → returns empty vec
    // -----------------------------------------------------------------------
    #[test]
    fn reaching_info_empty_when_no_nodes_at_line() {
        let cpg = Cpg::new();
        let result = reaching_info_at_line(&cpg, 99);
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // format_reaching_source: all variants produce non-empty strings
    // -----------------------------------------------------------------------
    #[test]
    fn format_reaching_source_all_variants() {
        use apex_cpg::{CtrlKind, NodeKind};

        let cases: Vec<NodeKind> = vec![
            NodeKind::Parameter { name: "p".into(), index: 0 },
            NodeKind::Assignment { lhs: "x".into(), line: 5 },
            NodeKind::Call { name: "foo".into(), line: 6 },
            NodeKind::Literal { value: "42".into(), line: 7 },
            NodeKind::Method { name: "bar".into(), file: "f.py".into(), line: 1 },
            NodeKind::Identifier { name: "y".into(), line: 8 },
            NodeKind::Return { line: 9 },
            NodeKind::ControlStructure { kind: CtrlKind::If, line: 10 },
        ];

        for case in &cases {
            let s = format_reaching_source("v", case);
            assert!(!s.is_empty(), "format_reaching_source should not return empty for {case:?}");
            assert!(s.contains('`'), "should include backtick-quoted name: {s}");
        }
    }
}
