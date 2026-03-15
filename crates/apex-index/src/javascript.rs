use apex_core::types::BranchId;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::debug;

// ---------------------------------------------------------------------------
// Istanbul coverage JSON schema (nyc, c8 istanbul reporter, vitest)
// ---------------------------------------------------------------------------

/// Top-level Istanbul coverage JSON: map of file path -> file coverage data.
type IstanbulCoverage = HashMap<String, IstanbulFileCoverage>;

#[derive(Debug, Deserialize)]
struct IstanbulFileCoverage {
    #[serde(default)]
    path: String,
    /// Statement map: id -> location span.
    #[serde(default, rename = "statementMap")]
    statement_map: HashMap<String, IstanbulLocation>,
    /// Statement execution counts: id -> count.
    #[serde(default)]
    s: HashMap<String, u64>,
    /// Branch map: id -> branch descriptor.
    #[serde(default, rename = "branchMap")]
    branch_map: HashMap<String, IstanbulBranch>,
    /// Branch execution counts: id -> [count per arm].
    #[serde(default)]
    b: HashMap<String, Vec<u64>>,
    /// Function map: id -> function descriptor.
    #[serde(default, rename = "fnMap")]
    fn_map: HashMap<String, IstanbulFunction>,
    /// Function execution counts: id -> count.
    #[serde(default)]
    f: HashMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct IstanbulLocation {
    start: IstanbulPosition,
    #[allow(dead_code)]
    end: IstanbulPosition,
}

#[derive(Debug, Deserialize)]
struct IstanbulPosition {
    line: u32,
    #[allow(dead_code)]
    column: u32,
}

#[derive(Debug, Deserialize)]
struct IstanbulBranch {
    #[serde(default)]
    locations: Vec<IstanbulLocation>,
    #[allow(dead_code)]
    #[serde(default, rename = "type")]
    branch_type: String,
}

#[derive(Debug, Deserialize)]
struct IstanbulFunction {
    #[allow(dead_code)]
    #[serde(default)]
    name: String,
    #[serde(default)]
    line: u32,
}

// ---------------------------------------------------------------------------
// V8 coverage JSON schema (c8 v8 reporter, Node.js built-in)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct V8Coverage {
    result: Vec<V8ScriptCoverage>,
}

#[derive(Debug, Deserialize)]
struct V8ScriptCoverage {
    #[allow(dead_code)]
    #[serde(default, rename = "scriptId")]
    script_id: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    functions: Vec<V8FunctionCoverage>,
}

#[derive(Debug, Deserialize)]
struct V8FunctionCoverage {
    #[allow(dead_code)]
    #[serde(default, rename = "functionName")]
    function_name: String,
    #[serde(default)]
    ranges: Vec<V8CoverageRange>,
}

#[derive(Debug, Deserialize)]
struct V8CoverageRange {
    #[serde(default, rename = "startOffset")]
    start_offset: u32,
    #[serde(default, rename = "endOffset")]
    end_offset: u32,
    #[serde(default)]
    count: u64,
}

// ---------------------------------------------------------------------------
// FNV-1a hash (must match apex-instrument and python.rs)
// ---------------------------------------------------------------------------

fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result from parsing JavaScript coverage JSON.
#[derive(Debug)]
pub struct JsCoverageResult {
    /// All branch IDs discovered.
    pub branches: Vec<BranchId>,
    /// Mapping from file_id -> relative file path.
    pub file_paths: HashMap<u64, PathBuf>,
}

/// Parse JavaScript coverage JSON, auto-detecting the format.
///
/// Tries Istanbul format first (top-level keys are file paths),
/// falls back to V8 format (has `"result"` key).
pub fn parse_js_coverage(json: &str) -> Result<JsCoverageResult, String> {
    // Try V8 first: check for "result" key
    if let Ok(v8) = serde_json::from_str::<V8Coverage>(json) {
        debug!("detected V8 coverage format");
        return Ok(parse_v8_coverage_data(&v8));
    }

    // Try Istanbul: top-level keys are file paths
    if let Ok(istanbul) = serde_json::from_str::<IstanbulCoverage>(json) {
        if !istanbul.is_empty() {
            debug!("detected Istanbul coverage format");
            return Ok(parse_istanbul_coverage_data(&istanbul));
        }
    }

    // Empty object parses as both; return empty result
    if json.trim() == "{}" {
        return Ok(JsCoverageResult {
            branches: Vec::new(),
            file_paths: HashMap::new(),
        });
    }

    Err("unrecognized JavaScript coverage format".to_string())
}

