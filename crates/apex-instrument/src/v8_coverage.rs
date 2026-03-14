use apex_core::{hash::fnv1a_hash, types::BranchId};
use serde::Deserialize;
use std::{collections::HashMap, path::{Path, PathBuf}};

/// Parsed V8 coverage: (all_branches, executed_branches, file_paths).
pub type V8ParseResult = (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>);

// V8 coverage JSON schema
#[derive(Debug, Deserialize)]
pub struct V8CoverageResult {
    pub result: Vec<V8ScriptCoverage>,
}

#[derive(Debug, Deserialize)]
pub struct V8ScriptCoverage {
    pub url: String,
    pub functions: Vec<V8FunctionCoverage>,
}

#[derive(Debug, Deserialize)]
pub struct V8FunctionCoverage {
    pub ranges: Vec<V8CoverageRange>,
}

#[derive(Debug, Deserialize)]
pub struct V8CoverageRange {
    #[serde(rename = "startOffset")]
    pub start_offset: usize,
    #[serde(rename = "endOffset")]
    pub end_offset: usize,
    pub count: u64,
}

// OffsetIndex — byte offset → (line, col)
pub struct OffsetIndex {
    line_starts: Vec<usize>,
}

impl OffsetIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        OffsetIndex { line_starts }
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (u32, u16) {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let col = offset.saturating_sub(self.line_starts[line_idx]);
        let line = (line_idx + 1) as u32;
        let col = col.min(u16::MAX as usize) as u16;
        (line, col)
    }
}

// Parsing
pub fn url_to_repo_relative(url: &str, repo_root: &Path) -> Option<PathBuf> {
    let path_str = url.strip_prefix("file://")?;
    let abs_path = Path::new(path_str);
    let rel = abs_path.strip_prefix(repo_root).unwrap_or(abs_path);
    Some(rel.to_path_buf())
}

pub fn parse_v8_coverage(
    json_str: &str,
    repo_root: &Path,
    source_loader: &dyn Fn(&Path) -> Option<String>,
) -> Result<V8ParseResult, String> {
    let data: V8CoverageResult =
        serde_json::from_str(json_str).map_err(|e| format!("parse V8 JSON: {e}"))?;

    let mut all_branches = Vec::new();
    let mut executed_branches = Vec::new();
    let mut file_paths = HashMap::new();

    for script in &data.result {
        let Some(rel_path) = url_to_repo_relative(&script.url, repo_root) else {
            continue;
        };
        let rel_str = rel_path.to_string_lossy();
        let file_id = fnv1a_hash(&rel_str);
        let abs_path = repo_root.join(&*rel_str);
        file_paths.insert(file_id, rel_path);
        let Some(source) = source_loader(&abs_path) else {
            continue;
        };
        let index = OffsetIndex::new(&source);

        for func in &script.functions {
            let branch_points = extract_branch_points(&func.ranges);

            for group in &branch_points {
                if group.len() < 2 {
                    continue;
                }
                for (direction, range_idx) in group.iter().enumerate() {
                    let range = &func.ranges[*range_idx];
                    let (line, col) = index.offset_to_line_col(range.start_offset);
                    let dir = (direction).min(u8::MAX as usize) as u8;
                    let bid = BranchId::new(file_id, line, col, dir);
                    all_branches.push(bid.clone());
                    if range.count > 0 {
                        executed_branches.push(bid);
                    }
                }
            }
        }
    }

    Ok((all_branches, executed_branches, file_paths))
}

