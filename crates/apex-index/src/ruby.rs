use apex_core::hash::fnv1a_hash;
use apex_core::types::BranchId;
use std::collections::HashMap;
use std::path::PathBuf;

/// Parse SimpleCov JSON coverage into branch data for indexing.
pub fn parse_simplecov_coverage(json: &str) -> RubyCoverageData {
    let mut data = RubyCoverageData::default();

    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return data,
    };

    let coverage = match parsed.get("coverage").and_then(|c| c.as_object()) {
        Some(c) => c,
        None => return data,
    };

    for (file_path, file_data) in coverage {
        let file_id = fnv1a_hash(file_path);
        data.file_paths.insert(file_id, PathBuf::from(file_path));

        let lines = file_data
            .get("lines")
            .and_then(|l| l.as_array())
            .cloned()
            .unwrap_or_default();

        for (i, val) in lines.iter().enumerate() {
            if val.is_null() {
                continue;
            }
            let count = val.as_u64().unwrap_or(0);
            let line = (i + 1) as u32;
            let branch = BranchId::new(file_id, line, 0, 0);
            data.all_branches.push(branch.clone());
            data.total += 1;
            if count > 0 {
                data.covered_branches.push(branch);
                data.covered += 1;
            }
        }
    }

    data
}

#[derive(Debug, Default)]
pub struct RubyCoverageData {
    pub all_branches: Vec<BranchId>,
    pub covered_branches: Vec<BranchId>,
    pub file_paths: HashMap<u64, PathBuf>,
    pub total: usize,
    pub covered: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let json = r#"{"coverage":{"app.rb":{"lines":[null,1,0,1]}}}"#;
        let data = parse_simplecov_coverage(json);
        assert_eq!(data.total, 3);
        assert_eq!(data.covered, 2);
    }

    #[test]
    fn parse_empty() {
        let data = parse_simplecov_coverage(r#"{"coverage":{}}"#);
        assert_eq!(data.total, 0);
    }

    #[test]
    fn parse_invalid() {
        let data = parse_simplecov_coverage("bad");
        assert_eq!(data.total, 0);
    }

    #[test]
    fn parse_multiple_files() {
        let json = r#"{"coverage":{"a.rb":{"lines":[1]},"b.rb":{"lines":[0,1]}}}"#;
        let data = parse_simplecov_coverage(json);
        assert_eq!(data.total, 3);
        assert_eq!(data.file_paths.len(), 2);
    }
}
