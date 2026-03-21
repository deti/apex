//! Import external coverage data files into APEX branch IDs.
//!
//! When the user provides `--coverage-file coverage.json`, APEX skips
//! `install_deps` + `instrument` entirely and parses the user's coverage data
//! directly via [`load_coverage_file`].

use apex_core::error::{ApexError, Result};
use apex_core::hash::fnv1a_hash;
use apex_core::types::BranchId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parsed coverage data: `(all_branches, executed_branches, file_paths)`.
pub type CoverageData = (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>);

/// Supported coverage data formats.
#[derive(Debug, Clone, Copy)]
pub enum CoverageFormat {
    /// coverage.py JSON (`meta` + `files` keys)
    CoveragePy,
    /// llvm-cov export JSON (`data` array)
    LlvmCov,
    /// Istanbul/nyc JSON (file-path keys with `branchMap`)
    Istanbul,
    /// V8 coverage JSON (`result` key or top-level array)
    V8,
    /// `go test -coverprofile` text format
    GoCover,
    /// JaCoCo XML (`<report>` root element)
    Jacoco,
    /// Cobertura XML (`<coverage>` root element)
    Cobertura,
    /// SimpleCov JSON (runner keys with `coverage` sub-objects)
    SimpleCov,
    /// LCOV info text format (`SF:`, `DA:`, `BRDA:`)
    Lcov,
}

/// Auto-detect coverage format from file content.
pub fn detect_format(content: &[u8]) -> Result<CoverageFormat> {
    let text = String::from_utf8_lossy(content);
    let trimmed = text.trim_start();

    // Text-based formats
    if trimmed.starts_with("mode: ") {
        return Ok(CoverageFormat::GoCover);
    }
    if trimmed.starts_with("TN:") || trimmed.starts_with("SF:") {
        return Ok(CoverageFormat::Lcov);
    }

    // XML formats
    if trimmed.contains("<coverage ") || trimmed.contains("<coverage>") {
        return Ok(CoverageFormat::Cobertura);
    }
    if trimmed.contains("<report ") || trimmed.contains("<report>") {
        return Ok(CoverageFormat::Jacoco);
    }

    // JSON formats
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let val: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| ApexError::Instrumentation(format!("invalid JSON: {e}")))?;

        // coverage.py: has "meta" + "files" top-level keys
        if val.get("meta").is_some() && val.get("files").is_some() {
            return Ok(CoverageFormat::CoveragePy);
        }
        // llvm-cov export: has "data" array
        if val
            .get("data")
            .and_then(|d| d.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false)
        {
            return Ok(CoverageFormat::LlvmCov);
        }
        // V8: has "result" key, or is an array of script coverage objects
        if val.get("result").is_some() {
            return Ok(CoverageFormat::V8);
        }
        if val.as_array().is_some() {
            return Ok(CoverageFormat::V8);
        }
        // Istanbul: top-level object where values have "branchMap"
        if val
            .as_object()
            .map(|o| o.values().any(|v| v.get("branchMap").is_some()))
            .unwrap_or(false)
        {
            return Ok(CoverageFormat::Istanbul);
        }
        // SimpleCov: top-level object where values have "coverage"
        if val
            .as_object()
            .map(|o| o.values().any(|v| v.get("coverage").is_some()))
            .unwrap_or(false)
        {
            return Ok(CoverageFormat::SimpleCov);
        }
    }

    Err(ApexError::Instrumentation(
        "unable to detect coverage format".into(),
    ))
}

/// Parse a coverage file into APEX branch data.
///
/// Returns `(all_branches, executed_branches, file_paths)`.
pub fn load_coverage_file(
    path: &Path,
    target_root: &Path,
    format_hint: Option<CoverageFormat>,
) -> Result<CoverageData> {
    let content = std::fs::read(path)
        .map_err(|e| ApexError::Instrumentation(format!("read {}: {e}", path.display())))?;

    let format = match format_hint {
        Some(f) => f,
        None => detect_format(&content)?,
    };
    let text = String::from_utf8_lossy(&content);

    match format {
        CoverageFormat::Lcov => parse_lcov(&text),
        CoverageFormat::GoCover => {
            let (all, exec, paths) = crate::go::parse_coverage_out(&text, target_root);
            Ok((all, exec, paths))
        }
        CoverageFormat::Cobertura => {
            let (all, exec, paths) = crate::csharp::parse_cobertura_xml(&text, target_root);
            Ok((all, exec, paths))
        }
        CoverageFormat::CoveragePy => parse_coverage_py(&text),
        CoverageFormat::LlvmCov => {
            let filter = crate::llvm_coverage::FileFilter::default();
            let result = crate::llvm_coverage::parse_llvm_cov_export(&content, target_root, &filter)
                .map_err(|e| ApexError::Instrumentation(format!("llvm-cov JSON: {e}")))?;
            Ok((result.branch_ids, result.executed_branch_ids, result.file_paths))
        }
        CoverageFormat::SimpleCov => {
            let (all, exec, paths) = crate::ruby::parse_simplecov_json(&text);
            Ok((all, exec, paths))
        }
        _ => Err(ApexError::Instrumentation(format!(
            "format {format:?} import not yet implemented \
             — use lcov, cobertura, llvm-cov, go-cover, simplecov, or coverage.py"
        ))),
    }
}