fn extract_branch_points(ranges: &[V8CoverageRange]) -> Vec<Vec<usize>> {
    if ranges.len() < 2 {
        return Vec::new();
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current_group: Vec<usize> = Vec::new();
    let mut parent_end: usize = ranges[0].end_offset;

    for (i, range) in ranges.iter().enumerate().skip(1) {
        if range.start_offset >= parent_end {
            if current_group.len() >= 2 {
                groups.push(current_group.clone());
            }
            current_group.clear();
            parent_end = range.end_offset;
        }
        current_group.push(i);
    }

    if current_group.len() >= 2 {
        groups.push(current_group);
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_index_simple() {
        let source = "line1\nline2\nline3";
        let idx = OffsetIndex::new(source);
        assert_eq!(idx.offset_to_line_col(0), (1, 0));
        assert_eq!(idx.offset_to_line_col(3), (1, 3));
        assert_eq!(idx.offset_to_line_col(6), (2, 0));
        assert_eq!(idx.offset_to_line_col(12), (3, 0));
    }

    #[test]
    fn offset_index_empty_source() {
        let idx = OffsetIndex::new("");
        assert_eq!(idx.offset_to_line_col(0), (1, 0));
    }

    #[test]
    fn offset_index_col_saturates() {
        let long_line = "x".repeat(70000);
        let idx = OffsetIndex::new(&long_line);
        let (line, col) = idx.offset_to_line_col(66000);
        assert_eq!(line, 1);
        assert_eq!(col, u16::MAX);
    }

    #[test]
    fn url_to_repo_relative_strips_prefix() {
        let repo = Path::new("/home/user/project");
        let url = "file:///home/user/project/src/index.js";
        let rel = url_to_repo_relative(url, repo).unwrap();
        assert_eq!(rel, PathBuf::from("src/index.js"));
    }

    #[test]
    fn url_to_repo_relative_non_file_url() {
        let repo = Path::new("/project");
        assert!(url_to_repo_relative("https://example.com/file.js", repo).is_none());
    }

    #[test]
    fn url_to_repo_relative_outside_repo() {
        let repo = Path::new("/project");
        let url = "file:///other/path/file.js";
        let rel = url_to_repo_relative(url, repo).unwrap();
        assert_eq!(rel, PathBuf::from("/other/path/file.js"));
    }

    #[test]
    fn parse_v8_simple_branch() {
        let json = r#"{
            "result": [{
                "url": "file:///repo/src/app.js",
                "functions": [{
                    "functionName": "main",
                    "ranges": [
                        {"startOffset": 0, "endOffset": 100, "count": 1},
                        {"startOffset": 10, "endOffset": 50, "count": 1},
                        {"startOffset": 50, "endOffset": 90, "count": 0}
                    ]
                }]
            }]
        }"#;
        let source = "if (x) {\n  doA();\n} else {\n  doB();\n}\n".repeat(3);
        let repo = Path::new("/repo");
        let (all, exec, files) =
            parse_v8_coverage(json, repo, &|_| Some(source.clone())).unwrap();

        assert_eq!(files.len(), 1);
        assert!(files.values().any(|p| p == Path::new("src/app.js")));
        assert_eq!(all.len(), 2);
        assert_eq!(exec.len(), 1);
    }

    #[test]
    fn parse_v8_no_branches_single_range() {
        let json = r#"{
            "result": [{
                "url": "file:///repo/src/simple.js",
                "functions": [{
                    "functionName": "noop",
                    "ranges": [
                        {"startOffset": 0, "endOffset": 50, "count": 1}
                    ]
                }]
            }]
        }"#;
        let repo = Path::new("/repo");
        let (all, _, _) = parse_v8_coverage(json, repo, &|_| Some("noop".into())).unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn parse_v8_invalid_json() {
        let result = parse_v8_coverage("not json", Path::new("/"), &|_| None);
        assert!(result.is_err());
    }

    #[test]
    fn parse_v8_skips_files_without_source() {
        let json = r#"{"result": [{"url": "file:///repo/x.js", "functions": []}]}"#;
        let repo = Path::new("/repo");
        let (all, _, files) = parse_v8_coverage(json, repo, &|_| None).unwrap();
        assert!(all.is_empty());
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn extract_branch_points_two_siblings() {
        let ranges = vec![
            V8CoverageRange { start_offset: 0, end_offset: 100, count: 1 },
            V8CoverageRange { start_offset: 10, end_offset: 50, count: 1 },
            V8CoverageRange { start_offset: 50, end_offset: 90, count: 0 },
        ];
        let groups = extract_branch_points(&ranges);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![1, 2]);
    }

    #[test]
    fn extract_branch_points_no_inner_ranges() {
        let ranges = vec![
            V8CoverageRange { start_offset: 0, end_offset: 100, count: 1 },
        ];
        let groups = extract_branch_points(&ranges);
        assert!(groups.is_empty());
    }

    #[test]
    fn direction_saturates_at_u8_max() {
        let mut ranges_json = String::from(
            r#"{"startOffset": 0, "endOffset": 10000, "count": 1}"#
        );
        for i in 0..260u32 {
            let start = (i + 1) * 10;
            let end = start + 9;
            ranges_json.push_str(&format!(
                r#", {{"startOffset": {start}, "endOffset": {end}, "count": 1}}"#
            ));
        }
        let json = format!(
            r#"{{"result": [{{"url": "file:///repo/big.js", "functions": [{{"functionName": "f", "ranges": [{ranges_json}]}}]}}]}}"#
        );
        let repo = Path::new("/repo");
        let source = " ".repeat(11000);
        let (all, _, _) = parse_v8_coverage(&json, repo, &|_| Some(source.clone())).unwrap();
        for bid in &all {
            assert!(bid.direction <= u8::MAX);
        }
    }
}
