use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::supply_chain::tree::{DepTreeNode, DepTreeSnapshot, Ecosystem};

/// Classification of a single change between two tree snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    VersionChanged { from: String, to: String },
    ChecksumChanged { from: Option<String>, to: Option<String> },
    DepthChanged { from: u32, to: u32 },
    SourceChanged { from: Option<String>, to: Option<String> },
    BranchMutated { branch: String, from_commit: Option<String>, to_commit: Option<String> },
    LicenseChanged { from: Option<String>, to: Option<String> },
}

/// A single change in the dependency tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeChange {
    pub package: String,
    pub ecosystem: Ecosystem,
    pub kind: ChangeKind,
    /// Full propagation path from root to affected node.
    pub propagation_path: Vec<String>,
    pub depth: u32,
    /// Risk score (filled by risk scoring).
    pub risk_score: f64,
    /// Risk signals that contributed to the score.
    pub risk_signals: Vec<String>,
}

/// The diff between two dependency tree snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeDiff {
    pub from_timestamp: DateTime<Utc>,
    pub to_timestamp: DateTime<Utc>,
    pub ecosystem: Ecosystem,
    pub changes: Vec<TreeChange>,
    /// Aggregate risk score across all changes (0.0 - 10.0).
    pub aggregate_risk: f64,
    pub summary: TreeDiffSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeDiffSummary {
    pub added: usize,
    pub removed: usize,
    pub version_changed: usize,
    pub checksum_changed: usize,
    pub depth_changed: usize,
    pub source_changed: usize,
    pub branch_mutated: usize,
    pub license_changed: usize,
    pub total_changes: usize,
}