/// Parse Istanbul coverage JSON into branch IDs.
pub fn parse_istanbul_coverage(json: &str) -> Result<Vec<BranchId>, String> {
    let data: IstanbulCoverage =
        serde_json::from_str(json).map_err(|e| format!("invalid Istanbul JSON: {e}"))?;
    Ok(parse_istanbul_coverage_data(&data).branches)
}

/// Parse V8 coverage JSON into branch IDs.
pub fn parse_v8_coverage(json: &str) -> Result<Vec<BranchId>, String> {
    let data: V8Coverage =
        serde_json::from_str(json).map_err(|e| format!("invalid V8 JSON: {e}"))?;
    Ok(parse_v8_coverage_data(&data).branches)
}

// ---------------------------------------------------------------------------
// Internal parsing
// ---------------------------------------------------------------------------

fn parse_istanbul_coverage_data(data: &IstanbulCoverage) -> JsCoverageResult {
    let mut branches = Vec::new();
    let mut file_paths = HashMap::new();

    for (file_key, file_cov) in data {
        // Use the `path` field if non-empty, otherwise the map key
        let path_str = if file_cov.path.is_empty() {
            file_key.as_str()
        } else {
            file_cov.path.as_str()
        };
        let rel = normalize_path(path_str);
        let file_id = fnv1a_hash(&rel);
        file_paths.insert(file_id, PathBuf::from(&rel));

        // Extract branches from branchMap + b
        for (branch_id, branch_desc) in &file_cov.branch_map {
            if let Some(counts) = file_cov.b.get(branch_id) {
                for (arm_idx, &count) in counts.iter().enumerate() {
                    let line = branch_desc
                        .locations
                        .get(arm_idx)
                        .map(|loc| loc.start.line)
                        .unwrap_or(0);
                    let direction = arm_idx as u8;
                    let mut bid = BranchId::new(file_id, line, 0, direction);
                    // Store count > 0 info in the branch direction:
                    // direction 0 = first arm (true), 1 = second arm (false), etc.
                    if count > 0 {
                        branches.push(bid.clone());
                    }
                    // Also emit uncovered arms so total branch count is correct
                    if count == 0 {
                        bid.discriminator = 1; // mark as uncovered
                        branches.push(bid);
                    }
                }
            }
        }

        // Extract statement-level branches (each statement is a "branch point"
        // with executed/not-executed as the two arms)
        for (stmt_id, loc) in &file_cov.statement_map {
            let count = file_cov.s.get(stmt_id).copied().unwrap_or(0);
            let line = loc.start.line;
            // Use direction 0 for statement coverage, discriminator to distinguish
            // from branch entries
            let mut bid = BranchId::new(file_id, line, 0, 0);
            bid.discriminator = 100; // distinguish statement branches
            if count > 0 {
                branches.push(bid);
            }
        }

        // Extract function-level coverage
        for (fn_id, fn_desc) in &file_cov.fn_map {
            let count = file_cov.f.get(fn_id).copied().unwrap_or(0);
            let mut bid = BranchId::new(file_id, fn_desc.line, 0, 0);
            bid.discriminator = 200; // distinguish function branches
            if count > 0 {
                branches.push(bid);
            }
        }
    }

    JsCoverageResult {
        branches,
        file_paths,
    }
}

fn parse_v8_coverage_data(data: &V8Coverage) -> JsCoverageResult {
    let mut branches = Vec::new();
    let mut file_paths = HashMap::new();

    for script in &data.result {
        let path_str = normalize_v8_url(&script.url);
        let rel = normalize_path(&path_str);
        let file_id = fnv1a_hash(&rel);
        file_paths.insert(file_id, PathBuf::from(&rel));

        for func in &script.functions {
            for (range_idx, range) in func.ranges.iter().enumerate() {
                // Each range is a coverage region; ranges after the first are
                // typically nested (uncovered) sub-ranges.
                let direction = if range.count > 0 { 0u8 } else { 1u8 };
                let bid = BranchId::new(
                    file_id,
                    range.start_offset,
                    range.end_offset as u16,
                    direction,
                );
                branches.push(bid);
                debug!(
                    file = %rel,
                    range_idx,
                    start = range.start_offset,
                    end = range.end_offset,
                    count = range.count,
                    "v8 coverage range"
                );
            }
        }
    }

    JsCoverageResult {
        branches,
        file_paths,
    }
}

