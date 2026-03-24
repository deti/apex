//! Unified LLVM source-based coverage backend.
//!
//! All compiled languages that use LLVM (C, C++, Rust, Swift) produce the
//! same `llvm-cov export --format=json` output. This module provides a single
//! JSON parser (`parse_llvm_cov_export`) so language-specific instrumentors
//! don't need to duplicate parsing logic.
//!
//! ## Segment layout
//!
//! Each segment is `[line, col, count, has_count, is_region_entry, is_gap_region]`.
//! We filter to segments where `has_count=true AND is_region_entry=true AND is_gap=false`.
//! This matches the Rust parser semantics (the most precise of the three original parsers).
//!
//! Boolean fields may be encoded as `true`/`false` or `0`/`1` depending on the LLVM
//! version, so we use [`json_truthy`] for compatibility.

use apex_core::{hash::fnv1a_hash as fnv1a, types::BranchId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parsed output from `llvm-cov export --format=json`.
pub struct ParsedCoverage {
    /// All coverable branches (one per region-entry segment).
    pub branch_ids: Vec<BranchId>,
    /// Branches with execution count > 0.
    pub executed_branch_ids: Vec<BranchId>,
    /// Map from file_id to relative path.
    pub file_paths: HashMap<u64, PathBuf>,
}

/// Controls which files are included during parsing.
pub struct FileFilter {
    /// Skip files whose absolute path is not under `target_root`.
    pub require_under_root: bool,
    /// Skip test files (tests/, *_test.rs, *_tests.rs, etc.).
    pub skip_test_files: bool,
}

impl Default for FileFilter {
    fn default() -> Self {
        FileFilter {
            require_under_root: true,
            skip_test_files: false,
        }
    }
}

/// Interpret a JSON value as a boolean: supports both `true`/`false` literals
/// and integer `0`/`1` (different LLVM versions use different encodings).
fn json_truthy(v: &serde_json::Value) -> bool {
    v.as_bool().unwrap_or_else(|| v.as_u64().unwrap_or(0) != 0)
}

/// Parse `llvm-cov export --format=json` output.
///
/// The JSON schema is the same for C, C++, Rust, and Swift:
/// ```json
/// { "data": [{ "files": [{ "filename": "...", "segments": [...] }] }] }
/// ```
///
/// Each segment is `[line, col, count, has_count, is_region_entry, is_gap_region]`.
/// We filter to segments where has_count=true AND is_region_entry=true AND is_gap=false.
pub fn parse_llvm_cov_export(
    bytes: &[u8],
    target_root: &Path,
    filter: &FileFilter,
) -> std::result::Result<ParsedCoverage, Box<dyn std::error::Error + Send + Sync>> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;

    let mut branch_ids: Vec<BranchId> = Vec::new();
    let mut executed_ids: Vec<BranchId> = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    // Canonicalize target_root so strip_prefix works even when the coverage
    // JSON contains symlink-resolved paths (e.g. /private/tmp vs /tmp on macOS).
    let canon_root = target_root.canonicalize().unwrap_or_else(|_| target_root.to_path_buf());

    let data = v["data"].as_array().ok_or("missing data array")?;
    for entry in data {
        let files = entry["files"].as_array().ok_or("missing files array")?;
        for file in files {
            let filename = file["filename"].as_str().ok_or("missing filename")?;

            // Derive relative path from absolute filename.
            // Try both the raw path and its canonical form so symlinks
            // (e.g. /tmp -> /private/tmp on macOS) don't cause mismatches.
            let abs = Path::new(filename);
            let canon_abs = abs.canonicalize().unwrap_or_else(|_| abs.to_path_buf());
            let rel = if let Ok(r) = abs.strip_prefix(&canon_root) {
                r.to_path_buf()
            } else if let Ok(r) = canon_abs.strip_prefix(&canon_root) {
                r.to_path_buf()
            } else if let Ok(r) = abs.strip_prefix(target_root) {
                r.to_path_buf()
            } else if let Ok(r) = canon_abs.strip_prefix(target_root) {
                r.to_path_buf()
            } else {
                if filter.require_under_root {
                    continue;
                }
                PathBuf::from(filename)
            };

            // Skip test files
            if filter.skip_test_files {
                let rel_str = rel.to_string_lossy();
                if rel_str.starts_with("tests/")
                    || rel_str.contains("/tests/")
                    || rel_str.ends_with("_test.rs")
                    || rel_str.ends_with("_tests.rs")
                {
                    continue;
                }
            }

            let fid = fnv1a(&rel.to_string_lossy());
            file_paths.entry(fid).or_insert_with(|| rel.clone());

            let segments = file["segments"].as_array().ok_or("missing segments")?;
            for seg in segments {
                let arr = seg.as_array().ok_or("segment not array")?;
                if arr.len() < 6 {
                    continue;
                }
                let line = arr[0].as_u64().unwrap_or(0) as u32;
                let col = arr[1].as_u64().unwrap_or(0).min(u16::MAX as u64) as u16;
                let count = arr[2].as_u64().unwrap_or(0);
                let has_count = json_truthy(&arr[3]);
                let is_entry = json_truthy(&arr[4]);
                let is_gap = json_truthy(&arr[5]);

                if !has_count || !is_entry || is_gap {
                    continue;
                }

                let bid = BranchId::new(fid, line, col, 0);
                branch_ids.push(bid.clone());
                if count > 0 {
                    executed_ids.push(bid);
                }
            }
        }
    }

    // Deduplicate.
    branch_ids.sort_by_key(|b| (b.file_id, b.line, b.col));
    branch_ids.dedup();
    executed_ids.sort_by_key(|b| (b.file_id, b.line, b.col));
    executed_ids.dedup();

    Ok(ParsedCoverage {
        branch_ids,
        executed_branch_ids: executed_ids,
        file_paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json(root: &str) -> String {
        format!(
            r#"{{
  "data": [
    {{
      "files": [
        {{
          "filename": "{root}/src/main.rs",
          "segments": [
            [5, 1, 10, true, true, false],
            [8, 5, 0, true, true, false],
            [12, 1, 3, true, true, false],
            [15, 1, 0, false, false, false],
            [20, 1, 1, true, false, false],
            [25, 1, 0, true, true, true]
          ]
        }},
        {{
          "filename": "{root}/src/lib.rs",
          "segments": [
            [3, 1, 5, true, true, false],
            [7, 1, 0, true, true, false]
          ]
        }},
        {{
          "filename": "/rustc/abc123/library/core/src/ops.rs",
          "segments": [
            [1, 1, 100, true, true, false]
          ]
        }}
      ]
    }}
  ]
}}"#
        )
    }

    #[test]
    fn parse_basic_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = sample_json(root.to_str().unwrap());
        let filter = FileFilter {
            require_under_root: true,
            skip_test_files: true,
        };

        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.branch_ids.len(), 5);
        assert_eq!(result.executed_branch_ids.len(), 3);
        assert_eq!(result.file_paths.len(), 2);
    }

    #[test]
    fn skips_external_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = sample_json(root.to_str().unwrap());
        let filter = FileFilter {
            require_under_root: true,
            skip_test_files: false,
        };

        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        for path in result.file_paths.values() {
            let s = path.to_string_lossy();
            assert!(!s.contains("ops.rs"), "should skip external file: {s}");
        }
    }

    #[test]
    fn deduplication() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "{root}/src/dup.rs", "segments": [[1, 1, 5, true, true, false], [1, 1, 5, true, true, false]]}}]}}]}}"#,
            root = root.to_str().unwrap()
        );
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.branch_ids.len(), 1);
        assert_eq!(result.executed_branch_ids.len(), 1);
    }

    #[test]
    fn empty_data() {
        let json = r#"{"data": [{"files": []}]}"#;
        let filter = FileFilter::default();
        let result =
            parse_llvm_cov_export(json.as_bytes(), Path::new("/nonexistent"), &filter).unwrap();
        assert_eq!(result.branch_ids.len(), 0);
        assert_eq!(result.executed_branch_ids.len(), 0);
        assert_eq!(result.file_paths.len(), 0);
    }

    #[test]
    fn short_segment_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "{root}/src/s.rs", "segments": [[1, 2, 3, true, true]]}}]}}]}}"#,
            root = root.to_str().unwrap()
        );
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.branch_ids.len(), 0);
    }

    #[test]
    fn invalid_json_returns_error() {
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(b"not json", Path::new("/tmp"), &filter);
        assert!(result.is_err());
    }

    #[test]
    fn integer_booleans() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "{root}/src/i.c", "segments": [[10, 5, 3, 1, 1, 0]]}}]}}]}}"#,
            root = root.to_str().unwrap()
        );
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.branch_ids.len(), 1);
        assert_eq!(result.executed_branch_ids.len(), 1);
    }

    #[test]
    fn gap_region_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "{root}/src/g.rs", "segments": [[1, 1, 5, true, true, true], [2, 1, 5, true, true, false]]}}]}}]}}"#,
            root = root.to_str().unwrap()
        );
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.branch_ids.len(), 1);
        assert_eq!(result.executed_branch_ids.len(), 1);
    }

    #[test]
    fn skip_test_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [
      {{"filename": "{root}/src/lib.rs", "segments": [[1, 1, 5, true, true, false]]}},
      {{"filename": "{root}/tests/integration.rs", "segments": [[1, 1, 3, true, true, false]]}},
      {{"filename": "{root}/src/foo_test.rs", "segments": [[1, 1, 2, true, true, false]]}},
      {{"filename": "{root}/src/bar_tests.rs", "segments": [[1, 1, 1, true, true, false]]}}
    ]
  }}]
}}"#,
            root = root.to_str().unwrap()
        );
        let filter = FileFilter {
            require_under_root: true,
            skip_test_files: true,
        };
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.file_paths.len(), 1);
        assert_eq!(result.branch_ids.len(), 1);
    }

    #[test]
    fn direction_always_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "{root}/src/d.swift", "segments": [[1, 1, 0, true, true, false], [2, 1, 5, true, true, false]]}}]}}]}}"#,
            root = root.to_str().unwrap()
        );
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        for b in &result.branch_ids {
            assert_eq!(b.direction, 0, "all branches should have direction=0");
        }
    }

    #[test]
    fn json_truthy_handles_both_types() {
        assert!(json_truthy(&serde_json::Value::Bool(true)));
        assert!(!json_truthy(&serde_json::Value::Bool(false)));
        assert!(json_truthy(&serde_json::json!(1)));
        assert!(!json_truthy(&serde_json::json!(0)));
    }

    #[test]
    fn not_region_entry_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "{root}/src/n.rs", "segments": [[1, 1, 5, true, false, false], [2, 1, 5, false, true, false]]}}]}}]}}"#,
            root = root.to_str().unwrap()
        );
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.branch_ids.len(), 0);
    }

    #[test]
    fn col_clamped_to_u16_max() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "{root}/src/c.rs", "segments": [[1, 70000, 1, true, true, false]]}}]}}]}}"#,
            root = root.to_str().unwrap()
        );
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.branch_ids[0].col, 65535);
    }

    #[test]
    fn multiple_data_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [
  {{"files": [{{"filename": "{root}/src/a.rs", "segments": [[1, 1, 1, true, true, false]]}}]}},
  {{"files": [{{"filename": "{root}/src/b.rs", "segments": [[2, 1, 0, true, true, false]]}}]}}
]}}"#,
            root = root.to_str().unwrap()
        );
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.branch_ids.len(), 2);
        assert_eq!(result.executed_branch_ids.len(), 1);
        assert_eq!(result.file_paths.len(), 2);
    }

    #[test]
    fn missing_data_key_errors() {
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(br#"{"not_data": []}"#, Path::new("/tmp"), &filter);
        assert!(result.is_err());
    }

    #[test]
    fn missing_files_key_errors() {
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(
            br#"{"data": [{"not_files": []}]}"#,
            Path::new("/tmp"),
            &filter,
        );
        assert!(result.is_err());
    }

    #[test]
    fn missing_filename_errors() {
        let filter = FileFilter::default();
        let result = parse_llvm_cov_export(
            br#"{"data": [{"files": [{"no_filename": true, "segments": []}]}]}"#,
            Path::new("/tmp"),
            &filter,
        );
        assert!(result.is_err());
    }

    #[test]
    fn require_under_root_false_includes_external() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "/external/lib.c", "segments": [[1, 1, 5, true, true, false]]}}]}}]}}"#,
        );
        let filter = FileFilter {
            require_under_root: false,
            skip_test_files: false,
        };
        let result = parse_llvm_cov_export(json.as_bytes(), root, &filter).unwrap();
        assert_eq!(result.branch_ids.len(), 1);
        assert_eq!(result.file_paths.len(), 1);
    }
}
