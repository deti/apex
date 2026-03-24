//! Incremental coverage cache — skip re-instrumenting unchanged files.
//!
//! Stores FNV-1a hashes of file content keyed by path in `.apex/cache/coverage.json`.
//! On subsequent runs, `is_fresh` returns `true` for files whose content hasn't changed,
//! avoiding redundant instrumentation passes.

use crate::error::{ApexError, Result};
use crate::hash::fnv1a_hash;
use crate::types::BranchId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Persisted form of all cache entries.  Stored as a flat JSON object where
/// keys are canonical (slash-separated) path strings.
#[derive(Debug, Serialize, Deserialize, Default)]
struct CacheFile {
    entries: HashMap<String, CacheEntry>,
}

/// Per-file cache record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// FNV-1a hash of the file content at cache time.
    pub content_hash: u64,
    /// Branch IDs discovered during instrumentation of this file.
    pub branches: Vec<BranchId>,
    /// Unix timestamp (seconds) when this entry was written.
    pub timestamp: u64,
}

/// Incremental coverage cache backed by `.apex/cache/coverage.json`.
pub struct CoverageCache {
    entries: HashMap<PathBuf, CacheEntry>,
    cache_dir: PathBuf,
}

impl CoverageCache {
    /// Load the cache from `<cache_dir>/coverage.json`.
    ///
    /// If the file does not exist or cannot be parsed, an empty cache is returned
    /// without error — a cold start is always valid.
    pub fn load(cache_dir: &Path) -> Self {
        let path = cache_dir.join("coverage.json");
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<CacheFile>(&s).ok())
            .map(|cf| {
                cf.entries
                    .into_iter()
                    .map(|(k, v)| (PathBuf::from(k), v))
                    .collect()
            })
            .unwrap_or_default();

        CoverageCache {
            entries,
            cache_dir: cache_dir.to_path_buf(),
        }
    }

    /// Return `true` when the cached entry for `path` was built from the same
    /// content as `content` (i.e. instrumentation can be skipped).
    pub fn is_fresh(&self, path: &Path, content: &str) -> bool {
        let hash = fnv1a_hash(content);
        self.entries
            .get(path)
            .map(|e| e.content_hash == hash)
            .unwrap_or(false)
    }

    /// Return the cached branches for `path`, if the entry is still fresh.
    pub fn cached_branches(&self, path: &Path, content: &str) -> Option<&Vec<BranchId>> {
        if self.is_fresh(path, content) {
            self.entries.get(path).map(|e| &e.branches)
        } else {
            None
        }
    }

    /// Update the cache entry for `path` with the new content and branch list.
    pub fn update(&mut self, path: &Path, content: &str, branches: Vec<BranchId>) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.entries.insert(
            path.to_path_buf(),
            CacheEntry {
                content_hash: fnv1a_hash(content),
                branches,
                timestamp,
            },
        );
    }

    /// Persist the cache to `<cache_dir>/coverage.json`.
    ///
    /// Creates the cache directory if it does not exist.
    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| {
            ApexError::Config(format!(
                "create cache dir {}: {e}",
                self.cache_dir.display()
            ))
        })?;

        let path = self.cache_dir.join("coverage.json");
        let cache_file = CacheFile {
            entries: self
                .entries
                .iter()
                .map(|(k, v)| {
                    // Normalise to forward-slash strings for portability.
                    let key = k.to_string_lossy().replace('\\', "/");
                    (key, v.clone())
                })
                .collect(),
        };

        let json = serde_json::to_string_pretty(&cache_file)
            .map_err(|e| ApexError::Config(format!("serialise coverage cache: {e}")))?;

        std::fs::write(&path, json)
            .map_err(|e| ApexError::Config(format!("write cache {}: {e}", path.display())))?;

        Ok(())
    }

    /// Total number of entries currently held in memory.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::fnv1a_hash;
    use crate::types::BranchId;

    fn sample_branches() -> Vec<BranchId> {
        vec![
            BranchId::new(fnv1a_hash("src/lib.rs"), 10, 4, 0),
            BranchId::new(fnv1a_hash("src/lib.rs"), 10, 4, 1),
        ]
    }

    // ---- is_fresh ----------------------------------------------------------

    #[test]
    fn fresh_after_update() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverageCache::load(dir.path());
        let path = PathBuf::from("src/lib.rs");
        let content = "fn foo() {}";

        assert!(
            !cache.is_fresh(&path, content),
            "empty cache is never fresh"
        );

        cache.update(&path, content, sample_branches());
        assert!(cache.is_fresh(&path, content), "same content → fresh");
    }

    #[test]
    fn stale_after_modification() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverageCache::load(dir.path());
        let path = PathBuf::from("src/lib.rs");

        cache.update(&path, "fn old() {}", sample_branches());
        assert!(
            !cache.is_fresh(&path, "fn new() {}"),
            "different content → stale"
        );
    }

    // ---- save / load roundtrip --------------------------------------------

    #[test]
    fn roundtrip_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = PathBuf::from("src/main.rs");
        let content = "fn main() { println!(\"hello\"); }";
        let branches = sample_branches();

        let mut cache = CoverageCache::load(dir.path());
        cache.update(&path, content, branches.clone());
        cache.save().expect("save should succeed");

        let loaded = CoverageCache::load(dir.path());
        assert_eq!(loaded.len(), 1, "one entry persisted");
        assert!(loaded.is_fresh(&path, content), "loaded entry is fresh");

        let cached = loaded.cached_branches(&path, content).unwrap();
        assert_eq!(cached.len(), branches.len());
        assert_eq!(cached[0].line, branches[0].line);
        assert_eq!(cached[1].direction, branches[1].direction);
    }

    #[test]
    fn load_missing_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        // Point at a non-existent sub-directory — should not panic.
        let cache = CoverageCache::load(&dir.path().join("nonexistent"));
        assert!(cache.is_empty());
    }

    #[test]
    fn save_creates_cache_dir() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("nested").join("cache");
        let cache = CoverageCache::load(&cache_dir);
        cache.save().expect("save should create parent dirs");
        assert!(cache_dir.join("coverage.json").exists());
    }

    // ---- multiple files ---------------------------------------------------

    #[test]
    fn multiple_files_independent() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverageCache::load(dir.path());

        let path_a = PathBuf::from("src/a.rs");
        let path_b = PathBuf::from("src/b.rs");

        cache.update(&path_a, "fn a() {}", vec![]);
        cache.update(&path_b, "fn b() {}", vec![]);
        cache.save().unwrap();

        let loaded = CoverageCache::load(dir.path());
        assert_eq!(loaded.len(), 2);
        assert!(loaded.is_fresh(&path_a, "fn a() {}"));
        assert!(loaded.is_fresh(&path_b, "fn b() {}"));
        assert!(!loaded.is_fresh(&path_a, "fn b() {}"), "cross-content miss");
    }

    // ---- cached_branches --------------------------------------------------

    #[test]
    fn cached_branches_returns_none_when_stale() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverageCache::load(dir.path());
        let path = PathBuf::from("src/lib.rs");
        cache.update(&path, "fn old() {}", sample_branches());

        assert!(
            cache.cached_branches(&path, "fn new() {}").is_none(),
            "stale content → no cached branches"
        );
    }

    #[test]
    fn cached_branches_returns_some_when_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CoverageCache::load(dir.path());
        let path = PathBuf::from("src/lib.rs");
        let content = "fn x() { if true {} }";
        cache.update(&path, content, sample_branches());

        let branches = cache.cached_branches(&path, content);
        assert!(branches.is_some());
        assert_eq!(branches.unwrap().len(), 2);
    }
}
