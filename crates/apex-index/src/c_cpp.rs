//! C/C++ coverage index — parses gcov text and llvm-cov JSON formats.
//!
//! Two coverage formats are supported:
//!
//! **gcov format** (produced by `gcc --coverage` / `gcov`):
//! ```text
//!         1:    5:  int x = 0;
//!         0:   10:  if (never_taken) {
//! ```
//! Each line is `<count>:<line_number>:<source_text>`.
//! A count of `-` means the line is non-executable (e.g., blank lines, comments).
//!
//! **llvm-cov JSON format** (produced by `llvm-cov export --format=text`):
//! ```json
//! {"data":[{"files":[{"filename":"foo.c","segments":[[line,col,count,has_count,is_region_entry],...]}]}]}
//! ```
//! Each segment tuple: `[line, col, count, has_count (bool), is_region_entry (bool)]`.
//! Only segments with `has_count == true` contribute to branch coverage.
//!
//! Auto-detection: if the content starts with `{` it is parsed as JSON; otherwise gcov text.

use crate::types::{BranchIndex, TestTrace};
use apex_core::hash::fnv1a_hash;
use apex_core::types::{BranchId, Language};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

// ---------------------------------------------------------------------------
// llvm-cov JSON schema
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LlvmCovJson {
    data: Vec<LlvmCovData>,
}

#[derive(Deserialize)]
struct LlvmCovData {
    files: Vec<LlvmCovFile>,
}

#[derive(Deserialize)]
struct LlvmCovFile {
    filename: String,
    /// Each segment is [line, col, count, has_count, is_region_entry, ...].
    /// Additional fields may appear in newer llvm-cov versions and are ignored.
    segments: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parsed C/C++ coverage data ready for indexing.
#[derive(Debug, Default)]
pub struct CCppCoverageData {
    /// All branch points discovered (covered + uncovered).
    pub all_branches: Vec<BranchId>,
    /// Only branches with a hit count > 0.
    pub covered_branches: Vec<BranchId>,
    /// Maps file_id → file path.
    pub file_paths: HashMap<u64, PathBuf>,
    /// Total executable branch points.
    pub total: usize,
    /// Branch points with count > 0.
    pub covered: usize,
}

/// Parse gcov text coverage output into coverage data.
///
/// Expected line format:
/// ```text
///         <count>:  <line_no>:  <source>
/// ```
/// where `count` is a hit count, `#####` (never executed), or `-` (non-executable).
pub fn parse_gcov(content: &str) -> CCppCoverageData {
    let mut data = CCppCoverageData::default();

    // gcov lines have no filename — use a synthetic file_id of 0.
    let file_id: u64 = fnv1a_hash("<gcov>");
    data.file_paths
        .insert(file_id, PathBuf::from("<gcov-unknown>"));

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Split on ':' — expect at least 3 fields: count, line_no, source
        let mut parts = line.splitn(3, ':');
        let count_str = match parts.next() {
            Some(s) => s.trim(),
            None => continue,
        };
        let line_no_str = match parts.next() {
            Some(s) => s.trim(),
            None => continue,
        };

        // Non-executable lines (comments, blanks, declarations) are marked '-'
        if count_str == "-" {
            continue;
        }

        let line_no: u32 = match line_no_str.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        // Skip line 0 (gcov uses line 0 for metadata)
        if line_no == 0 {
            continue;
        }

        // "#####" means never executed; any digit string is a hit count
        let (hit, count): (bool, u64) = if count_str == "#####" || count_str == "=====" {
            (false, 0)
        } else {
            match count_str.replace(',', "").parse::<u64>() {
                Ok(n) => (n > 0, n),
                Err(_) => continue,
            }
        };
        let _ = count;

        let branch = BranchId::new(file_id, line_no, 0, if hit { 0 } else { 1 });
        data.all_branches.push(branch.clone());
        data.total += 1;
        if hit {
            data.covered_branches.push(branch);
            data.covered += 1;
        }
    }

    data
}

