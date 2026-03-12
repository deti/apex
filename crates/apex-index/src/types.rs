use apex_core::types::{BranchId, ExecutionStatus, Language};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Per-test trace
// ---------------------------------------------------------------------------

/// Branch footprint of a single test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestTrace {
    pub test_name: String,
    pub branches: Vec<BranchId>,
    pub duration_ms: u64,
    pub status: ExecutionStatus,
}

// ---------------------------------------------------------------------------
// Branch profile (aggregate)
// ---------------------------------------------------------------------------

/// Aggregate statistics for a single branch across all tests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchProfile {
    pub branch: BranchId,
    /// Total hit count across all tests.
    pub hit_count: u64,
    /// Number of distinct tests that reach this branch.
    pub test_count: usize,
    /// Names of tests that reach this branch.
    pub test_names: Vec<String>,
}

// ---------------------------------------------------------------------------
// The full index
// ---------------------------------------------------------------------------

/// Persistent per-test branch mapping for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchIndex {
    pub traces: Vec<TestTrace>,
    pub profiles: HashMap<String, BranchProfile>,
    pub file_paths: HashMap<u64, PathBuf>,
    pub total_branches: usize,
    pub covered_branches: usize,
    pub created_at: String,
    pub language: Language,
    pub target_root: PathBuf,
    /// SHA-256 of concatenated source file contents for staleness detection.
    pub source_hash: String,
}

impl BranchIndex {
    /// Build profiles from traces.
    pub fn build_profiles(traces: &[TestTrace]) -> HashMap<String, BranchProfile> {
        let mut map: HashMap<String, BranchProfile> = HashMap::new();

        for trace in traces {
            for branch in &trace.branches {
                let key = branch_key(branch);
                let profile = map.entry(key).or_insert_with(|| BranchProfile {
                    branch: branch.clone(),
                    hit_count: 0,
                    test_count: 0,
                    test_names: Vec::new(),
                });
                profile.hit_count += 1;
                profile.test_count += 1;
                profile.test_names.push(trace.test_name.clone());
            }
        }

        map
    }

    /// Compute coverage percentage.
    pub fn coverage_percent(&self) -> f64 {
        if self.total_branches == 0 {
            return 100.0;
        }
        (self.covered_branches as f64 / self.total_branches as f64) * 100.0
    }

    /// Get all branches that are never hit by any test.
    pub fn dead_branches(&self) -> Vec<&BranchProfile> {
        // Branches that exist in total set but have no profile entry are dead.
        // But since profiles only contain hit branches, we need a different approach.
        // Dead branches = total branches - branches in any profile.
        // This method returns profiles with lowest hit counts for analysis.
        // For true dead branches, see dead_branch_ids().
        self.profiles.values().filter(|p| p.hit_count == 0).collect()
    }

    /// Get BranchIds that appear in no test trace.
    pub fn uncovered_branch_ids(&self, all_branches: &[BranchId]) -> Vec<BranchId> {
        let covered: HashSet<String> = self.profiles.keys().cloned().collect();
        all_branches
            .iter()
            .filter(|b| !covered.contains(&branch_key(b)))
            .cloned()
            .collect()
    }

    /// Persist the index to a JSON file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Load index from a JSON file.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Check if the index is stale (source files changed since index was built).
    pub fn is_stale(&self, current_hash: &str) -> bool {
        self.source_hash != current_hash
    }
}

/// Stable string key for a BranchId (for HashMap keying in profiles).
pub fn branch_key(b: &BranchId) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        b.file_id,
        b.line,
        b.col,
        b.direction,
        b.condition_index.unwrap_or(255)
    )
}

