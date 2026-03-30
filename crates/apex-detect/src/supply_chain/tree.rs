use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Package ecosystem identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    Cargo,
    Npm,
    #[serde(rename = "pypi")]
    PyPI,
    Go,
    Maven,
    #[serde(rename = "nuget")]
    NuGet,
    #[serde(rename = "rubygems")]
    RubyGems,
    Composer,
}

/// A single node in the dependency tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DepTreeNode {
    pub name: String,
    pub version: String,
    pub ecosystem: Ecosystem,
    pub purl: String,
    /// Depth in the tree (0 = direct dependency of root).
    pub depth: u32,
    /// Full path from root to this node: ["A", "B", "C", "this"].
    pub path: Vec<String>,
    /// Integrity hash (sha256/sha512) from lockfile.
    pub checksum: Option<String>,
    /// Registry or source URL.
    pub source_url: Option<String>,
    /// SPDX license expression.
    pub license: Option<String>,
    /// For git-branch dependencies: the branch name.
    pub git_branch: Option<String>,
    /// For git-branch dependencies: the resolved commit SHA.
    pub git_commit: Option<String>,
    /// Names of this node's direct dependencies.
    pub dependencies: Vec<String>,
}

/// A complete transitive dependency tree snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepTreeSnapshot {
    pub version: u32,
    pub timestamp: DateTime<Utc>,
    pub ecosystem: Ecosystem,
    pub root_package: String,
    pub root_version: String,
    pub git_ref: Option<String>,
    pub git_branch: Option<String>,
    pub total_deps: usize,
    pub max_depth: u32,
    /// All nodes keyed by "name@version" for O(1) lookup.
    pub nodes: HashMap<String, DepTreeNode>,
    /// Directed edges: (from "name@version", to "name@version").
    pub edges: Vec<(String, String)>,
    pub lockfile_path: String,
    pub resolution_method: String,
}

impl DepTreeNode {
    /// Construct the node key used in DepTreeSnapshot.nodes.
    pub fn key(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

impl DepTreeSnapshot {
    /// Get a node by package name (first match).
    pub fn find_by_name(&self, name: &str) -> Option<&DepTreeNode> {
        self.nodes.values().find(|n| n.name == name)
    }

    /// Get all nodes at a given depth.
    pub fn nodes_at_depth(&self, depth: u32) -> Vec<&DepTreeNode> {
        self.nodes.values().filter(|n| n.depth == depth).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node(name: &str, version: &str, depth: u32) -> DepTreeNode {
        DepTreeNode {
            name: name.to_string(),
            version: version.to_string(),
            ecosystem: Ecosystem::Cargo,
            purl: format!("pkg:cargo/{name}@{version}"),
            depth,
            path: vec!["root".to_string(), name.to_string()],
            checksum: Some("sha256:abc123".to_string()),
            source_url: Some("registry+https://crates.io".to_string()),
            license: Some("MIT".to_string()),
            git_branch: None,
            git_commit: None,
            dependencies: vec![],
        }
    }

    fn sample_snapshot() -> DepTreeSnapshot {
        let mut nodes = HashMap::new();
        let n1 = sample_node("serde", "1.0.200", 1);
        let n2 = sample_node("tokio", "1.37.0", 1);
        let n3 = DepTreeNode {
            name: "mio".to_string(),
            version: "0.8.11".to_string(),
            ecosystem: Ecosystem::Cargo,
            purl: "pkg:cargo/mio@0.8.11".to_string(),
            depth: 2,
            path: vec![
                "root".to_string(),
                "tokio".to_string(),
                "mio".to_string(),
            ],
            checksum: None,
            source_url: None,
            license: None,
            git_branch: None,
            git_commit: None,
            dependencies: vec![],
        };
        nodes.insert(n1.key(), n1);
        nodes.insert(n2.key(), n2);
        nodes.insert(n3.key(), n3);

        DepTreeSnapshot {
            version: 1,
            timestamp: Utc::now(),
            ecosystem: Ecosystem::Cargo,
            root_package: "my-app".to_string(),
            root_version: "0.1.0".to_string(),
            git_ref: None,
            git_branch: None,
            total_deps: 3,
            max_depth: 2,
            nodes,
            edges: vec![
                ("my-app@0.1.0".to_string(), "serde@1.0.200".to_string()),
                ("my-app@0.1.0".to_string(), "tokio@1.37.0".to_string()),
                ("tokio@1.37.0".to_string(), "mio@0.8.11".to_string()),
            ],
            lockfile_path: "Cargo.lock".to_string(),
            resolution_method: "cargo-metadata".to_string(),
        }
    }

    #[test]
    fn node_key_format() {
        let node = sample_node("serde", "1.0.200", 1);
        assert_eq!(node.key(), "serde@1.0.200");
    }

    #[test]
    fn serde_roundtrip() {
        let snapshot = sample_snapshot();
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let deserialized: DepTreeSnapshot =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.version, 1);
        assert_eq!(deserialized.ecosystem, Ecosystem::Cargo);
        assert_eq!(deserialized.root_package, "my-app");
        assert_eq!(deserialized.total_deps, 3);
        assert_eq!(deserialized.max_depth, 2);
        assert_eq!(deserialized.nodes.len(), 3);
        assert_eq!(deserialized.edges.len(), 3);
        assert_eq!(deserialized.lockfile_path, "Cargo.lock");
        assert_eq!(deserialized.resolution_method, "cargo-metadata");
    }

    #[test]
    fn find_by_name_found() {
        let snapshot = sample_snapshot();
        let node = snapshot.find_by_name("serde").expect("should find serde");
        assert_eq!(node.version, "1.0.200");
    }

    #[test]
    fn find_by_name_not_found() {
        let snapshot = sample_snapshot();
        assert!(snapshot.find_by_name("nonexistent").is_none());
    }

    #[test]
    fn nodes_at_depth() {
        let snapshot = sample_snapshot();
        let depth1 = snapshot.nodes_at_depth(1);
        assert_eq!(depth1.len(), 2);

        let depth2 = snapshot.nodes_at_depth(2);
        assert_eq!(depth2.len(), 1);
        assert_eq!(depth2[0].name, "mio");

        let depth99 = snapshot.nodes_at_depth(99);
        assert!(depth99.is_empty());
    }

    #[test]
    fn ecosystem_serde_roundtrip() {
        let eco = Ecosystem::PyPI;
        let json = serde_json::to_string(&eco).expect("serialize ecosystem");
        assert_eq!(json, "\"pypi\"");
        let deserialized: Ecosystem =
            serde_json::from_str(&json).expect("deserialize ecosystem");
        assert_eq!(deserialized, Ecosystem::PyPI);
    }

    #[test]
    fn all_ecosystems_serialize() {
        let ecosystems = vec![
            Ecosystem::Cargo,
            Ecosystem::Npm,
            Ecosystem::PyPI,
            Ecosystem::Go,
            Ecosystem::Maven,
            Ecosystem::NuGet,
            Ecosystem::RubyGems,
            Ecosystem::Composer,
        ];
        for eco in ecosystems {
            let json = serde_json::to_string(&eco).expect("serialize");
            let back: Ecosystem = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, eco);
        }
    }
}