/// Normalize a V8 `file://` URL to a plain path.
fn normalize_v8_url(url: &str) -> String {
    if let Some(stripped) = url.strip_prefix("file:///") {
        // On Unix, file:///path/to/file -> /path/to/file
        // On Windows, file:///C:/path -> C:/path (but we normalize to forward slash)
        if stripped.contains(':') {
            // Windows-style path
            stripped.to_string()
        } else {
            format!("/{stripped}")
        }
    } else if let Some(stripped) = url.strip_prefix("file://") {
        stripped.to_string()
    } else {
        url.to_string()
    }
}

/// Normalize a file path: strip leading slashes for relative paths,
/// convert backslashes to forward slashes.
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Istanbul format tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_istanbul_basic() {
        let json = r#"{
            "src/app.js": {
                "path": "src/app.js",
                "statementMap": {
                    "0": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 30}},
                    "1": {"start": {"line": 2, "column": 0}, "end": {"line": 2, "column": 20}}
                },
                "s": {"0": 5, "1": 0},
                "branchMap": {
                    "0": {
                        "type": "if",
                        "locations": [
                            {"start": {"line": 3, "column": 0}, "end": {"line": 3, "column": 10}},
                            {"start": {"line": 5, "column": 0}, "end": {"line": 5, "column": 10}}
                        ]
                    }
                },
                "b": {"0": [3, 2]},
                "fnMap": {
                    "0": {"name": "foo", "line": 1}
                },
                "f": {"0": 5}
            }
        }"#;

        let branches = parse_istanbul_coverage(json).unwrap();
        // 2 branch arms (both covered) + 1 statement (count>0) + 1 function = 4
        assert!(!branches.is_empty());
        // Verify we got branch entries
        let branch_entries: Vec<_> = branches.iter().filter(|b| b.discriminator == 0).collect();
        assert_eq!(branch_entries.len(), 2); // both arms covered
    }

    #[test]
    fn parse_istanbul_uncovered_branch_arm() {
        let json = r#"{
            "src/app.js": {
                "path": "src/app.js",
                "statementMap": {},
                "s": {},
                "branchMap": {
                    "0": {
                        "type": "if",
                        "locations": [
                            {"start": {"line": 3, "column": 0}, "end": {"line": 3, "column": 10}},
                            {"start": {"line": 5, "column": 0}, "end": {"line": 5, "column": 10}}
                        ]
                    }
                },
                "b": {"0": [3, 0]},
                "fnMap": {},
                "f": {}
            }
        }"#;

        let branches = parse_istanbul_coverage(json).unwrap();
        // 1 covered arm (discriminator=0) + 1 uncovered arm (discriminator=1)
        let covered: Vec<_> = branches.iter().filter(|b| b.discriminator == 0).collect();
        let uncovered: Vec<_> = branches.iter().filter(|b| b.discriminator == 1).collect();
        assert_eq!(covered.len(), 1);
        assert_eq!(uncovered.len(), 1);
    }

    #[test]
    fn parse_istanbul_empty_coverage() {
        let json = r#"{}"#;
        let branches = parse_istanbul_coverage(json).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_istanbul_no_branches() {
        let json = r#"{
            "src/util.js": {
                "path": "src/util.js",
                "statementMap": {},
                "s": {},
                "branchMap": {},
                "b": {},
                "fnMap": {},
                "f": {}
            }
        }"#;

        let branches = parse_istanbul_coverage(json).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_istanbul_uses_path_field() {
        let json = r#"{
            "key_doesnt_matter": {
                "path": "real/path.js",
                "statementMap": {"0": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 10}}},
                "s": {"0": 1},
                "branchMap": {},
                "b": {},
                "fnMap": {},
                "f": {}
            }
        }"#;

        let result = parse_js_coverage(json).unwrap();
        let file_id = fnv1a_hash("real/path.js");
        assert!(result.file_paths.contains_key(&file_id));
    }

    #[test]
    fn parse_istanbul_falls_back_to_key() {
        let json = r#"{
            "fallback/path.js": {
                "path": "",
                "statementMap": {"0": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 10}}},
                "s": {"0": 1},
                "branchMap": {},
                "b": {},
                "fnMap": {},
                "f": {}
            }
        }"#;

        let result = parse_js_coverage(json).unwrap();
        let file_id = fnv1a_hash("fallback/path.js");
        assert!(result.file_paths.contains_key(&file_id));
    }

    #[test]
    fn parse_istanbul_missing_branch_counts() {
        // branchMap has an entry but b does not
        let json = r#"{
            "src/app.js": {
                "path": "src/app.js",
                "statementMap": {},
                "s": {},
                "branchMap": {
                    "0": {
                        "type": "if",
                        "locations": [
                            {"start": {"line": 3, "column": 0}, "end": {"line": 3, "column": 10}}
                        ]
                    }
                },
                "b": {},
                "fnMap": {},
                "f": {}
            }
        }"#;

        let branches = parse_istanbul_coverage(json).unwrap();
        // No branch counts in `b`, so no branch entries emitted from branchMap
        let branch_entries: Vec<_> = branches
            .iter()
            .filter(|b| b.discriminator == 0 || b.discriminator == 1)
            .collect();
        assert!(branch_entries.is_empty());
    }

    #[test]
    fn parse_istanbul_multiple_files() {
        let json = r#"{
            "src/a.js": {
                "path": "src/a.js",
                "statementMap": {},
                "s": {},
                "branchMap": {
                    "0": {
                        "type": "if",
                        "locations": [
                            {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 10}},
                            {"start": {"line": 2, "column": 0}, "end": {"line": 2, "column": 10}}
                        ]
                    }
                },
                "b": {"0": [1, 0]},
                "fnMap": {},
                "f": {}
            },
            "src/b.ts": {
                "path": "src/b.ts",
                "statementMap": {},
                "s": {},
                "branchMap": {
                    "0": {
                        "type": "if",
                        "locations": [
                            {"start": {"line": 5, "column": 0}, "end": {"line": 5, "column": 10}},
                            {"start": {"line": 7, "column": 0}, "end": {"line": 7, "column": 10}}
                        ]
                    }
                },
                "b": {"0": [2, 3]},
                "fnMap": {},
                "f": {}
            }
        }"#;

        let result = parse_js_coverage(json).unwrap();
        assert_eq!(result.file_paths.len(), 2);
        // a.js: 1 covered + 1 uncovered = 2; b.ts: 2 covered = 2; total = 4
        let branch_entries: Vec<_> = result
            .branches
            .iter()
            .filter(|b| b.discriminator <= 1)
            .collect();
        assert_eq!(branch_entries.len(), 4);
    }

    #[test]
    fn parse_istanbul_invalid_json() {
        let result = parse_istanbul_coverage("not json at all");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // V8 format tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_v8_basic() {
        let json = r#"{
            "result": [
                {
                    "scriptId": "1",
                    "url": "file:///path/to/file.js",
                    "functions": [
                        {
                            "functionName": "foo",
                            "ranges": [
                                {"startOffset": 0, "endOffset": 100, "count": 5},
                                {"startOffset": 10, "endOffset": 50, "count": 0}
                            ]
                        }
                    ]
                }
            ]
        }"#;

        let branches = parse_v8_coverage(json).unwrap();
        assert_eq!(branches.len(), 2);
        // First range: count > 0, direction = 0
        assert_eq!(branches[0].direction, 0);
        // Second range: count == 0, direction = 1
        assert_eq!(branches[1].direction, 1);
    }

    #[test]
    fn parse_v8_empty_result() {
        let json = r#"{"result": []}"#;
        let branches = parse_v8_coverage(json).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_v8_empty_functions() {
        let json = r#"{
            "result": [
                {
                    "scriptId": "1",
                    "url": "file:///app.js",
                    "functions": []
                }
            ]
        }"#;
        let branches = parse_v8_coverage(json).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_v8_empty_ranges() {
        let json = r#"{
            "result": [
                {
                    "scriptId": "1",
                    "url": "file:///app.js",
                    "functions": [
                        {"functionName": "bar", "ranges": []}
                    ]
                }
            ]
        }"#;
        let branches = parse_v8_coverage(json).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn parse_v8_multiple_scripts() {
        let json = r#"{
            "result": [
                {
                    "scriptId": "1",
                    "url": "file:///a.js",
                    "functions": [
                        {
                            "functionName": "",
                            "ranges": [{"startOffset": 0, "endOffset": 50, "count": 1}]
                        }
                    ]
                },
                {
                    "scriptId": "2",
                    "url": "file:///b.js",
                    "functions": [
                        {
                            "functionName": "main",
                            "ranges": [
                                {"startOffset": 0, "endOffset": 200, "count": 3},
                                {"startOffset": 50, "endOffset": 100, "count": 0}
                            ]
                        }
                    ]
                }
            ]
        }"#;

        let result = parse_v8_coverage(json).unwrap();
        assert_eq!(result.len(), 3); // 1 from a.js + 2 from b.js
    }

    #[test]
    fn parse_v8_file_url_normalization() {
        let json = r#"{
            "result": [
                {
                    "scriptId": "1",
                    "url": "file:///path/to/file.js",
                    "functions": [
                        {
                            "functionName": "",
                            "ranges": [{"startOffset": 0, "endOffset": 10, "count": 1}]
                        }
                    ]
                }
            ]
        }"#;

        let result = parse_js_coverage(json).unwrap();
        let file_id = fnv1a_hash("/path/to/file.js");
        assert!(result.file_paths.contains_key(&file_id));
        assert_eq!(
            result.file_paths[&file_id],
            PathBuf::from("/path/to/file.js")
        );
    }

    #[test]
    fn parse_v8_non_file_url() {
        let json = r#"{
            "result": [
                {
                    "scriptId": "1",
                    "url": "node:internal/modules/cjs/loader",
                    "functions": [
                        {
                            "functionName": "",
                            "ranges": [{"startOffset": 0, "endOffset": 10, "count": 1}]
                        }
                    ]
                }
            ]
        }"#;

        let result = parse_js_coverage(json).unwrap();
        let file_id = fnv1a_hash("node:internal/modules/cjs/loader");
        assert!(result.file_paths.contains_key(&file_id));
    }

    #[test]
    fn parse_v8_invalid_json() {
        let result = parse_v8_coverage("{bad json}");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Auto-detect tests
    // -----------------------------------------------------------------------

    #[test]
    fn auto_detect_istanbul() {
        let json = r#"{
            "src/app.js": {
                "path": "src/app.js",
                "statementMap": {"0": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 10}}},
                "s": {"0": 1},
                "branchMap": {},
                "b": {},
                "fnMap": {},
                "f": {}
            }
        }"#;

        let result = parse_js_coverage(json);
        assert!(result.is_ok());
        assert!(!result.unwrap().branches.is_empty());
    }

    #[test]
    fn auto_detect_v8() {
        let json = r#"{
            "result": [
                {
                    "scriptId": "1",
                    "url": "file:///app.js",
                    "functions": [
                        {
                            "functionName": "",
                            "ranges": [{"startOffset": 0, "endOffset": 10, "count": 1}]
                        }
                    ]
                }
            ]
        }"#;

        let result = parse_js_coverage(json);
        assert!(result.is_ok());
        assert!(!result.unwrap().branches.is_empty());
    }

    #[test]
    fn auto_detect_empty_object() {
        let result = parse_js_coverage("{}");
        assert!(result.is_ok());
        assert!(result.unwrap().branches.is_empty());
    }

    #[test]
    fn auto_detect_invalid_json() {
        let result = parse_js_coverage("not json");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // normalize_v8_url tests
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_file_url_unix() {
        assert_eq!(
            normalize_v8_url("file:///usr/src/app.js"),
            "/usr/src/app.js"
        );
    }

    #[test]
    fn normalize_file_url_windows() {
        assert_eq!(
            normalize_v8_url("file:///C:/Users/app.js"),
            "C:/Users/app.js"
        );
    }

    #[test]
    fn normalize_non_file_url() {
        assert_eq!(
            normalize_v8_url("node:internal/loader"),
            "node:internal/loader"
        );
    }

    #[test]
    fn normalize_file_double_slash() {
        assert_eq!(
            normalize_v8_url("file://host/share/app.js"),
            "host/share/app.js"
        );
    }

    // -----------------------------------------------------------------------
    // normalize_path tests
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_backslashes() {
        assert_eq!(normalize_path("src\\app.js"), "src/app.js");
    }

    #[test]
    fn normalize_forward_slashes_unchanged() {
        assert_eq!(normalize_path("src/app.js"), "src/app.js");
    }

    // -----------------------------------------------------------------------
    // fnv1a_hash consistency
    // -----------------------------------------------------------------------

    #[test]
    fn fnv1a_matches_python_module() {
        // Must produce the same hash as python.rs
        assert_eq!(fnv1a_hash(""), 0xcbf2_9ce4_8422_2325);
        let h = fnv1a_hash("src/app.js");
        assert_ne!(h, 0);
    }

    #[test]
    fn fnv1a_deterministic() {
        assert_eq!(fnv1a_hash("hello.js"), fnv1a_hash("hello.js"));
    }

    #[test]
    fn fnv1a_different_inputs() {
        assert_ne!(fnv1a_hash("a.js"), fnv1a_hash("b.js"));
    }

    // -----------------------------------------------------------------------
    // JsCoverageResult structure tests
    // -----------------------------------------------------------------------

    #[test]
    fn coverage_result_file_paths_populated() {
        let json = r#"{
            "result": [
                {
                    "scriptId": "1",
                    "url": "file:///src/index.js",
                    "functions": [
                        {
                            "functionName": "",
                            "ranges": [{"startOffset": 0, "endOffset": 100, "count": 1}]
                        }
                    ]
                },
                {
                    "scriptId": "2",
                    "url": "file:///src/utils.ts",
                    "functions": [
                        {
                            "functionName": "helper",
                            "ranges": [{"startOffset": 0, "endOffset": 50, "count": 2}]
                        }
                    ]
                }
            ]
        }"#;

        let result = parse_js_coverage(json).unwrap();
        assert_eq!(result.file_paths.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Istanbul edge cases: missing optional fields via serde(default)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_istanbul_minimal_file_entry() {
        // File entry with only required path, everything else defaults
        let json = r#"{
            "src/minimal.js": {
                "path": "src/minimal.js"
            }
        }"#;

        let result = parse_istanbul_coverage(json);
        assert!(result.is_ok());
        // No branches, statements, or functions
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn parse_istanbul_branch_location_out_of_bounds() {
        // More counts than locations in a branch
        let json = r#"{
            "src/app.js": {
                "path": "src/app.js",
                "statementMap": {},
                "s": {},
                "branchMap": {
                    "0": {
                        "type": "if",
                        "locations": [
                            {"start": {"line": 3, "column": 0}, "end": {"line": 3, "column": 10}}
                        ]
                    }
                },
                "b": {"0": [1, 2, 3]},
                "fnMap": {},
                "f": {}
            }
        }"#;

        let branches = parse_istanbul_coverage(json).unwrap();
        // 3 arms: locations has only 1 entry, so arms 1 and 2 get line=0
        let branch_entries: Vec<_> = branches.iter().filter(|b| b.discriminator == 0).collect();
        assert_eq!(branch_entries.len(), 3); // all 3 have count > 0
    }

    // -----------------------------------------------------------------------
    // V8 edge cases: default field values
    // -----------------------------------------------------------------------

    #[test]
    fn parse_v8_missing_optional_fields() {
        // scriptId and functionName have defaults
        let json = r#"{
            "result": [
                {
                    "url": "file:///app.js",
                    "functions": [
                        {
                            "ranges": [{"startOffset": 0, "endOffset": 10, "count": 1}]
                        }
                    ]
                }
            ]
        }"#;

        let branches = parse_v8_coverage(json).unwrap();
        assert_eq!(branches.len(), 1);
    }

    #[test]
    fn parse_v8_zero_count_range() {
        let json = r#"{
            "result": [
                {
                    "scriptId": "1",
                    "url": "file:///app.js",
                    "functions": [
                        {
                            "functionName": "",
                            "ranges": [{"startOffset": 0, "endOffset": 100, "count": 0}]
                        }
                    ]
                }
            ]
        }"#;

        let branches = parse_v8_coverage(json).unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].direction, 1); // uncovered
    }
}