/// Compute the diff between two DepTreeSnapshots.
pub fn diff_trees(from: &DepTreeSnapshot, to: &DepTreeSnapshot) -> TreeDiff {
    let mut changes: Vec<TreeChange> = Vec::new();
    let ecosystem = to.ecosystem;

    // Build name-to-nodes maps for both snapshots
    let from_by_name = group_by_name(&from.nodes);
    let to_by_name = group_by_name(&to.nodes);

    let all_names: HashSet<&str> = from_by_name.keys().chain(to_by_name.keys()).copied().collect();

    for name in all_names {
        let from_nodes = from_by_name.get(name);
        let to_nodes = to_by_name.get(name);

        match (from_nodes, to_nodes) {
            // Package added
            (None, Some(nodes)) => {
                for node in nodes {
                    let path = build_propagation_path(to, &node.key());
                    changes.push(TreeChange {
                        package: name.to_string(),
                        ecosystem,
                        kind: ChangeKind::Added,
                        propagation_path: path,
                        depth: node.depth,
                        risk_score: 0.0,
                        risk_signals: vec![],
                    });
                }
            }
            // Package removed
            (Some(nodes), None) => {
                for node in nodes {
                    let path = build_propagation_path(from, &node.key());
                    changes.push(TreeChange {
                        package: name.to_string(),
                        ecosystem,
                        kind: ChangeKind::Removed,
                        propagation_path: path,
                        depth: node.depth,
                        risk_score: 0.0,
                        risk_signals: vec![],
                    });
                }
            }
            // Package exists in both -- check for changes
            (Some(from_nodes), Some(to_nodes)) => {
                // Pass 1: Version changes (compare version sets)
                let from_versions: HashSet<&str> =
                    from_nodes.iter().map(|n| n.version.as_str()).collect();
                let to_versions: HashSet<&str> =
                    to_nodes.iter().map(|n| n.version.as_str()).collect();

                if from_versions != to_versions {
                    let old_v = from_nodes.first().map(|n| n.version.as_str()).unwrap_or("?");
                    let new_v = to_nodes.first().map(|n| n.version.as_str()).unwrap_or("?");
                    let ref_node = to_nodes.first().unwrap();
                    let path = build_propagation_path(to, &ref_node.key());

                    changes.push(TreeChange {
                        package: name.to_string(),
                        ecosystem,
                        kind: ChangeKind::VersionChanged {
                            from: old_v.to_string(),
                            to: new_v.to_string(),
                        },
                        propagation_path: path,
                        depth: ref_node.depth,
                        risk_score: 0.0,
                        risk_signals: vec![],
                    });
                }

                // Pass 2: Same name@version, compare fields
                for to_node in to_nodes {
                    let key = to_node.key();
                    if let Some(from_node) = from.nodes.get(&key) {
                        // Checksum changed (CRITICAL -- same version, different artifact)
                        if from_node.checksum != to_node.checksum
                            && (from_node.checksum.is_some() || to_node.checksum.is_some())
                        {
                            let path = build_propagation_path(to, &key);
                            changes.push(TreeChange {
                                package: name.to_string(),
                                ecosystem,
                                kind: ChangeKind::ChecksumChanged {
                                    from: from_node.checksum.clone(),
                                    to: to_node.checksum.clone(),
                                },
                                propagation_path: path,
                                depth: to_node.depth,
                                risk_score: 0.0,
                                risk_signals: vec![],
                            });
                        }

                        // Depth changed
                        if from_node.depth != to_node.depth {
                            let path = build_propagation_path(to, &key);
                            changes.push(TreeChange {
                                package: name.to_string(),
                                ecosystem,
                                kind: ChangeKind::DepthChanged {
                                    from: from_node.depth,
                                    to: to_node.depth,
                                },
                                propagation_path: path,
                                depth: to_node.depth,
                                risk_score: 0.0,
                                risk_signals: vec![],
                            });
                        }

                        // Source changed
                        if from_node.source_url != to_node.source_url
                            && (from_node.source_url.is_some() || to_node.source_url.is_some())
                        {
                            let path = build_propagation_path(to, &key);
                            changes.push(TreeChange {
                                package: name.to_string(),
                                ecosystem,
                                kind: ChangeKind::SourceChanged {
                                    from: from_node.source_url.clone(),
                                    to: to_node.source_url.clone(),
                                },
                                propagation_path: path,
                                depth: to_node.depth,
                                risk_score: 0.0,
                                risk_signals: vec![],
                            });
                        }

                        // License changed
                        if from_node.license != to_node.license
                            && (from_node.license.is_some() || to_node.license.is_some())
                        {
                            let path = build_propagation_path(to, &key);
                            changes.push(TreeChange {
                                package: name.to_string(),
                                ecosystem,
                                kind: ChangeKind::LicenseChanged {
                                    from: from_node.license.clone(),
                                    to: to_node.license.clone(),
                                },
                                propagation_path: path,
                                depth: to_node.depth,
                                risk_score: 0.0,
                                risk_signals: vec![],
                            });
                        }

                        // Branch mutated (same branch, different commit)
                        if from_node.git_branch.is_some()
                            && from_node.git_branch == to_node.git_branch
                            && from_node.git_commit != to_node.git_commit
                        {
                            let path = build_propagation_path(to, &key);
                            changes.push(TreeChange {
                                package: name.to_string(),
                                ecosystem,
                                kind: ChangeKind::BranchMutated {
                                    branch: from_node.git_branch.clone().unwrap_or_default(),
                                    from_commit: from_node.git_commit.clone(),
                                    to_commit: to_node.git_commit.clone(),
                                },
                                propagation_path: path,
                                depth: to_node.depth,
                                risk_score: 0.0,
                                risk_signals: vec![],
                            });
                        }
                    }
                }
            }
            (None, None) => unreachable!(),
        }
    }

    let summary = build_summary(&changes);

    TreeDiff {
        from_timestamp: from.timestamp,
        to_timestamp: to.timestamp,
        ecosystem,
        changes,
        aggregate_risk: 0.0,
        summary,
    }
}