/// Parse llvm-cov JSON coverage output into coverage data.
///
/// The JSON format is produced by:
/// ```sh
/// llvm-cov export --format=text <binary> -instr-profile=<profdata>
/// ```
pub fn parse_llvm_cov_json(content: &str) -> Result<CCppCoverageData, BoxErr> {
    let mut data = CCppCoverageData::default();

    let root: LlvmCovJson = serde_json::from_str(content)?;

    for export_data in root.data {
        for file in export_data.files {
            let file_id = fnv1a_hash(&file.filename);
            data.file_paths
                .entry(file_id)
                .or_insert_with(|| PathBuf::from(&file.filename));

            for segment in &file.segments {
                // Segment layout: [line, col, count, has_count, is_region_entry, ...]
                let arr = match segment.as_array() {
                    Some(a) => a,
                    None => continue,
                };
                if arr.len() < 5 {
                    continue;
                }

                let line = match arr[0].as_u64() {
                    Some(l) => l as u32,
                    None => continue,
                };
                let col = match arr[1].as_u64() {
                    Some(c) => c as u16,
                    None => continue,
                };
                let count = arr[2].as_u64().unwrap_or(0);
                let has_count = arr[3].as_bool().unwrap_or(false)
                    || arr[3].as_u64().map(|n| n != 0).unwrap_or(false);
                let is_region_entry = arr[4].as_bool().unwrap_or(false)
                    || arr[4].as_u64().map(|n| n != 0).unwrap_or(false);

                // Only count region-entry segments with a valid count
                if !has_count || !is_region_entry {
                    continue;
                }

                let hit = count > 0;
                let branch = BranchId::new(file_id, line, col, if hit { 0 } else { 1 });
                data.all_branches.push(branch.clone());
                data.total += 1;
                if hit {
                    data.covered_branches.push(branch);
                    data.covered += 1;
                }
            }
        }
    }

    Ok(data)
}

/// Auto-detect format and parse C/C++ coverage content into a [`BranchIndex`].
///
/// - JSON (starts with `{`): parsed as llvm-cov export JSON
/// - Otherwise: parsed as gcov text format
pub fn parse(content: &str) -> Result<BranchIndex, BoxErr> {
    let cov_data = if content.trim_start().starts_with('{') {
        parse_llvm_cov_json(content)?
    } else {
        parse_gcov(content)
    };

    build_index_from_data(cov_data)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn build_index_from_data(data: CCppCoverageData) -> Result<BranchIndex, BoxErr> {
    let traces: Vec<TestTrace> = Vec::new();
    let profiles = BranchIndex::build_profiles(&traces);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(BranchIndex {
        traces,
        profiles,
        file_paths: data.file_paths,
        total_branches: data.total,
        covered_branches: data.covered,
        created_at: format!("{now}"),
        language: Language::C,
        target_root: PathBuf::new(),
        source_hash: String::new(),
    })
}

// ---------------------------------------------------------------------------
// CCppCoverageIndex — named wrapper for consistency with other index modules
// ---------------------------------------------------------------------------

/// Stateless parser for C/C++ coverage data.
///
/// Wraps the module-level functions with a struct API consistent with other
/// language index modules in this crate.
pub struct CCppCoverageIndex;

impl CCppCoverageIndex {
    pub fn new() -> Self {
        Self
    }

    /// Parse gcov text format.
    pub fn parse_gcov(&self, content: &str) -> CCppCoverageData {
        parse_gcov(content)
    }

    /// Parse llvm-cov JSON format.
    pub fn parse_llvm_cov_json(&self, content: &str) -> Result<CCppCoverageData, BoxErr> {
        parse_llvm_cov_json(content)
    }

    /// Auto-detect format and return a [`BranchIndex`].
    pub fn parse(&self, content: &str) -> Result<BranchIndex, BoxErr> {
        parse(content)
    }
}

impl Default for CCppCoverageIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- gcov fixtures -----

    const GCOV_BASIC: &str = "\
        -:    0:Source:src/main.c
        -:    1:#include <stdio.h>
        -:    2:
        1:    3:int main() {
        1:    4:    int x = 0;
    #####:    5:    if (0) {
    #####:    6:        x = 1;
        -:    7:    }
        2:    8:    return x;
        -:    9:}
";

    const GCOV_EMPTY: &str = "\
        -:    0:Source:empty.c
        -:    1:// just comments
