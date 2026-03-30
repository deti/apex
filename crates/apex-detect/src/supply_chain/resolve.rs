use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use chrono::Utc;

use apex_core::error::ApexError;

use crate::supply_chain::tree::{DepTreeNode, DepTreeSnapshot, Ecosystem};

/// Package info tuple: (name, version, license, source).
type PkgInfo<'a> = (&'a str, &'a str, Option<&'a str>, Option<&'a str>);

/// Auto-detect ecosystem from lockfiles present in target directory.
pub fn detect_ecosystem(target: &Path) -> Option<Ecosystem> {
    if target.join("Cargo.lock").exists() {
        Some(Ecosystem::Cargo)
    } else if target.join("package-lock.json").exists() {
        Some(Ecosystem::Npm)
    } else if target.join("go.sum").exists() {
        Some(Ecosystem::Go)
    } else if target.join("requirements.txt").exists() || target.join("Pipfile.lock").exists() {
        Some(Ecosystem::PyPI)
    } else if target.join("Gemfile.lock").exists() {
        Some(Ecosystem::RubyGems)
    } else if target.join("composer.lock").exists() {
        Some(Ecosystem::Composer)
    } else if target.join("packages.lock.json").exists() {
        Some(Ecosystem::NuGet)
    } else {
        None
    }
}

/// Resolve the full transitive dependency tree for a project.
pub fn resolve_tree(
    target: &Path,
    ecosystem: Ecosystem,
) -> Result<DepTreeSnapshot, ApexError> {
    match ecosystem {
        Ecosystem::Cargo => resolve_cargo_tree(target),
        Ecosystem::Npm => resolve_npm_tree(target),
        _ => resolve_flat_fallback(target, ecosystem),
    }
}