/// Compute SHA-256 hash of source files in a directory for staleness detection.
pub fn hash_source_files(root: &Path, language: Language) -> String {
    let extensions: &[&str] = match language {
        Language::Python => &["py"],
        Language::Rust => &["rs"],
        Language::JavaScript => &["js", "ts"],
        Language::Java => &["java"],
        Language::C => &["c", "h"],
        Language::Wasm => &["wat", "wasm"],
        Language::Ruby => &["rb"],
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    collect_source_files(root, extensions, &mut paths);
    paths.sort();

    let mut hasher = Sha256::new();
    for path in &paths {
        if let Ok(content) = std::fs::read(path) {
            hasher.update(path.to_string_lossy().as_bytes());
            hasher.update(&content);
        }
    }

    format!("{:x}", hasher.finalize())
}

fn collect_source_files(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden dirs, build artifacts, venvs
        if name_str.starts_with('.')
            || name_str == "target"
            || name_str == "node_modules"
            || name_str == "__pycache__"
            || name_str == ".venv"
            || name_str == "venv"
        {
            continue;
        }

        if path.is_dir() {
            collect_source_files(&path, extensions, out);
        } else if let Some(ext) = path.extension() {
            if extensions.iter().any(|e| ext == *e) {
                out.push(path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_branch(file_id: u64, line: u32, direction: u8) -> BranchId {
        BranchId::new(file_id, line, 0, direction)
    }

    #[test]
    fn branch_key_deterministic() {
        let b = make_branch(42, 10, 0);
        assert_eq!(branch_key(&b), branch_key(&b));
    }

    #[test]
    fn branch_key_differs_by_direction() {
        let a = make_branch(42, 10, 0);
        let b = make_branch(42, 10, 1);
        assert_ne!(branch_key(&a), branch_key(&b));
    }

    #[test]
    fn build_profiles_empty() {
        let profiles = BranchIndex::build_profiles(&[]);
        assert!(profiles.is_empty());
    }

    #[test]
    fn build_profiles_counts_correctly() {
        let traces = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![make_branch(1, 10, 0), make_branch(1, 20, 0)],
                duration_ms: 100,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![make_branch(1, 10, 0), make_branch(2, 5, 1)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
        ];

        let profiles = BranchIndex::build_profiles(&traces);

        let key_10 = branch_key(&make_branch(1, 10, 0));
        let profile = &profiles[&key_10];
        assert_eq!(profile.hit_count, 2);
        assert_eq!(profile.test_count, 2);
        assert_eq!(profile.test_names, vec!["test_a", "test_b"]);

        let key_20 = branch_key(&make_branch(1, 20, 0));
        assert_eq!(profiles[&key_20].test_count, 1);
    }

    #[test]
    fn coverage_percent_empty() {
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 0,
            covered_branches: 0,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        assert!((index.coverage_percent() - 100.0).abs() < 0.01);
    }

    #[test]
    fn coverage_percent_partial() {
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 100,
            covered_branches: 75,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        assert!((index.coverage_percent() - 75.0).abs() < 0.01);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let index = BranchIndex {
            traces: vec![TestTrace {
                test_name: "test_x".into(),
                branches: vec![make_branch(1, 10, 0)],
                duration_ms: 42,
                status: ExecutionStatus::Pass,
            }],
            profiles: BranchIndex::build_profiles(&[TestTrace {
                test_name: "test_x".into(),
                branches: vec![make_branch(1, 10, 0)],
                duration_ms: 42,
                status: ExecutionStatus::Pass,
            }]),
            file_paths: HashMap::from([(1u64, PathBuf::from("src/lib.py"))]),
            total_branches: 5,
            covered_branches: 3,
            created_at: "2026-03-12T00:00:00Z".into(),
            language: Language::Python,
            target_root: PathBuf::from("/tmp/test"),
            source_hash: "abc123".into(),
        };

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".apex/index.json");

        index.save(&path).unwrap();
        let loaded = BranchIndex::load(&path).unwrap();

        assert_eq!(loaded.traces.len(), 1);
        assert_eq!(loaded.total_branches, 5);
        assert_eq!(loaded.covered_branches, 3);
        assert_eq!(loaded.source_hash, "abc123");
    }

    #[test]
    fn is_stale_detects_change() {
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 0,
            covered_branches: 0,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: "old_hash".into(),
        };
        assert!(index.is_stale("new_hash"));
        assert!(!index.is_stale("old_hash"));
    }

    #[test]
    fn dead_branches_returns_zero_hit_profiles() {
        let mut profiles = HashMap::new();
        // Insert a profile with hit_count == 0 (dead)
        let dead_branch = make_branch(1, 30, 0);
        let dead_key = branch_key(&dead_branch);
        profiles.insert(
            dead_key.clone(),
            BranchProfile {
                branch: dead_branch,
                hit_count: 0,
                test_count: 0,
                test_names: vec![],
            },
        );
        // Insert a profile with hit_count > 0 (alive)
        let live_branch = make_branch(1, 40, 0);
        let live_key = branch_key(&live_branch);
        profiles.insert(
            live_key,
            BranchProfile {
                branch: live_branch,
                hit_count: 3,
                test_count: 2,
                test_names: vec!["t1".into(), "t2".into()],
            },
        );

        let index = BranchIndex {
            traces: vec![],
            profiles,
            file_paths: HashMap::new(),
            total_branches: 2,
            covered_branches: 1,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let dead = index.dead_branches();
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].hit_count, 0);
        assert_eq!(branch_key(&dead[0].branch), dead_key);
    }

    #[test]
    fn dead_branches_empty_when_all_hit() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![make_branch(1, 10, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            traces: traces.clone(),
            profiles: BranchIndex::build_profiles(&traces),
            file_paths: HashMap::new(),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        assert!(index.dead_branches().is_empty());
    }

    #[test]
    fn load_nonexistent_file_returns_error() {
        let result = BranchIndex::load(Path::new("/nonexistent/path/index.json"));
        assert!(result.is_err());
    }

    #[test]
    fn load_invalid_json_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.json");
        std::fs::write(&path, "not valid json {{{").unwrap();

        let result = BranchIndex::load(&path);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn save_creates_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a/b/c/index.json");

        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 0,
            covered_branches: 0,
            created_at: String::new(),
            language: Language::Rust,
            target_root: PathBuf::new(),
            source_hash: "deadbeef".into(),
        };

        index.save(&path).unwrap();
        assert!(path.exists());

        let loaded = BranchIndex::load(&path).unwrap();
        assert_eq!(loaded.source_hash, "deadbeef");
        assert!(matches!(loaded.language, Language::Rust));
    }

    #[test]
    fn branch_key_includes_condition_index() {
        let mut b = make_branch(1, 10, 0);
        b.condition_index = Some(3);
        let key = branch_key(&b);
        assert!(key.ends_with(":3"), "expected condition_index in key, got: {key}");

        // Without condition_index, should end with :255
        let b2 = make_branch(1, 10, 0);
        let key2 = branch_key(&b2);
        assert!(key2.ends_with(":255"), "expected 255 sentinel, got: {key2}");
    }

    #[test]
    fn hash_source_files_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("hello.py");
        std::fs::write(&src, "print('hello')").unwrap();

        let h1 = hash_source_files(tmp.path(), Language::Python);
        let h2 = hash_source_files(tmp.path(), Language::Python);
        assert_eq!(h1, h2);
        assert!(!h1.is_empty());
    }

    #[test]
    fn hash_source_files_changes_with_content() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.py");
        std::fs::write(&src, "x = 1").unwrap();
        let h1 = hash_source_files(tmp.path(), Language::Python);

        std::fs::write(&src, "x = 2").unwrap();
        let h2 = hash_source_files(tmp.path(), Language::Python);
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_source_files_ignores_wrong_extension() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("data.txt"), "not python").unwrap();

        let h = hash_source_files(tmp.path(), Language::Python);
        // With no matching files, should still produce a hash (empty digest)
        assert!(!h.is_empty());

        // Now add a .py file and confirm it changes
        std::fs::write(tmp.path().join("main.py"), "pass").unwrap();
        let h2 = hash_source_files(tmp.path(), Language::Python);
        assert_ne!(h, h2);
    }

    #[test]
    fn hash_source_files_skips_hidden_and_special_dirs() {
        let tmp = tempfile::tempdir().unwrap();

        // Only this top-level file should be hashed
        std::fs::write(tmp.path().join("main.py"), "print(1)").unwrap();
        let h_baseline = hash_source_files(tmp.path(), Language::Python);

        // Adding files in hidden/special dirs should NOT change the hash
        let hidden = tmp.path().join(".hidden");
        std::fs::create_dir(&hidden).unwrap();
        std::fs::write(hidden.join("secret.py"), "secret").unwrap();

        let cache = tmp.path().join("__pycache__");
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(cache.join("mod.py"), "cached").unwrap();

        let nm = tmp.path().join("node_modules");
        std::fs::create_dir(&nm).unwrap();
        std::fs::write(nm.join("dep.py"), "dep").unwrap();

        let h_after = hash_source_files(tmp.path(), Language::Python);
        assert_eq!(h_baseline, h_after, "hidden/special dir files should be skipped");
    }

    #[test]
    fn hash_source_files_nonexistent_dir() {
        let h = hash_source_files(Path::new("/nonexistent/dir"), Language::Python);
        // Should not panic, returns empty-input hash
        assert!(!h.is_empty());
    }

    #[test]
    fn hash_source_files_language_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("lib.rs"), "fn main() {}").unwrap();
        std::fs::write(tmp.path().join("lib.py"), "pass").unwrap();

        let h_rust = hash_source_files(tmp.path(), Language::Rust);
        let h_python = hash_source_files(tmp.path(), Language::Python);
        // Different languages pick different files, so hashes differ
        assert_ne!(h_rust, h_python);
    }

    #[test]
    fn uncovered_branch_ids_finds_missing() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            traces: traces.clone(),
            profiles: BranchIndex::build_profiles(&traces),
            file_paths: HashMap::new(),
            total_branches: 3,
            covered_branches: 1,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let all = vec![
            make_branch(1, 10, 0), // covered
            make_branch(1, 20, 0), // not covered
            make_branch(2, 5, 1),  // not covered
        ];

        let uncovered = index.uncovered_branch_ids(&all);
        assert_eq!(uncovered.len(), 2);
    }

    #[test]
    fn uncovered_branch_ids_empty_all_branches() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![make_branch(1, 10, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            traces: traces.clone(),
            profiles: BranchIndex::build_profiles(&traces),
            file_paths: HashMap::new(),
            total_branches: 1,
            covered_branches: 1,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let uncovered = index.uncovered_branch_ids(&[]);
        assert!(uncovered.is_empty());
    }

    #[test]
    fn uncovered_branch_ids_all_covered() {
        let traces = vec![TestTrace {
            test_name: "t1".into(),
            branches: vec![make_branch(1, 10, 0), make_branch(1, 20, 1)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            traces: traces.clone(),
            profiles: BranchIndex::build_profiles(&traces),
            file_paths: HashMap::new(),
            total_branches: 2,
            covered_branches: 2,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        let all = vec![make_branch(1, 10, 0), make_branch(1, 20, 1)];
        let uncovered = index.uncovered_branch_ids(&all);
        assert!(uncovered.is_empty());
    }

    #[test]
    fn coverage_percent_100_when_all_covered() {
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 10,
            covered_branches: 10,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        assert!((index.coverage_percent() - 100.0).abs() < 0.01);
    }

    #[test]
    fn save_path_with_no_parent() {
        // Path::new("index.json") has parent = Some("") which is empty but create_dir_all("") is a no-op
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("index.json");
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 0,
            covered_branches: 0,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        index.save(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn hash_source_files_javascript_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("app.js"), "console.log(1);").unwrap();
        std::fs::write(tmp.path().join("types.ts"), "type Foo = string;").unwrap();
        std::fs::write(tmp.path().join("other.py"), "pass").unwrap();

        let h_js = hash_source_files(tmp.path(), Language::JavaScript);
        let h_py = hash_source_files(tmp.path(), Language::Python);
        assert_ne!(h_js, h_py);
    }

    #[test]
    fn hash_source_files_java_extension() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Main.java"), "class Main {}").unwrap();

        let h = hash_source_files(tmp.path(), Language::Java);
        assert!(!h.is_empty());
        // Java file not picked up by Python extension
        let h_py = hash_source_files(tmp.path(), Language::Python);
        assert_ne!(h, h_py);
    }

    #[test]
    fn hash_source_files_c_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("lib.c"), "int main() { return 0; }").unwrap();
        std::fs::write(tmp.path().join("lib.h"), "#pragma once").unwrap();

        let h = hash_source_files(tmp.path(), Language::C);
        assert!(!h.is_empty());
    }

    #[test]
    fn hash_source_files_wasm_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("module.wat"), "(module)").unwrap();

        let h = hash_source_files(tmp.path(), Language::Wasm);
        assert!(!h.is_empty());
    }

    #[test]
    fn hash_source_files_ruby_extension() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("app.rb"), "puts 'hello'").unwrap();

        let h = hash_source_files(tmp.path(), Language::Ruby);
        assert!(!h.is_empty());
    }

    #[test]
    fn hash_source_files_skips_target_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
        let h1 = hash_source_files(tmp.path(), Language::Rust);

        // Files in "target" dir should be skipped
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("build.rs"), "fn main() {}").unwrap();

        let h2 = hash_source_files(tmp.path(), Language::Rust);
        assert_eq!(h1, h2, "target dir files should be skipped");
    }

    #[test]
    fn hash_source_files_skips_venv_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("app.py"), "pass").unwrap();
        let h1 = hash_source_files(tmp.path(), Language::Python);

        // Files in "venv" dir should be skipped
        let venv = tmp.path().join("venv");
        std::fs::create_dir(&venv).unwrap();
        std::fs::write(venv.join("site.py"), "pass").unwrap();

        let h2 = hash_source_files(tmp.path(), Language::Python);
        assert_eq!(h1, h2, "venv dir files should be skipped");
    }

    #[test]
    fn hash_source_files_skips_dot_venv_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("app.py"), "pass").unwrap();
        let h1 = hash_source_files(tmp.path(), Language::Python);

        // Files in ".venv" dir should also be skipped (starts with '.' check)
        let dotenv = tmp.path().join(".venv");
        std::fs::create_dir(&dotenv).unwrap();
        std::fs::write(dotenv.join("env.py"), "pass").unwrap();

        let h2 = hash_source_files(tmp.path(), Language::Python);
        assert_eq!(h1, h2, ".venv dir files should be skipped");
    }

    #[test]
    fn build_profiles_multiple_traces_same_branch() {
        // Branch hit by 3 different tests -> hit_count = 3, test_count = 3
        let b = make_branch(5, 42, 0);
        let traces = vec![
            TestTrace { test_name: "a".into(), branches: vec![b.clone()], duration_ms: 1, status: ExecutionStatus::Pass },
            TestTrace { test_name: "b".into(), branches: vec![b.clone()], duration_ms: 1, status: ExecutionStatus::Pass },
            TestTrace { test_name: "c".into(), branches: vec![b.clone()], duration_ms: 1, status: ExecutionStatus::Pass },
        ];
        let profiles = BranchIndex::build_profiles(&traces);
        let key = branch_key(&b);
        let p = &profiles[&key];
        assert_eq!(p.hit_count, 3);
        assert_eq!(p.test_count, 3);
        assert_eq!(p.test_names, vec!["a", "b", "c"]);
    }

    #[test]
    fn branch_key_differs_by_file_id() {
        let a = make_branch(1, 10, 0);
        let b = make_branch(2, 10, 0);
        assert_ne!(branch_key(&a), branch_key(&b));
    }

    #[test]
    fn branch_key_differs_by_line() {
        let a = make_branch(1, 10, 0);
        let b = make_branch(1, 11, 0);
        assert_ne!(branch_key(&a), branch_key(&b));
    }

    #[test]
    fn dead_branches_empty_profiles() {
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 0,
            covered_branches: 0,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        assert!(index.dead_branches().is_empty());
    }

    #[test]
    fn hash_source_files_recursive_subdir() {
        // Files in subdirectories should be included
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("src");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("lib.py"), "pass").unwrap();

        let h = hash_source_files(tmp.path(), Language::Python);
        assert!(!h.is_empty());

        // Adding another file changes the hash
        std::fs::write(sub.join("other.py"), "x = 1").unwrap();
        let h2 = hash_source_files(tmp.path(), Language::Python);
        assert_ne!(h, h2);
    }
}