";

    // ----- llvm-cov JSON fixtures -----

    const LLVM_COV_BASIC: &str = r#"{
  "data": [
    {
      "files": [
        {
          "filename": "/project/src/main.c",
          "segments": [
            [3, 12, 1, true, true],
            [4, 5, 1, true, true],
            [5, 8, 0, true, true],
            [6, 9, 0, true, false],
            [8, 5, 2, true, true]
          ]
        }
      ]
    }
  ]
}"#;

    const LLVM_COV_MULTI_FILE: &str = r#"{
  "data": [
    {
      "files": [
        {
          "filename": "/project/src/a.c",
          "segments": [
            [1, 1, 5, true, true],
            [2, 1, 0, true, true]
          ]
        },
        {
          "filename": "/project/src/b.cpp",
          "segments": [
            [10, 1, 3, true, true],
            [11, 1, 3, true, false],
            [15, 1, 0, true, true]
          ]
        }
      ]
    }
  ]
}"#;

    const LLVM_COV_ZERO_COUNT_SEGMENTS: &str = r#"{
  "data": [
    {
      "files": [
        {
          "filename": "/src/util.c",
          "segments": [
            [1, 1, 0, false, true],
            [2, 1, 10, true, true],
            [3, 1, 0, true, true]
          ]
        }
      ]
    }
  ]
}"#;

    // ----- gcov tests -----

    #[test]
    fn gcov_parses_hit_and_miss_lines() {
        let data = parse_gcov(GCOV_BASIC);
        // Lines 3, 4, 5, 6, 8 are executable (5 total)
        assert_eq!(data.total, 5, "expected 5 executable lines");
        // Lines 3, 4, 8 are hit (count > 0)
        assert_eq!(data.covered, 3, "expected 3 covered lines");
    }

    #[test]
    fn gcov_skips_non_executable_lines() {
        let data = parse_gcov(GCOV_BASIC);
        // Lines with '-' are skipped (includes, blanks, braces)
        // Confirmed: only 5 branches in data.all_branches
        assert_eq!(data.all_branches.len(), 5);
    }

    #[test]
    fn gcov_empty_returns_zero() {
        let data = parse_gcov(GCOV_EMPTY);
        assert_eq!(data.total, 0);
        assert_eq!(data.covered, 0);
    }

    #[test]
    fn gcov_invalid_lines_are_skipped() {
        let data = parse_gcov("garbage\nmore garbage\n");
        assert_eq!(data.total, 0);
    }

    #[test]
    fn gcov_file_paths_populated() {
        let data = parse_gcov(GCOV_BASIC);
        // Synthetic gcov file_id always present
        assert_eq!(data.file_paths.len(), 1);
    }

    #[test]
    fn gcov_direction_hit_vs_miss() {
        let data = parse_gcov(GCOV_BASIC);
        let hits: Vec<_> = data
            .all_branches
            .iter()
            .filter(|b| b.direction == 0)
            .collect();
        let misses: Vec<_> = data
            .all_branches
            .iter()
            .filter(|b| b.direction == 1)
            .collect();
        assert_eq!(hits.len(), 3);
        assert_eq!(misses.len(), 2);
    }

    #[test]
    fn gcov_count_with_commas_parsed() {
        // gcov sometimes formats large counts with commas: 1,234
        let input = "    1,234:    5:  return 0;\n";
        let data = parse_gcov(input);
        assert_eq!(data.total, 1);
        assert_eq!(data.covered, 1);
    }

    #[test]
    fn gcov_equals_marks_unexecuted() {
        // "=====" marks unexecuted branch alternative in some gcov versions
        let input = "    =====:   10:  x = 1;\n";
        let data = parse_gcov(input);
        assert_eq!(data.total, 1);
        assert_eq!(data.covered, 0);
    }

    // ----- llvm-cov JSON tests -----

    #[test]
    fn llvm_cov_parses_region_entries() {
        let data = parse_llvm_cov_json(LLVM_COV_BASIC).unwrap();
        // Segments with is_region_entry=true and has_count=true: lines 3,4,5,8 (4 segments)
        assert_eq!(data.total, 4, "expected 4 region-entry segments");
        // Lines 3, 4, 8 have count > 0
        assert_eq!(data.covered, 3);
    }

    #[test]
    fn llvm_cov_skips_non_region_entries() {
        let data = parse_llvm_cov_json(LLVM_COV_BASIC).unwrap();
        // Segment at line 6 has is_region_entry=false — must be excluded
        let line6: Vec<_> = data.all_branches.iter().filter(|b| b.line == 6).collect();
        assert!(
            line6.is_empty(),
            "non-region-entry segment should be skipped"
        );
    }

    #[test]
    fn llvm_cov_multi_file_populates_file_paths() {
        let data = parse_llvm_cov_json(LLVM_COV_MULTI_FILE).unwrap();
        assert_eq!(data.file_paths.len(), 2);
    }

    #[test]
    fn llvm_cov_multi_file_totals() {
        let data = parse_llvm_cov_json(LLVM_COV_MULTI_FILE).unwrap();
        // a.c: 2 region entries (lines 1, 2); b.cpp: 2 region entries (lines 10, 15)
        // b.cpp line 11 has is_region_entry=false → skipped
        assert_eq!(data.total, 4);
        // a.c line 1 (5), b.cpp line 10 (3) are hit; a.c line 2 (0), b.cpp line 15 (0) are not
        assert_eq!(data.covered, 2);
    }

    #[test]
    fn llvm_cov_skips_no_has_count() {
        let data = parse_llvm_cov_json(LLVM_COV_ZERO_COUNT_SEGMENTS).unwrap();
        // Segment at line 1 has has_count=false → skipped
        // Segment at line 2 hit, line 3 miss
        assert_eq!(data.total, 2);
        assert_eq!(data.covered, 1);
    }

    #[test]
    fn llvm_cov_invalid_json_returns_error() {
        let result = parse_llvm_cov_json("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn llvm_cov_empty_data_array() {
        let json = r#"{"data":[]}"#;
        let data = parse_llvm_cov_json(json).unwrap();
        assert_eq!(data.total, 0);
    }

    #[test]
    fn llvm_cov_segment_too_short_skipped() {
        let json = r#"{"data":[{"files":[{"filename":"a.c","segments":[[1,2,3]]}]}]}"#;
        let data = parse_llvm_cov_json(json).unwrap();
        assert_eq!(data.total, 0);
    }

    // ----- auto-detect tests -----

    #[test]
    fn parse_auto_detects_json() {
        let index = parse(LLVM_COV_BASIC).unwrap();
        assert_eq!(index.total_branches, 4);
        assert_eq!(index.covered_branches, 3);
        assert!(matches!(index.language, Language::C));
    }

    #[test]
    fn parse_auto_detects_gcov() {
        let index = parse(GCOV_BASIC).unwrap();
        assert_eq!(index.total_branches, 5);
        assert_eq!(index.covered_branches, 3);
    }

    #[test]
    fn parse_gcov_leading_whitespace_json_check() {
        // JSON with leading whitespace is still detected as JSON
        let json = format!("  \n{}", LLVM_COV_BASIC);
        let index = parse(&json).unwrap();
        assert_eq!(index.total_branches, 4);
    }

    #[test]
    fn parse_empty_gcov_builds_empty_index() {
        let index = parse(GCOV_EMPTY).unwrap();
        assert_eq!(index.total_branches, 0);
        assert_eq!(index.covered_branches, 0);
    }

    // ----- CCppCoverageIndex struct tests -----

    #[test]
    fn struct_parse_gcov_delegates() {
        let idx = CCppCoverageIndex::new();
        let data = idx.parse_gcov(GCOV_BASIC);
        assert_eq!(data.total, 5);
    }

    #[test]
    fn struct_parse_llvm_cov_json_delegates() {
        let idx = CCppCoverageIndex::default();
        let data = idx.parse_llvm_cov_json(LLVM_COV_BASIC).unwrap();
        assert_eq!(data.total, 4);
    }

    #[test]
    fn struct_parse_auto_detect() {
        let idx = CCppCoverageIndex::new();
        let index = idx.parse(LLVM_COV_BASIC).unwrap();
        assert_eq!(index.total_branches, 4);
    }

    #[test]
    fn coverage_data_default_is_zero() {
        let data = CCppCoverageData::default();
        assert_eq!(data.total, 0);
        assert_eq!(data.covered, 0);
        assert!(data.all_branches.is_empty());
        assert!(data.file_paths.is_empty());
    }

    #[test]
    fn build_index_language_is_c() {
        let index = parse(GCOV_BASIC).unwrap();
        assert!(matches!(index.language, Language::C));
    }

    #[test]
    fn build_index_traces_empty_for_static_parse() {
        // Static parse (no test runner) yields no traces — traces come from per-test runs
        let index = parse(GCOV_BASIC).unwrap();
        assert!(index.traces.is_empty());
    }

    #[test]
    fn file_id_consistent_across_calls() {
        let data1 = parse_gcov(GCOV_BASIC);
        let data2 = parse_gcov(GCOV_BASIC);
        let ids1: Vec<u64> = data1.file_paths.keys().cloned().collect();
        let ids2: Vec<u64> = data2.file_paths.keys().cloned().collect();
        assert_eq!(ids1, ids2);
    }
}
