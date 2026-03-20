//! Dependency Graph — visualize and analyze package dependencies.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct DepNode {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepGraphReport {
    pub nodes: Vec<DepNode>,
    pub edges: Vec<(String, String)>, // (from, to)
    pub cycles: Vec<Vec<String>>,
    pub fan_in: HashMap<String, usize>,  // how many depend on this
    pub fan_out: HashMap<String, usize>, // how many this depends on
}

/// Parse Cargo.toml to extract dependencies.
pub fn analyze_cargo(target: &Path) -> DepGraphReport {
    let cargo_path = target.join("Cargo.toml");
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut fan_in: HashMap<String, usize> = HashMap::new();
    let mut fan_out: HashMap<String, usize> = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(&cargo_path) {
        if let Ok(value) = content.parse::<toml::Value>() {
            let pkg_name = value
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();

            nodes.push(DepNode {
                name: pkg_name.clone(),
                version: value
                    .get("package")
                    .and_then(|p| p.get("version"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });

            for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(deps) = value.get(section).and_then(|d| d.as_table()) {
                    for (dep_name, _dep_val) in deps {
                        nodes.push(DepNode {
                            name: dep_name.clone(),
                            version: None,
                        });
                        edges.push((pkg_name.clone(), dep_name.clone()));
                        *fan_out.entry(pkg_name.clone()).or_default() += 1;
                        *fan_in.entry(dep_name.clone()).or_default() += 1;
                    }
                }
            }
        }
    }

    // Workspace members
    let workspace_tomls = find_workspace_tomls(target);
    for wt in &workspace_tomls {
        if let Ok(content) = std::fs::read_to_string(wt) {
            if let Ok(value) = content.parse::<toml::Value>() {
                let pkg_name = value
                    .get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                nodes.push(DepNode {
                    name: pkg_name.clone(),
                    version: None,
                });

                if let Some(deps) = value.get("dependencies").and_then(|d| d.as_table()) {
                    for (dep_name, _) in deps {
                        edges.push((pkg_name.clone(), dep_name.clone()));
                        *fan_out.entry(pkg_name.clone()).or_default() += 1;
                        *fan_in.entry(dep_name.clone()).or_default() += 1;
                    }
                }
            }
        }
    }

    // Deduplicate nodes
    let mut seen = HashSet::new();
    nodes.retain(|n| seen.insert(n.name.clone()));

    // Ensure every package has an entry in both fan_in and fan_out (zero-degree nodes
    // would otherwise be missing, making callers unable to distinguish "absent" from "0").
    for node in &nodes {
        fan_in.entry(node.name.clone()).or_insert(0);
        fan_out.entry(node.name.clone()).or_insert(0);
    }

    // Simple cycle detection via DFS
    let cycles = detect_cycles(&edges);

    DepGraphReport {
        nodes,
        edges,
        cycles,
        fan_in,
        fan_out,
    }
}

fn find_workspace_tomls(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let cargo = path.join("Cargo.toml");
                if cargo.exists() {
                    result.push(cargo);
                }
                // One level deeper for crates/ pattern
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub in sub_entries.flatten() {
                        let sub_cargo = sub.path().join("Cargo.toml");
                        if sub.path().is_dir() && sub_cargo.exists() {
                            result.push(sub_cargo);
                        }
                    }
                }
            }
        }
    }
    result
}

fn detect_cycles(edges: &[(String, String)]) -> Vec<Vec<String>> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to) in edges {
        adj.entry(from.as_str()).or_default().push(to.as_str());
    }

    let mut cycles = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = HashSet::new();
    let mut path = Vec::new();

    for node in adj.keys() {
        if !visited.contains(node) {
            dfs_cycles(node, &adj, &mut visited, &mut stack, &mut path, &mut cycles);
        }
    }
    cycles
}

