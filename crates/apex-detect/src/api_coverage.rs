//! API Spec Coverage — compares OpenAPI spec against code route handlers.

use apex_core::error::{ApexError, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub enum EndpointStatus {
    Implemented,
    SpecOnly,
    CodeOnly,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointCoverage {
    pub method: String,
    pub path: String,
    pub status: EndpointStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiCoverageReport {
    pub endpoints: Vec<EndpointCoverage>,
    pub spec_count: usize,
    pub implemented_count: usize,
    pub spec_only_count: usize,
    pub code_only_count: usize,
}

pub fn analyze_coverage(
    spec_json: &str,
    source_cache: &HashMap<PathBuf, String>,
    lang: apex_core::types::Language,
) -> Result<ApiCoverageReport> {
    let spec: serde_json::Value = serde_json::from_str(spec_json)
        .map_err(|e| ApexError::Detect(format!("invalid OpenAPI spec: {e}")))?;

    // Extract endpoints from spec
    let mut spec_endpoints: Vec<(String, String)> = Vec::new();
    if let Some(paths) = spec.get("paths").and_then(|p| p.as_object()) {
        for (path, item) in paths {
            for method in &["get", "post", "put", "delete", "patch", "head", "options"] {
                if item.get(method).is_some() {
                    spec_endpoints.push((method.to_uppercase(), path.clone()));
                }
            }
        }
    }

    // Extract route handlers from source code using regex patterns
    let route_patterns: Vec<regex::Regex> = match lang {
        apex_core::types::Language::Python => vec![
            regex::Regex::new(r#"@(?:app|router)\.(get|post|put|delete|patch)\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap(),
            regex::Regex::new(r#"path\(\s*['"]([^'"]+)['"]"#).unwrap(),
        ],
        apex_core::types::Language::JavaScript => vec![
            regex::Regex::new(r#"(?:app|router)\.(get|post|put|delete|patch)\s*\(\s*['"]([^'"]+)['"]"#).unwrap(),
        ],
        _ => vec![],
    };

    let mut code_endpoints: Vec<(String, String)> = Vec::new();
    for source in source_cache.values() {
        for re in &route_patterns {
            for cap in re.captures_iter(source) {
                if cap.len() >= 3 {
                    let method = cap[1].to_uppercase();
                    let path = cap[2].to_string();
                    code_endpoints.push((method, path));
                } else if cap.len() >= 2 {
                    // path() pattern — method unknown
                    code_endpoints.push(("ANY".into(), cap[1].to_string()));
                }
            }
        }
    }

    // Cross-reference
    let mut endpoints = Vec::new();
    let mut implemented = 0usize;
    let mut spec_only = 0usize;

    for (method, path) in &spec_endpoints {
        let found = code_endpoints
            .iter()
            .any(|(cm, cp)| cm == method && paths_match(cp, path));
        if found {
            endpoints.push(EndpointCoverage {
                method: method.clone(),
                path: path.clone(),
                status: EndpointStatus::Implemented,
            });
            implemented += 1;
        } else {
            endpoints.push(EndpointCoverage {
                method: method.clone(),
                path: path.clone(),
                status: EndpointStatus::SpecOnly,
            });
            spec_only += 1;
        }
    }

    let mut code_only = 0usize;
    for (method, path) in &code_endpoints {
        let in_spec = spec_endpoints
            .iter()
            .any(|(sm, sp)| sm == method && paths_match(path, sp));
        if !in_spec {
            endpoints.push(EndpointCoverage {
                method: method.clone(),
                path: path.clone(),
                status: EndpointStatus::CodeOnly,
            });
            code_only += 1;
        }
    }

    Ok(ApiCoverageReport {
        spec_count: spec_endpoints.len(),
        implemented_count: implemented,
        spec_only_count: spec_only,
        code_only_count: code_only,
        endpoints,
    })
}

fn paths_match(code_path: &str, spec_path: &str) -> bool {
    let param_re = regex::Regex::new(r"\{[^}]+\}").unwrap();
    let code_param_re = regex::Regex::new(r"<[^>]+>|:\w+").unwrap();
    let spec_norm = param_re.replace_all(spec_path, ":param").to_string();
    let code_norm = code_param_re.replace_all(code_path, ":param").to_string();
    code_norm == spec_norm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_implemented_endpoints() {
        let spec = r#"{"paths":{"/users":{"get":{},"post":{}}}}"#;
        let mut src = HashMap::new();
        src.insert(
            PathBuf::from("app.py"),
            r#"@app.get("/users")\ndef list(): pass\n@app.post("/users")\ndef create(): pass"#
                .into(),
        );
        let r = analyze_coverage(spec, &src, apex_core::types::Language::Python).unwrap();
        assert_eq!(r.implemented_count, 2);
    }

    #[test]
    fn detects_spec_only() {
        let spec = r#"{"paths":{"/users":{"get":{}}}}"#;
        let src = HashMap::new();
        let r = analyze_coverage(spec, &src, apex_core::types::Language::Python).unwrap();
        assert_eq!(r.spec_only_count, 1);
    }

    #[test]
    fn paths_match_with_params() {
        assert!(paths_match("/users/:id", "/users/{id}"));
        assert!(!paths_match("/users", "/posts"));
    }
}