// ─── LCOV parser ────────────────────────────────────────────────────────────

/// Parse LCOV info format into branch data.
///
/// Handles both `DA:` (line coverage) and `BRDA:` (branch coverage) records.
pub fn parse_lcov(
    content: &str,
) -> Result<CoverageData> {
    let mut all = Vec::new();
    let mut executed = Vec::new();
    let mut file_paths = HashMap::new();
    let mut current_file_id: u64 = 0;

    for line in content.lines() {
        if let Some(path) = line.strip_prefix("SF:") {
            let path = path.trim();
            current_file_id = fnv1a_hash(path);
            file_paths
                .entry(current_file_id)
                .or_insert_with(|| PathBuf::from(path));
        } else if let Some(da) = line.strip_prefix("DA:") {
            // DA:line_number,execution_count[,checksum]
            let parts: Vec<&str> = da.split(',').collect();
            if parts.len() >= 2 {
                if let Ok(line_num) = parts[0].parse::<u32>() {
                    let count: i64 = parts[1].parse().unwrap_or(0);
                    let bid = BranchId::new(current_file_id, line_num, 0, 0);
                    all.push(bid.clone());
                    if count > 0 {
                        executed.push(bid);
                    }
                }
            }
        } else if let Some(brda) = line.strip_prefix("BRDA:") {
            // BRDA:line,block,branch,count
            let parts: Vec<&str> = brda.split(',').collect();
            if parts.len() >= 4 {
                if let Ok(line_num) = parts[0].parse::<u32>() {
                    let branch: u32 = parts[2].parse().unwrap_or(0);
                    let count_str = parts[3].trim();
                    let count: i64 = if count_str == "-" {
                        0
                    } else {
                        count_str.parse().unwrap_or(0)
                    };
                    let bid = BranchId::new(current_file_id, line_num, 0, branch as u8);
                    all.push(bid.clone());
                    if count > 0 {
                        executed.push(bid);
                    }
                }
            }
        }
    }

    Ok((all, executed, file_paths))
}

// ─── coverage.py JSON parser ────────────────────────────────────────────────