fn group_by_name(
    nodes: &HashMap<String, DepTreeNode>,
) -> HashMap<&str, Vec<&DepTreeNode>> {
    let mut map: HashMap<&str, Vec<&DepTreeNode>> = HashMap::new();
    for node in nodes.values() {
        map.entry(node.name.as_str()).or_default().push(node);
    }
    map
}

/// Build propagation path from root to a given node key using BFS on edges.
fn build_propagation_path(snapshot: &DepTreeSnapshot, target_key: &str) -> Vec<String> {
    // If the node has a stored path, use it directly
    if let Some(node) = snapshot.nodes.get(target_key) {
        if !node.path.is_empty() {
            return node.path.clone();
        }
    }

    // Fallback: BFS through edges
    let root_key = format!("{}@{}", snapshot.root_package, snapshot.root_version);
    if target_key == root_key {
        return vec![snapshot.root_package.clone()];
    }

    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to) in &snapshot.edges {
        adjacency.entry(from.as_str()).or_default().push(to.as_str());
    }

    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<(&str, Vec<String>)> = VecDeque::new();

    visited.insert(&root_key);
    queue.push_back((&root_key, vec![snapshot.root_package.clone()]));

    while let Some((current, path)) = queue.pop_front() {
        if let Some(neighbors) = adjacency.get(current) {
            for &neighbor in neighbors {
                if visited.insert(neighbor) {
                    let name = neighbor.split('@').next().unwrap_or(neighbor);
                    let mut new_path = path.clone();
                    new_path.push(name.to_string());

                    if neighbor == target_key {
                        return new_path;
                    }
                    queue.push_back((neighbor, new_path));
                }
            }
        }
    }

    // Fallback: just the package name
    let name = target_key.split('@').next().unwrap_or(target_key);
    vec![snapshot.root_package.clone(), name.to_string()]
}