// TODO(Bug 14): The current `visited` set causes DFS to skip nodes already seen
// from a different path, which may miss cycles reachable only via those nodes.
// A proper fix requires Tarjan's SCC algorithm or resetting `visited` per
// root — both require more invasive restructuring.
fn dfs_cycles<'a>(
    node: &'a str,
    adj: &HashMap<&'a str, Vec<&'a str>>,
    visited: &mut HashSet<&'a str>,
    stack: &mut HashSet<&'a str>,
    path: &mut Vec<&'a str>,
    cycles: &mut Vec<Vec<String>>,
) {
    visited.insert(node);
    stack.insert(node);
    path.push(node);

    if let Some(neighbors) = adj.get(node) {
        for &next in neighbors {
            if !visited.contains(next) {
                dfs_cycles(next, adj, visited, stack, path, cycles);
            } else if stack.contains(next) {
                // Found cycle
                let start = path.iter().position(|&n| n == next).unwrap_or(0);
                let cycle: Vec<String> = path[start..].iter().map(|s| s.to_string()).collect();
                if !cycle.is_empty() {
                    cycles.push(cycle);
                }
            }
        }
    }

    stack.remove(node);
    path.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cycles_finds_cycle() {
        let edges = vec![
            ("a".into(), "b".into()),
            ("b".into(), "c".into()),
            ("c".into(), "a".into()),
        ];
        let cycles = detect_cycles(&edges);
        assert!(!cycles.is_empty());
    }

    #[test]
    fn detect_cycles_no_cycle() {
        let edges = vec![("a".into(), "b".into()), ("b".into(), "c".into())];
        let cycles = detect_cycles(&edges);
        assert!(cycles.is_empty());
    }

    #[test]
    fn empty_edges_no_cycles() {
        let cycles = detect_cycles(&[]);
        assert!(cycles.is_empty());
    }

    #[test]
    fn dep_graph_report_serializes() {
        let report = DepGraphReport {
            nodes: vec![DepNode {
                name: "test".into(),
                version: Some("1.0".into()),
            }],
            edges: vec![],
            cycles: vec![],
            fan_in: HashMap::new(),
            fan_out: HashMap::new(),
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("test"));
    }

    // -----------------------------------------------------------------------
    // Cycle detection edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn detect_self_loop_is_cycle() {
        // A node pointing to itself is a cycle (stack contains it when we see it again)
        let edges = vec![("a".to_string(), "a".to_string())];
        let cycles = detect_cycles(&edges);
        assert!(
            !cycles.is_empty(),
            "self-loop should be detected as a cycle"
        );
    }

    #[test]
    fn detect_two_node_cycle() {
        // a -> b, b -> a
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "a".to_string()),
        ];
        let cycles = detect_cycles(&edges);
        assert!(!cycles.is_empty());
    }

    #[test]
    fn detect_cycle_in_longer_chain() {
        // a -> b -> c -> d -> b (cycle b-c-d)
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
            ("c".to_string(), "d".to_string()),
            ("d".to_string(), "b".to_string()),
        ];
        let cycles = detect_cycles(&edges);
        assert!(!cycles.is_empty());
    }

    #[test]
    fn no_cycle_in_diamond_dag() {
        // Diamond: a -> b, a -> c, b -> d, c -> d — no cycle
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("a".to_string(), "c".to_string()),
            ("b".to_string(), "d".to_string()),
            ("c".to_string(), "d".to_string()),
        ];
        let cycles = detect_cycles(&edges);
        assert!(cycles.is_empty(), "diamond DAG has no cycles");
    }

    #[test]
    fn disconnected_graph_two_components_no_cycle() {
        // Two unconnected chains: a->b and c->d
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("c".to_string(), "d".to_string()),
        ];
        let cycles = detect_cycles(&edges);
        assert!(cycles.is_empty());
    }

    #[test]
    fn disconnected_graph_one_component_with_cycle() {
        // a->b (no cycle) and c->d->c (cycle)
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("c".to_string(), "d".to_string()),
            ("d".to_string(), "c".to_string()),
        ];
        let cycles = detect_cycles(&edges);
        assert!(
            !cycles.is_empty(),
            "should detect cycle in second component"
        );
    }

    #[test]
    fn cycle_path_contains_expected_nodes() {
        // a -> b -> c -> a: cycle should include a, b, c
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
            ("c".to_string(), "a".to_string()),
        ];
        let cycles = detect_cycles(&edges);
        assert!(!cycles.is_empty());
        let flat: Vec<&str> = cycles.iter().flatten().map(|s| s.as_str()).collect();
        assert!(
            flat.contains(&"a") || flat.contains(&"b") || flat.contains(&"c"),
            "cycle path should include nodes from the cycle"
        );
    }

    // -----------------------------------------------------------------------
    // Fan-in / fan-out metrics
    // -----------------------------------------------------------------------

    #[test]
    fn fan_in_fan_out_zero_for_isolated_nodes() {
        // All nodes with zero fan-in/out should have entries (not missing)
        let dir = std::env::temp_dir().join("apex_test_depgraph_isolated");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"isolated\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        let report = analyze_cargo(&dir);
        // Package with no deps: fan_out["isolated"] == 0, fan_in["isolated"] == 0
        assert_eq!(report.fan_out.get("isolated").copied(), Some(0));
        assert_eq!(report.fan_in.get("isolated").copied(), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fan_out_increments_per_dependency() {
        let dir = std::env::temp_dir().join("apex_test_depgraph_fan_out");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\ntokio = \"1\"\n",
        )
        .unwrap();
        let report = analyze_cargo(&dir);
        assert_eq!(
            report.fan_out.get("myapp").copied(),
            Some(2),
            "myapp depends on 2 crates"
        );
        assert_eq!(report.fan_in.get("serde").copied(), Some(1));
        assert_eq!(report.fan_in.get("tokio").copied(), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fan_in_reflects_shared_dependency() {
        // Two packages both depending on "shared" -> fan_in["shared"] == 2
        // We simulate this via dev-dependencies and dependencies in one Cargo.toml
        let dir = std::env::temp_dir().join("apex_test_depgraph_shared_dep");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nshared = \"1\"\n\n[dev-dependencies]\nshared = \"1\"\n",
        )
        .unwrap();
        let report = analyze_cargo(&dir);
        // "shared" is listed twice (once per section), so fan_in should be 2
        assert_eq!(
            report.fan_in.get("shared").copied(),
            Some(2),
            "shared dep appears in both deps and dev-deps"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // Workspace member discovery
    // -----------------------------------------------------------------------

    #[test]
    fn workspace_members_are_discovered() {
        // find_workspace_tomls looks for Cargo.toml in subdirs and crates/**
        let dir = std::env::temp_dir().join("apex_test_depgraph_workspace");
        let _ = std::fs::remove_dir_all(&dir);
        let crates_dir = dir.join("crates").join("mylib");
        let _ = std::fs::create_dir_all(&crates_dir);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/mylib\"]\n",
        )
        .unwrap();
        std::fs::write(
            crates_dir.join("Cargo.toml"),
            "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let report = analyze_cargo(&dir);
        let node_names: Vec<&str> = report.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(
            node_names.contains(&"mylib"),
            "workspace member should be discovered"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_member_deps_contribute_to_fan_metrics() {
        // A workspace member with its own dep should update fan_in/fan_out
        let dir = std::env::temp_dir().join("apex_test_depgraph_ws_fanout");
        let _ = std::fs::remove_dir_all(&dir);
        let crates_dir = dir.join("crates").join("mylib");
        let _ = std::fs::create_dir_all(&crates_dir);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/mylib\"]\n",
        )
        .unwrap();
        std::fs::write(
            crates_dir.join("Cargo.toml"),
            "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\n\n[dependencies]\nanyhow = \"1\"\n",
        )
        .unwrap();
        let report = analyze_cargo(&dir);
        assert_eq!(
            report.fan_out.get("mylib").copied(),
            Some(1),
            "mylib should have fan_out=1 for its dep on anyhow"
        );
        assert_eq!(report.fan_in.get("anyhow").copied(), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_cargo_toml_returns_empty_report() {
        // analyze_cargo on a dir without Cargo.toml returns an empty report
        let dir = std::env::temp_dir().join("apex_test_depgraph_no_cargo");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let report = analyze_cargo(&dir);
        assert!(report.nodes.is_empty(), "no nodes without Cargo.toml");
        assert!(report.edges.is_empty());
        assert!(report.cycles.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deduplication_removes_duplicate_nodes() {
        // If a dep appears in both dependencies and dev-dependencies, it should
        // appear only once in nodes after deduplication
        let dir = std::env::temp_dir().join("apex_test_depgraph_dedup");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n\n[dev-dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        let report = analyze_cargo(&dir);
        let serde_count = report.nodes.iter().filter(|n| n.name == "serde").count();
        assert_eq!(
            serde_count, 1,
            "deduplication should ensure serde appears once"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_dependencies_are_included() {
        let dir = std::env::temp_dir().join("apex_test_depgraph_build_deps");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[build-dependencies]\ncc = \"1\"\n",
        )
        .unwrap();
        let report = analyze_cargo(&dir);
        let node_names: Vec<&str> = report.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(
            node_names.contains(&"cc"),
            "build-dependencies should be included"
        );
        assert_eq!(report.fan_in.get("cc").copied(), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