/// Parse coverage.py JSON export format.
///
/// Expected structure:
/// ```json
/// { "meta": { "version": "7.x" }, "files": { "path": { "executed_lines": [...], "missing_lines": [...], ... } } }
/// ```
fn parse_coverage_py(
    content: &str,
) -> Result<CoverageData> {
    let val: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| ApexError::Instrumentation(format!("invalid JSON: {e}")))?;

    let mut all = Vec::new();
    let mut executed = Vec::new();
    let mut file_paths = HashMap::new();

    let files = val
        .get("files")
        .and_then(|f| f.as_object())
        .ok_or_else(|| ApexError::Instrumentation("missing 'files' key".into()))?;

    for (path, fdata) in files {
        let file_id = fnv1a_hash(path);
        file_paths
            .entry(file_id)
            .or_insert_with(|| PathBuf::from(path));

        // executed_lines → covered line branches
        if let Some(exec_lines) = fdata.get("executed_lines").and_then(|v| v.as_array()) {
            for line_val in exec_lines {
                if let Some(line_num) = line_val.as_u64() {
                    let bid = BranchId::new(file_id, line_num as u32, 0, 0);
                    all.push(bid.clone());
                    executed.push(bid);
                }
            }
        }

        // missing_lines → uncovered line branches
        if let Some(miss_lines) = fdata.get("missing_lines").and_then(|v| v.as_array()) {
            for line_val in miss_lines {
                if let Some(line_num) = line_val.as_u64() {
                    let bid = BranchId::new(file_id, line_num as u32, 0, 0);
                    all.push(bid);
                }
            }
        }

        // missing_branches → uncovered branch pairs [from, to]
        if let Some(missing_br) = fdata.get("missing_branches").and_then(|v| v.as_array()) {
            for pair in missing_br {
                if let Some(arr) = pair.as_array() {
                    if arr.len() >= 2 {
                        let from_line = arr[0].as_i64().unwrap_or(0).unsigned_abs() as u32;
                        let direction = if arr[1].as_i64().unwrap_or(0) < 0 {
                            1u8
                        } else {
                            0u8
                        };
                        let bid = BranchId::new(file_id, from_line, 0, direction);
                        all.push(bid);
                    }
                }
            }
        }

        // executed_branches → covered branch pairs [from, to]
        if let Some(exec_br) = fdata.get("executed_branches").and_then(|v| v.as_array()) {
            for pair in exec_br {
                if let Some(arr) = pair.as_array() {
                    if arr.len() >= 2 {
                        let from_line = arr[0].as_i64().unwrap_or(0).unsigned_abs() as u32;
                        let direction = if arr[1].as_i64().unwrap_or(0) < 0 {
                            1u8
                        } else {
                            0u8
                        };
                        let bid = BranchId::new(file_id, from_line, 0, direction);
                        all.push(bid.clone());
                        executed.push(bid);
                    }
                }
            }
        }
    }

    Ok((all, executed, file_paths))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_format tests ─────────────────────────────────────────────

    #[test]
    fn detect_lcov() {
        let data = b"TN:test_name\nSF:/src/main.rs\nDA:1,1\nend_of_record\n";
        assert!(matches!(detect_format(data).unwrap(), CoverageFormat::Lcov));
    }

    #[test]
    fn detect_lcov_sf_first() {
        let data = b"SF:/src/main.rs\nDA:1,1\nend_of_record\n";
        assert!(matches!(detect_format(data).unwrap(), CoverageFormat::Lcov));
    }

    #[test]
    fn detect_go_cover() {
        let data = b"mode: set\nexample.com/pkg/main.go:10.2,12.16 1 1\n";
        assert!(matches!(
            detect_format(data).unwrap(),
            CoverageFormat::GoCover
        ));
    }

    #[test]
    fn detect_cobertura() {
        let data = br#"<?xml version="1.0"?>
<coverage version="5.5">
  <packages><package name="pkg"/></packages>
</coverage>"#;
        assert!(matches!(
            detect_format(data).unwrap(),
            CoverageFormat::Cobertura
        ));
    }

    #[test]
    fn detect_jacoco() {
        let data = br#"<?xml version="1.0"?>
<report name="JaCoCo">
  <package name="com.example"/>
</report>"#;
        assert!(matches!(
            detect_format(data).unwrap(),
            CoverageFormat::Jacoco
        ));
    }

    #[test]
    fn detect_coverage_py() {
        let data = br#"{"meta": {"version": "7.4"}, "files": {"src/app.py": {}}}"#;
        assert!(matches!(
            detect_format(data).unwrap(),
            CoverageFormat::CoveragePy
        ));
    }

    #[test]
    fn detect_llvm_cov() {
        let data = br#"{"data": [{"files": [], "totals": {}}], "type": "llvm.coverage.json.export"}"#;
        assert!(matches!(
            detect_format(data).unwrap(),
            CoverageFormat::LlvmCov
        ));
    }

    #[test]
    fn detect_v8_result() {
        let data = br#"{"result": [{"scriptId": "1", "url": "file:///app.js"}]}"#;
        assert!(matches!(detect_format(data).unwrap(), CoverageFormat::V8));
    }

    #[test]
    fn detect_v8_array() {
        let data = br#"[{"scriptId": "1", "url": "file:///app.js"}]"#;
        assert!(matches!(detect_format(data).unwrap(), CoverageFormat::V8));
    }

    #[test]
    fn detect_istanbul() {
        let data = br#"{"/src/index.js": {"branchMap": {}, "s": {}, "f": {}}}"#;
        assert!(matches!(
            detect_format(data).unwrap(),
            CoverageFormat::Istanbul
        ));
    }

    #[test]
    fn detect_simplecov() {
        let data = br#"{"RSpec": {"coverage": {"/app/models/user.rb": [1,1,null,0]}}}"#;
        assert!(matches!(
            detect_format(data).unwrap(),
            CoverageFormat::SimpleCov
        ));
    }

    #[test]
    fn detect_format_unknown() {
        let data = b"this is not a coverage file";
        assert!(detect_format(data).is_err());
    }

    // ── parse_lcov tests ────────────────────────────────────────────────

    #[test]
    fn parse_lcov_realistic() {
        let lcov = "\
TN:test_name
SF:src/lib.rs
DA:1,5
DA:2,5
DA:3,0
DA:4,3
DA:10,0
LF:5
LH:3
end_of_record
SF:src/main.rs
DA:1,1
DA:2,0
LF:2
LH:1
end_of_record
";
        let (all, exec, paths) = parse_lcov(lcov).unwrap();
        // 5 lines in lib.rs + 2 lines in main.rs = 7
        assert_eq!(all.len(), 7);
        // 3 hit in lib.rs (lines 1,2,4) + 1 hit in main.rs (line 1) = 4
        assert_eq!(exec.len(), 4);
        assert_eq!(paths.len(), 2);
        assert!(paths.values().any(|p| p == Path::new("src/lib.rs")));
        assert!(paths.values().any(|p| p == Path::new("src/main.rs")));
    }

    #[test]
    fn parse_lcov_with_brda() {
        let lcov = "\
TN:
SF:src/lib.rs
DA:10,5
BRDA:10,0,0,5
BRDA:10,0,1,0
BRDA:15,1,0,3
BRDA:15,1,1,-
end_of_record
";
        let (all, exec, _paths) = parse_lcov(lcov).unwrap();
        // 1 DA + 4 BRDA = 5
        assert_eq!(all.len(), 5);
        // DA:10 hit + BRDA:10,0,0 hit + BRDA:15,1,0 hit = 3
        assert_eq!(exec.len(), 3);
    }

    #[test]
    fn parse_lcov_da_only_no_branches() {
        let lcov = "\
SF:src/app.py
DA:1,1
DA:2,0
DA:3,1
end_of_record
";
        let (all, exec, paths) = parse_lcov(lcov).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(exec.len(), 2);
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn parse_lcov_empty() {
        let (all, exec, paths) = parse_lcov("").unwrap();
        assert!(all.is_empty());
        assert!(exec.is_empty());
        assert!(paths.is_empty());
    }

    // ── parse_coverage_py tests ─────────────────────────────────────────

    #[test]
    fn parse_coverage_py_basic() {
        let json = r#"{
            "meta": {"version": "7.4"},
            "files": {
                "src/app.py": {
                    "executed_lines": [1, 2, 5],
                    "missing_lines": [3, 4],
                    "executed_branches": [[5, 6]],
                    "missing_branches": [[5, -8]]
                }
            }
        }"#;
        let (all, exec, paths) = parse_coverage_py(json).unwrap();
        // 3 exec lines + 2 missing lines + 1 exec branch + 1 missing branch = 7
        assert_eq!(all.len(), 7);
        // 3 exec lines + 1 exec branch = 4
        assert_eq!(exec.len(), 4);
        assert_eq!(paths.len(), 1);
    }

    // ── load_coverage_file end-to-end ───────────────────────────────────

    #[test]
    fn load_coverage_file_lcov_e2e() {
        let dir = tempfile::tempdir().unwrap();
        let lcov_path = dir.path().join("coverage.lcov");
        std::fs::write(
            &lcov_path,
            "\
TN:
SF:src/lib.rs
DA:1,1
DA:2,0
DA:3,1
BRDA:3,0,0,1
BRDA:3,0,1,0
end_of_record
",
        )
        .unwrap();

        let (all, exec, paths) =
            load_coverage_file(&lcov_path, dir.path(), None).unwrap();
        // 3 DA + 2 BRDA = 5
        assert_eq!(all.len(), 5);
        // DA:1 + DA:3 + BRDA:3,0,0 = 3
        assert_eq!(exec.len(), 3);
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn load_coverage_file_with_format_hint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.txt");
        std::fs::write(
            &path,
            "SF:src/main.rs\nDA:1,1\nDA:2,0\nend_of_record\n",
        )
        .unwrap();

        let (all, exec, _) =
            load_coverage_file(&path, dir.path(), Some(CoverageFormat::Lcov)).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(exec.len(), 1);
    }

    #[test]
    fn load_coverage_file_missing_file() {
        let result = load_coverage_file(
            Path::new("/nonexistent/coverage.lcov"),
            Path::new("/tmp"),
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn load_coverage_file_unimplemented_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cov.json");
        std::fs::write(&path, r#"{"result": [{"scriptId": "1"}]}"#).unwrap();

        // V8 format is not yet implemented
        let result = load_coverage_file(&path, dir.path(), None);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("not yet implemented"));
    }
}