fn build_summary(changes: &[TreeChange]) -> TreeDiffSummary {
    let mut s = TreeDiffSummary {
        added: 0,
        removed: 0,
        version_changed: 0,
        checksum_changed: 0,
        depth_changed: 0,
        source_changed: 0,
        branch_mutated: 0,
        license_changed: 0,
        total_changes: changes.len(),
    };
    for c in changes {
        match &c.kind {
            ChangeKind::Added => s.added += 1,
            ChangeKind::Removed => s.removed += 1,
            ChangeKind::VersionChanged { .. } => s.version_changed += 1,
            ChangeKind::ChecksumChanged { .. } => s.checksum_changed += 1,
            ChangeKind::DepthChanged { .. } => s.depth_changed += 1,
            ChangeKind::SourceChanged { .. } => s.source_changed += 1,
            ChangeKind::BranchMutated { .. } => s.branch_mutated += 1,
            ChangeKind::LicenseChanged { .. } => s.license_changed += 1,
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supply_chain::tree::Ecosystem;

    /// Helper to build a DepTreeNode with sensible defaults.
    fn make_node(
        name: &str,
        version: &str,
        depth: u32,
        path: Vec<&str>,
    ) -> DepTreeNode {
        DepTreeNode {
            name: name.to_string(),
            version: version.to_string(),
            ecosystem: Ecosystem::Npm,
            purl: format!("pkg:npm/{name}@{version}"),
            depth,
            path: path.iter().map(|s| s.to_string()).collect(),
            checksum: Some(format!("sha256:{name}-{version}")),
            source_url: Some("https://registry.npmjs.org".to_string()),
            license: Some("MIT".to_string()),
            git_branch: None,
            git_commit: None,
            dependencies: vec![],
        }
    }

    /// Build a snapshot with a chain: root -> A -> B -> C -> D
    fn chain_snapshot(
        d_version: &str,
        d_checksum: Option<&str>,
        d_source: Option<&str>,
        d_license: Option<&str>,
        d_git_branch: Option<&str>,
        d_git_commit: Option<&str>,
    ) -> DepTreeSnapshot {
        let mut nodes = HashMap::new();

        let a = make_node("A", "1.0.0", 1, vec!["root", "A"]);
        let b = make_node("B", "2.0.0", 2, vec!["root", "A", "B"]);
        let c = make_node("C", "3.0.0", 3, vec!["root", "A", "B", "C"]);
        let mut d = make_node("D", d_version, 4, vec!["root", "A", "B", "C", "D"]);
        d.checksum = d_checksum.map(|s| s.to_string());
        d.source_url = d_source.map(|s| s.to_string());
        d.license = d_license.map(|s| s.to_string());
        d.git_branch = d_git_branch.map(|s| s.to_string());
        d.git_commit = d_git_commit.map(|s| s.to_string());

        nodes.insert(a.key(), a);
        nodes.insert(b.key(), b);
        nodes.insert(c.key(), c);
        nodes.insert(d.key(), d);

        DepTreeSnapshot {
            version: 1,
            timestamp: Utc::now(),
            ecosystem: Ecosystem::Npm,
            root_package: "root".to_string(),
            root_version: "0.1.0".to_string(),
            git_ref: None,
            git_branch: None,
            total_deps: 4,
            max_depth: 4,
            nodes,
            edges: vec![
                ("root@0.1.0".to_string(), "A@1.0.0".to_string()),
                ("A@1.0.0".to_string(), "B@2.0.0".to_string()),
                ("B@2.0.0".to_string(), "C@3.0.0".to_string()),
                ("C@3.0.0".to_string(), format!("D@{d_version}")),
            ],
            lockfile_path: "package-lock.json".to_string(),
            resolution_method: "npm-ls".to_string(),
        }
    }

    #[test]
    fn diff_version_changed_with_propagation_path() {
        let from = chain_snapshot("0.8.0", Some("sha256:D-0.8.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
        let to = chain_snapshot("0.9.0", Some("sha256:D-0.9.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);

        let diff = diff_trees(&from, &to);

        let version_changes: Vec<_> = diff.changes.iter()
            .filter(|c| matches!(&c.kind, ChangeKind::VersionChanged { .. }))
            .collect();
        assert_eq!(version_changes.len(), 1);

        let vc = &version_changes[0];
        assert_eq!(vc.package, "D");
        if let ChangeKind::VersionChanged { from, to } = &vc.kind {
            assert_eq!(from, "0.8.0");
            assert_eq!(to, "0.9.0");
        }
        // Propagation path should go root -> A -> B -> C -> D
        assert_eq!(vc.propagation_path, vec!["root", "A", "B", "C", "D"]);
    }

    #[test]
    fn diff_checksum_changed_same_version() {
        let from = chain_snapshot("1.0.0", Some("sha256:aaa"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
        let to = chain_snapshot("1.0.0", Some("sha256:bbb"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);

        let diff = diff_trees(&from, &to);

        let checksum_changes: Vec<_> = diff.changes.iter()
            .filter(|c| matches!(&c.kind, ChangeKind::ChecksumChanged { .. }))
            .collect();
        assert_eq!(checksum_changes.len(), 1);
        assert_eq!(checksum_changes[0].package, "D");
    }

    #[test]
    fn diff_package_added() {
        let from = {
            let mut s = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
            // Remove D from the "from" snapshot
            s.nodes.remove("D@1.0.0");
            s.edges.retain(|(_f, t)| t != "D@1.0.0");
            s
        };
        let to = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);

        let diff = diff_trees(&from, &to);

        let added: Vec<_> = diff.changes.iter()
            .filter(|c| matches!(&c.kind, ChangeKind::Added))
            .collect();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].package, "D");
        assert_eq!(added[0].depth, 4);
    }

    #[test]
    fn diff_package_removed() {
        let from = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
        let to = {
            let mut s = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
            s.nodes.remove("D@1.0.0");
            s.edges.retain(|(_f, t)| t != "D@1.0.0");
            s
        };

        let diff = diff_trees(&from, &to);

        let removed: Vec<_> = diff.changes.iter()
            .filter(|c| matches!(&c.kind, ChangeKind::Removed))
            .collect();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].package, "D");
    }

    #[test]
    fn diff_source_changed() {
        let from = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
        let to = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://evil.registry.com"), Some("MIT"), None, None);

        let diff = diff_trees(&from, &to);

        let source_changes: Vec<_> = diff.changes.iter()
            .filter(|c| matches!(&c.kind, ChangeKind::SourceChanged { .. }))
            .collect();
        assert_eq!(source_changes.len(), 1);
        assert_eq!(source_changes[0].package, "D");
    }

    #[test]
    fn diff_branch_mutated() {
        let from = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), Some("main"), Some("abc123"));
        let to = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), Some("main"), Some("def456"));

        let diff = diff_trees(&from, &to);

        let mutations: Vec<_> = diff.changes.iter()
            .filter(|c| matches!(&c.kind, ChangeKind::BranchMutated { .. }))
            .collect();
        assert_eq!(mutations.len(), 1);
        assert_eq!(mutations[0].package, "D");
        if let ChangeKind::BranchMutated { branch, from_commit, to_commit } = &mutations[0].kind {
            assert_eq!(branch, "main");
            assert_eq!(from_commit.as_deref(), Some("abc123"));
            assert_eq!(to_commit.as_deref(), Some("def456"));
        }
    }

    #[test]
    fn diff_depth_changed() {
        let from = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
        let mut to = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);

        // Modify depth of D in "to" snapshot
        if let Some(d) = to.nodes.get_mut("D@1.0.0") {
            d.depth = 2;
        }

        let diff = diff_trees(&from, &to);

        let depth_changes: Vec<_> = diff.changes.iter()
            .filter(|c| matches!(&c.kind, ChangeKind::DepthChanged { .. }))
            .collect();
        assert_eq!(depth_changes.len(), 1);
        if let ChangeKind::DepthChanged { from, to } = &depth_changes[0].kind {
            assert_eq!(*from, 4);
            assert_eq!(*to, 2);
        }
    }

    #[test]
    fn diff_license_changed() {
        let from = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
        let to = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("GPL-3.0"), None, None);

        let diff = diff_trees(&from, &to);

        let license_changes: Vec<_> = diff.changes.iter()
            .filter(|c| matches!(&c.kind, ChangeKind::LicenseChanged { .. }))
            .collect();
        assert_eq!(license_changes.len(), 1);
        assert_eq!(license_changes[0].package, "D");
        if let ChangeKind::LicenseChanged { from, to } = &license_changes[0].kind {
            assert_eq!(from.as_deref(), Some("MIT"));
            assert_eq!(to.as_deref(), Some("GPL-3.0"));
        }
    }

    #[test]
    fn diff_identical_snapshots_empty() {
        let from = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
        let to = chain_snapshot("1.0.0", Some("sha256:D-1.0.0"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);

        let diff = diff_trees(&from, &to);
        assert_eq!(diff.changes.len(), 0);
        assert_eq!(diff.summary.total_changes, 0);
    }

    #[test]
    fn diff_summary_counts_correct() {
        // From has D@1.0.0, to has D@2.0.0 with different checksum (version change only since key differs)
        let from = chain_snapshot("1.0.0", Some("sha256:old"), Some("https://registry.npmjs.org"), Some("MIT"), None, None);
        let mut to = chain_snapshot("2.0.0", Some("sha256:new"), Some("https://evil.com"), Some("GPL-3.0"), None, None);

        // Also add a brand-new package E in "to"
        let e = make_node("E", "1.0.0", 2, vec!["root", "A", "E"]);
        to.nodes.insert(e.key(), e);

        let diff = diff_trees(&from, &to);

        // Should have: version changed for D, added for E
        assert!(diff.summary.version_changed >= 1, "expected version_changed >= 1, got {}", diff.summary.version_changed);
        assert!(diff.summary.added >= 1, "expected added >= 1, got {}", diff.summary.added);
        assert_eq!(diff.summary.total_changes, diff.changes.len());
    }
}