/// Resolve via `cargo metadata --format-version 1`.
/// Parses the `resolve` field to build the full dependency tree with BFS.
fn resolve_cargo_tree(target: &Path) -> Result<DepTreeSnapshot, ApexError> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(target)
        .output()
        .map_err(|e| ApexError::Detect(format!("cargo metadata exec: {e}")))?;

    if !output.status.success() {
        return Err(ApexError::Detect(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| ApexError::Detect(format!("Failed to parse cargo metadata: {e}")))?;

    let resolve = metadata
        .get("resolve")
        .ok_or_else(|| ApexError::Detect("No resolve field in cargo metadata".into()))?;
    let root_id = resolve
        .get("root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApexError::Detect("No resolve.root in cargo metadata".into()))?;

    // Build package info lookup from metadata.packages
    let packages = metadata
        .get("packages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ApexError::Detect("No packages array".into()))?;

    let mut pkg_info: HashMap<String, PkgInfo<'_>> = HashMap::new();
    for pkg in packages {
        let id = pkg.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");
        let license = pkg.get("license").and_then(|v| v.as_str());
        let source = pkg.get("source").and_then(|v| v.as_str());
        pkg_info.insert(id.to_string(), (name, version, license, source));
    }

    // Build resolve node adjacency from resolve.nodes
    let resolve_nodes = resolve
        .get("nodes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ApexError::Detect("No resolve.nodes".into()))?;

    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    for node in resolve_nodes {
        let id = node.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let deps = node
            .get("deps")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|d| d.get("pkg").and_then(|v| v.as_str()).map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        adjacency.insert(id.to_string(), deps);
    }

    // Parse root info
    let (root_name, root_version, _, _) = pkg_info
        .get(root_id)
        .copied()
        .unwrap_or(("unknown", "0.0.0", None, None));

    // BFS from root to build tree with depth and path
    let mut nodes: HashMap<String, DepTreeNode> = HashMap::new();
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut max_depth: u32 = 0;

    // Queue: (package_id, depth, path_so_far)
    let mut queue: VecDeque<(String, u32, Vec<String>)> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();

    queue.push_back((root_id.to_string(), 0, vec![root_name.to_string()]));
    visited.insert(root_id.to_string());

    while let Some((pkg_id, depth, path)) = queue.pop_front() {
        let (name, version, license, source) = pkg_info
            .get(&pkg_id)
            .copied()
            .unwrap_or(("unknown", "0.0.0", None, None));

        let key = format!("{name}@{version}");
        let purl = format!("pkg:cargo/{name}@{version}");

        // Parse git branch/commit from source URL if present
        let (git_branch, git_commit) = parse_cargo_source_git(source);

        let dep_names: Vec<String> = adjacency
            .get(&pkg_id)
            .map(|deps| {
                deps.iter()
                    .filter_map(|did| {
                        pkg_info.get(did.as_str()).map(|(n, _, _, _)| n.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();

        if depth > 0 {
            // Skip root node itself from the nodes map (it's the project, not a dep)
            let node = DepTreeNode {
                name: name.to_string(),
                version: version.to_string(),
                ecosystem: Ecosystem::Cargo,
                purl,
                depth,
                path: path.clone(),
                checksum: None, // Will be filled from Cargo.lock cross-reference
                source_url: source.map(|s| s.to_string()),
                license: license.map(|s| s.to_string()),
                git_branch,
                git_commit,
                dependencies: dep_names.clone(),
            };

            if !nodes.contains_key(&key) {
                nodes.insert(key.clone(), node);
            }
        }

        if depth > max_depth {
            max_depth = depth;
        }

        // Enqueue children
        if let Some(dep_ids) = adjacency.get(&pkg_id) {
            for dep_id in dep_ids {
                let (dep_name, dep_version, _, _) = pkg_info
                    .get(dep_id.as_str())
                    .copied()
                    .unwrap_or(("unknown", "0.0.0", None, None));

                let parent_key = if depth == 0 {
                    format!("{root_name}@{root_version}")
                } else {
                    format!("{name}@{version}")
                };
                let child_key = format!("{dep_name}@{dep_version}");
                edges.push((parent_key, child_key));

                if visited.insert(dep_id.clone()) {
                    let mut child_path = path.clone();
                    child_path.push(dep_name.to_string());
                    queue.push_back((dep_id.clone(), depth + 1, child_path));
                }
            }
        }
    }

    // Cross-reference checksums from Cargo.lock
    let lock_path = target.join("Cargo.lock");
    if lock_path.exists() {
        if let Ok(lock_content) = std::fs::read_to_string(&lock_path) {
            enrich_cargo_checksums(&mut nodes, &lock_content);
        }
    }

    let total_deps = nodes.len();

    Ok(DepTreeSnapshot {
        version: 1,
        timestamp: Utc::now(),
        ecosystem: Ecosystem::Cargo,
        root_package: root_name.to_string(),
        root_version: root_version.to_string(),
        git_ref: None,
        git_branch: None,
        total_deps,
        max_depth,
        nodes,
        edges,
        lockfile_path: "Cargo.lock".to_string(),
        resolution_method: "cargo-metadata".to_string(),
    })
}

/// Parse git branch and commit from a cargo source string.
/// Example: "git+https://github.com/foo/bar?branch=main#abc123"
pub(crate) fn parse_cargo_source_git(source: Option<&str>) -> (Option<String>, Option<String>) {
    let source = match source {
        Some(s) if s.starts_with("git+") => s,
        _ => return (None, None),
    };

    let branch = source.find("?branch=").map(|i| {
        let start = i + 8;
        let end = source[start..]
            .find('#')
            .map(|j| start + j)
            .unwrap_or(source.len());
        source[start..end].to_string()
    });

    let commit = source.rfind('#').map(|i| source[i + 1..].to_string());

    (branch, commit)
}

/// Enrich nodes with checksums from Cargo.lock content.
fn enrich_cargo_checksums(nodes: &mut HashMap<String, DepTreeNode>, lock_content: &str) {
    // Simple parser: look for [[package]] blocks with name, version, checksum
    let mut current_name: Option<String> = None;
    let mut current_version: Option<String> = None;
    let mut current_checksum: Option<String> = None;

    for line in lock_content.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            // Flush previous
            if let (Some(ref name), Some(ref version)) = (&current_name, &current_version) {
                let key = format!("{name}@{version}");
                if let Some(node) = nodes.get_mut(&key) {
                    node.checksum = current_checksum.take();
                }
            }
            current_name = None;
            current_version = None;
            current_checksum = None;
        } else if let Some(rest) = line.strip_prefix("name = ") {
            current_name = Some(rest.trim_matches('"').to_string());
        } else if let Some(rest) = line.strip_prefix("version = ") {
            current_version = Some(rest.trim_matches('"').to_string());
        } else if let Some(rest) = line.strip_prefix("checksum = ") {
            current_checksum = Some(rest.trim_matches('"').to_string());
        }
    }
    // Flush last block
    if let (Some(ref name), Some(ref version)) = (&current_name, &current_version) {
        let key = format!("{name}@{version}");
        if let Some(node) = nodes.get_mut(&key) {
            node.checksum = current_checksum;
        }
    }
}

/// Resolve npm dependency tree from package-lock.json v2/v3.
fn resolve_npm_tree(target: &Path) -> Result<DepTreeSnapshot, ApexError> {
    let lock_path = target.join("package-lock.json");
    let content = std::fs::read_to_string(&lock_path)
        .map_err(|e| ApexError::Detect(format!("read package-lock.json: {e}")))?;

    let lock: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ApexError::Detect(format!("parse package-lock.json: {e}")))?;

    let root_name = lock
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let root_version = lock
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0")
        .to_string();

    let mut nodes: HashMap<String, DepTreeNode> = HashMap::new();
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut max_depth: u32 = 0;

    // v2/v3: packages map with node_modules paths as keys
    if let Some(packages) = lock.get("packages").and_then(|v| v.as_object()) {
        for (path_key, pkg_val) in packages {
            if path_key.is_empty() {
                continue; // skip root entry ""
            }

            // Count depth from node_modules segments
            let depth = path_key.matches("node_modules/").count() as u32;
            if depth > max_depth {
                max_depth = depth;
            }

            // Extract name from last node_modules/ segment
            let name = path_key
                .rsplit("node_modules/")
                .next()
                .unwrap_or(path_key)
                .to_string();

            let version = pkg_val
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0")
                .to_string();

            let integrity = pkg_val
                .get("integrity")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let resolved = pkg_val
                .get("resolved")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let license = pkg_val
                .get("license")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let purl = format!("pkg:npm/{name}@{version}");
            let key = format!("{name}@{version}");

            // Build path from the node_modules nesting
            let mut dep_path = vec![root_name.clone()];
            let segments: Vec<&str> = path_key
                .split("node_modules/")
                .filter(|s| !s.is_empty())
                .collect();
            for seg in &segments {
                dep_path.push(seg.trim_end_matches('/').to_string());
            }

            // Parse child deps
            let dep_names: Vec<String> = pkg_val
                .get("dependencies")
                .and_then(|v| v.as_object())
                .map(|deps| deps.keys().cloned().collect())
                .unwrap_or_default();

            let node = DepTreeNode {
                name: name.clone(),
                version: version.clone(),
                ecosystem: Ecosystem::Npm,
                purl,
                depth,
                path: dep_path,
                checksum: integrity,
                source_url: resolved,
                license,
                git_branch: None,
                git_commit: None,
                dependencies: dep_names,
            };

            if !nodes.contains_key(&key) {
                nodes.insert(key.clone(), node);
            }

            // Add edge from parent
            if depth == 1 {
                edges.push((format!("{root_name}@{root_version}"), key));
            } else if segments.len() >= 2 {
                let parent_name = segments[segments.len() - 2].trim_end_matches('/');
                // Find parent version (best effort)
                if let Some(parent_node) = nodes.values().find(|n| n.name == parent_name) {
                    edges.push((parent_node.key(), key));
                }
            }
        }
    } else {
        // v1 fallback: flat dependencies map
        return resolve_npm_v1_tree(target, &lock, &root_name, &root_version);
    }

    let total_deps = nodes.len();

    Ok(DepTreeSnapshot {
        version: 1,
        timestamp: Utc::now(),
        ecosystem: Ecosystem::Npm,
        root_package: root_name,
        root_version,
        git_ref: None,
        git_branch: None,
        total_deps,
        max_depth,
        nodes,
        edges,
        lockfile_path: "package-lock.json".to_string(),
        resolution_method: "package-lock-v2".to_string(),
    })
}

/// Resolve npm v1 format with nested dependencies objects.
fn resolve_npm_v1_tree(
    _target: &Path,
    lock: &serde_json::Value,
    root_name: &str,
    root_version: &str,
) -> Result<DepTreeSnapshot, ApexError> {
    let mut nodes: HashMap<String, DepTreeNode> = HashMap::new();
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut max_depth: u32 = 0;

    fn walk_deps(
        deps: &serde_json::Map<String, serde_json::Value>,
        parent_key: &str,
        depth: u32,
        path: &[String],
        nodes: &mut HashMap<String, DepTreeNode>,
        edges: &mut Vec<(String, String)>,
        max_depth: &mut u32,
    ) {
        for (name, val) in deps {
            let version = val
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0")
                .to_string();
            let integrity = val
                .get("integrity")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let resolved = val
                .get("resolved")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let key = format!("{name}@{version}");
            let mut dep_path = path.to_vec();
            dep_path.push(name.clone());

            if depth > *max_depth {
                *max_depth = depth;
            }

            let child_deps: Vec<String> = val
                .get("dependencies")
                .and_then(|v| v.as_object())
                .map(|d| d.keys().cloned().collect())
                .unwrap_or_default();

            let node = DepTreeNode {
                name: name.clone(),
                version: version.clone(),
                ecosystem: Ecosystem::Npm,
                purl: format!("pkg:npm/{name}@{version}"),
                depth,
                path: dep_path.clone(),
                checksum: integrity,
                source_url: resolved,
                license: None,
                git_branch: None,
                git_commit: None,
                dependencies: child_deps,
            };

            if !nodes.contains_key(&key) {
                nodes.insert(key.clone(), node);
            }
            edges.push((parent_key.to_string(), key.clone()));

            // Recurse into nested dependencies
            if let Some(nested) = val.get("dependencies").and_then(|v| v.as_object()) {
                walk_deps(nested, &key, depth + 1, &dep_path, nodes, edges, max_depth);
            }
        }
    }

    if let Some(deps) = lock.get("dependencies").and_then(|v| v.as_object()) {
        let root_key = format!("{root_name}@{root_version}");
        walk_deps(
            deps,
            &root_key,
            1,
            &[root_name.to_string()],
            &mut nodes,
            &mut edges,
            &mut max_depth,
        );
    }

    let total_deps = nodes.len();

    Ok(DepTreeSnapshot {
        version: 1,
        timestamp: Utc::now(),
        ecosystem: Ecosystem::Npm,
        root_package: root_name.to_string(),
        root_version: root_version.to_string(),
        git_ref: None,
        git_branch: None,
        total_deps,
        max_depth,
        nodes,
        edges,
        lockfile_path: "package-lock.json".to_string(),
        resolution_method: "package-lock-v1".to_string(),
    })
}

/// Flat fallback for ecosystems without tree lockfiles.
/// All deps get depth=1.
fn resolve_flat_fallback(
    target: &Path,
    ecosystem: Ecosystem,
) -> Result<DepTreeSnapshot, ApexError> {
    let (deps, lockfile_name) = match ecosystem {
        Ecosystem::PyPI => {
            let req_path = target.join("requirements.txt");
            if req_path.exists() {
                let content = std::fs::read_to_string(&req_path)
                    .map_err(|e| ApexError::Detect(format!("read requirements.txt: {e}")))?;
                (
                    crate::lockfile::parse_requirements_str(&content),
                    "requirements.txt",
                )
            } else {
                (vec![], "requirements.txt")
            }
        }
        Ecosystem::Go => {
            let sum_path = target.join("go.sum");
            if sum_path.exists() {
                let content = std::fs::read_to_string(&sum_path)
                    .map_err(|e| ApexError::Detect(format!("read go.sum: {e}")))?;
                let deps = parse_go_sum(&content);
                (deps, "go.sum")
            } else {
                (vec![], "go.sum")
            }
        }
        _ => (vec![], "unknown"),
    };

    let mut nodes: HashMap<String, DepTreeNode> = HashMap::new();
    let mut edges: Vec<(String, String)> = Vec::new();

    let root_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let root_key = format!("{root_name}@0.0.0");

    for dep in &deps {
        let key = format!("{}@{}", dep.name, dep.version);
        let purl = match ecosystem {
            Ecosystem::PyPI => format!("pkg:pypi/{}@{}", dep.name, dep.version),
            Ecosystem::Go => format!("pkg:golang/{}@{}", dep.name, dep.version),
            _ => format!("pkg:generic/{}@{}", dep.name, dep.version),
        };

        let node = DepTreeNode {
            name: dep.name.clone(),
            version: dep.version.clone(),
            ecosystem,
            purl,
            depth: 1,
            path: vec![root_name.clone(), dep.name.clone()],
            checksum: dep.checksum.clone(),
            source_url: dep.source_url.clone(),
            license: dep.license.clone(),
            git_branch: None,
            git_commit: None,
            dependencies: vec![],
        };

        if !nodes.contains_key(&key) {
            nodes.insert(key.clone(), node);
            edges.push((root_key.clone(), key));
        }
    }

    let total_deps = nodes.len();

    Ok(DepTreeSnapshot {
        version: 1,
        timestamp: Utc::now(),
        ecosystem,
        root_package: root_name,
        root_version: "0.0.0".to_string(),
        git_ref: None,
        git_branch: None,
        total_deps,
        max_depth: if total_deps > 0 { 1 } else { 0 },
        nodes,
        edges,
        lockfile_path: lockfile_name.to_string(),
        resolution_method: "flat-fallback".to_string(),
    })
}

/// Parse go.sum into flat dependency list.
fn parse_go_sum(content: &str) -> Vec<crate::lockfile::Dependency> {
    let mut seen = HashSet::new();
    let mut deps = Vec::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let module = parts[0];
        let version = parts[1]
            .trim_start_matches('v')
            .split('/')
            .next()
            .unwrap_or(parts[1]);
        let checksum = parts[2];

        let key = format!("{module}@{version}");
        if seen.insert(key) {
            deps.push(crate::lockfile::Dependency {
                name: module.to_string(),
                version: version.to_string(),
                purl: format!("pkg:golang/{module}@{version}"),
                source_url: None,
                checksum: Some(checksum.to_string()),
                license: None,
            });
        }
    }

    deps
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detect_ecosystem_cargo() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), "").unwrap();
        assert_eq!(detect_ecosystem(dir.path()), Some(Ecosystem::Cargo));
    }

    #[test]
    fn detect_ecosystem_npm() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        assert_eq!(detect_ecosystem(dir.path()), Some(Ecosystem::Npm));
    }

    #[test]
    fn detect_ecosystem_go() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("go.sum"), "").unwrap();
        assert_eq!(detect_ecosystem(dir.path()), Some(Ecosystem::Go));
    }

    #[test]
    fn detect_ecosystem_pypi_requirements() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();
        assert_eq!(detect_ecosystem(dir.path()), Some(Ecosystem::PyPI));
    }

    #[test]
    fn detect_ecosystem_pypi_pipfile() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Pipfile.lock"), "").unwrap();
        assert_eq!(detect_ecosystem(dir.path()), Some(Ecosystem::PyPI));
    }

    #[test]
    fn detect_ecosystem_rubygems() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Gemfile.lock"), "").unwrap();
        assert_eq!(detect_ecosystem(dir.path()), Some(Ecosystem::RubyGems));
    }

    #[test]
    fn detect_ecosystem_composer() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("composer.lock"), "").unwrap();
        assert_eq!(detect_ecosystem(dir.path()), Some(Ecosystem::Composer));
    }

    #[test]
    fn detect_ecosystem_nuget() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("packages.lock.json"), "").unwrap();
        assert_eq!(detect_ecosystem(dir.path()), Some(Ecosystem::NuGet));
    }

    #[test]
    fn detect_ecosystem_none() {
        let dir = TempDir::new().unwrap();
        assert_eq!(detect_ecosystem(dir.path()), None);
    }

    #[test]
    fn detect_ecosystem_priority_cargo_over_npm() {
        // When both exist, Cargo wins (checked first)
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), "").unwrap();
        std::fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        assert_eq!(detect_ecosystem(dir.path()), Some(Ecosystem::Cargo));
    }

    #[test]
    fn parse_cargo_source_git_with_branch_and_commit() {
        let source = Some("git+https://github.com/foo/bar?branch=main#abc123def");
        let (branch, commit) = parse_cargo_source_git(source);
        assert_eq!(branch, Some("main".to_string()));
        assert_eq!(commit, Some("abc123def".to_string()));
    }

    #[test]
    fn parse_cargo_source_git_commit_only() {
        let source = Some("git+https://github.com/foo/bar#abc123def");
        let (branch, commit) = parse_cargo_source_git(source);
        assert_eq!(branch, None);
        assert_eq!(commit, Some("abc123def".to_string()));
    }

    #[test]
    fn parse_cargo_source_git_branch_no_commit() {
        let source = Some("git+https://github.com/foo/bar?branch=develop");
        let (branch, commit) = parse_cargo_source_git(source);
        assert_eq!(branch, Some("develop".to_string()));
        assert_eq!(commit, None);
    }

    #[test]
    fn parse_cargo_source_git_not_git() {
        let source = Some("registry+https://github.com/rust-lang/crates.io-index");
        let (branch, commit) = parse_cargo_source_git(source);
        assert_eq!(branch, None);
        assert_eq!(commit, None);
    }

    #[test]
    fn parse_cargo_source_git_none() {
        let (branch, commit) = parse_cargo_source_git(None);
        assert_eq!(branch, None);
        assert_eq!(commit, None);
    }

    #[test]
    fn parse_go_sum_basic() {
        let content = "golang.org/x/text v0.14.0 h1:abc123\ngolang.org/x/text v0.14.0/go.mod h1:def456\ngithub.com/foo/bar v1.2.3 h1:xyz789\n";
        let deps = parse_go_sum(content);
        // Dedup: "golang.org/x/text@0.14.0" appears twice but only counted once
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "golang.org/x/text");
        assert_eq!(deps[0].version, "0.14.0");
        assert_eq!(deps[0].checksum, Some("h1:abc123".to_string()));
        assert_eq!(deps[1].name, "github.com/foo/bar");
        assert_eq!(deps[1].version, "1.2.3");
    }

    #[test]
    fn parse_go_sum_empty() {
        let deps = parse_go_sum("");
        assert!(deps.is_empty());
    }

    #[test]
    fn resolve_flat_fallback_pypi() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("requirements.txt"),
            "requests==2.31.0\nflask>=2.0.0\n",
        )
        .unwrap();

        let snapshot = resolve_flat_fallback(dir.path(), Ecosystem::PyPI).unwrap();
        assert_eq!(snapshot.ecosystem, Ecosystem::PyPI);
        assert_eq!(snapshot.total_deps, 2);
        assert_eq!(snapshot.max_depth, 1);
        assert_eq!(snapshot.resolution_method, "flat-fallback");
        assert_eq!(snapshot.lockfile_path, "requirements.txt");

        let req = snapshot.find_by_name("requests").expect("should find requests");
        assert_eq!(req.version, "2.31.0");
        assert_eq!(req.depth, 1);
        assert!(req.purl.starts_with("pkg:pypi/"));
    }

    #[test]
    fn resolve_flat_fallback_empty() {
        let dir = TempDir::new().unwrap();
        // No requirements.txt at all
        let snapshot = resolve_flat_fallback(dir.path(), Ecosystem::PyPI).unwrap();
        assert_eq!(snapshot.total_deps, 0);
        assert_eq!(snapshot.max_depth, 0);
    }

    #[test]
    fn resolve_flat_fallback_go() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("go.sum"),
            "golang.org/x/text v0.14.0 h1:abc123\n",
        )
        .unwrap();

        let snapshot = resolve_flat_fallback(dir.path(), Ecosystem::Go).unwrap();
        assert_eq!(snapshot.ecosystem, Ecosystem::Go);
        assert_eq!(snapshot.total_deps, 1);
        assert_eq!(snapshot.lockfile_path, "go.sum");

        let node = snapshot
            .find_by_name("golang.org/x/text")
            .expect("should find go dep");
        assert!(node.purl.starts_with("pkg:golang/"));
    }

    #[test]
    fn resolve_npm_v1_fallback() {
        let dir = TempDir::new().unwrap();
        let lock_content = r#"{
            "name": "test-app",
            "version": "1.0.0",
            "lockfileVersion": 1,
            "dependencies": {
                "express": {
                    "version": "4.18.2",
                    "resolved": "https://registry.npmjs.org/express/-/express-4.18.2.tgz",
                    "integrity": "sha512-abc",
                    "dependencies": {
                        "body-parser": {
                            "version": "1.20.1",
                            "resolved": "https://registry.npmjs.org/body-parser/-/body-parser-1.20.1.tgz"
                        }
                    }
                },
                "lodash": {
                    "version": "4.17.21"
                }
            }
        }"#;
        std::fs::write(dir.path().join("package-lock.json"), lock_content).unwrap();

        let snapshot = resolve_npm_tree(dir.path()).unwrap();
        assert_eq!(snapshot.ecosystem, Ecosystem::Npm);
        assert_eq!(snapshot.root_package, "test-app");
        assert_eq!(snapshot.root_version, "1.0.0");
        assert_eq!(snapshot.resolution_method, "package-lock-v1");
        assert_eq!(snapshot.total_deps, 3);
        assert_eq!(snapshot.max_depth, 2);

        let express = snapshot
            .find_by_name("express")
            .expect("should find express");
        assert_eq!(express.depth, 1);
        assert_eq!(express.checksum, Some("sha512-abc".to_string()));

        let body_parser = snapshot
            .find_by_name("body-parser")
            .expect("should find body-parser");
        assert_eq!(body_parser.depth, 2);
    }

    #[test]
    fn resolve_npm_v2() {
        let dir = TempDir::new().unwrap();
        let lock_content = r#"{
            "name": "my-app",
            "version": "2.0.0",
            "lockfileVersion": 3,
            "packages": {
                "": {
                    "name": "my-app",
                    "version": "2.0.0"
                },
                "node_modules/express": {
                    "version": "4.18.2",
                    "resolved": "https://registry.npmjs.org/express/-/express-4.18.2.tgz",
                    "integrity": "sha512-abc",
                    "license": "MIT",
                    "dependencies": {
                        "body-parser": "^1.20.0"
                    }
                },
                "node_modules/express/node_modules/body-parser": {
                    "version": "1.20.1",
                    "resolved": "https://registry.npmjs.org/body-parser/-/body-parser-1.20.1.tgz",
                    "integrity": "sha512-def"
                }
            }
        }"#;
        std::fs::write(dir.path().join("package-lock.json"), lock_content).unwrap();

        let snapshot = resolve_npm_tree(dir.path()).unwrap();
        assert_eq!(snapshot.ecosystem, Ecosystem::Npm);
        assert_eq!(snapshot.root_package, "my-app");
        assert_eq!(snapshot.resolution_method, "package-lock-v2");
        assert_eq!(snapshot.total_deps, 2);
        assert_eq!(snapshot.max_depth, 2);

        let express = snapshot
            .find_by_name("express")
            .expect("should find express");
        assert_eq!(express.depth, 1);
        assert_eq!(express.license, Some("MIT".to_string()));
    }

    #[test]
    fn enrich_cargo_checksums_works() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "serde@1.0.200".to_string(),
            DepTreeNode {
                name: "serde".to_string(),
                version: "1.0.200".to_string(),
                ecosystem: Ecosystem::Cargo,
                purl: "pkg:cargo/serde@1.0.200".to_string(),
                depth: 1,
                path: vec!["root".to_string(), "serde".to_string()],
                checksum: None,
                source_url: None,
                license: None,
                git_branch: None,
                git_commit: None,
                dependencies: vec![],
            },
        );

        let lock_content = r#"
[[package]]
name = "serde"
version = "1.0.200"
checksum = "deadbeef"

[[package]]
name = "tokio"
version = "1.37.0"
checksum = "cafebabe"
"#;
        enrich_cargo_checksums(&mut nodes, lock_content);

        assert_eq!(
            nodes.get("serde@1.0.200").unwrap().checksum,
            Some("deadbeef".to_string())
        );
    }
}
