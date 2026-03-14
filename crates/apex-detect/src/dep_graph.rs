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

            for section in &[
                "dependencies",
                "dev-dependencies",
                "build-dependencies",
            ] {
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
            dfs_cycles(
                node,
                &adj,
                &mut visited,
                &mut stack,
                &mut path,
                &mut cycles,
            );
        }
    }
    cycles
}

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
                let cycle: Vec<String> =
                    path[start..].iter().map(|s| s.to_string()).collect();
                if cycle.len() > 1 {
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
}
