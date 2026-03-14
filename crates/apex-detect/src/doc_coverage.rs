//! Documentation Coverage — measures API doc completeness.

use apex_core::error::{ApexError, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DocIssue {
    pub path: String,
    pub method: String,
    pub issue: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocCoverageReport {
    pub total_endpoints: usize,
    pub documented_endpoints: usize,
    pub coverage_pct: f64,
    pub issues: Vec<DocIssue>,
}

pub fn analyze_doc_coverage(spec_json: &str) -> Result<DocCoverageReport> {
    let spec: serde_json::Value = serde_json::from_str(spec_json)
        .map_err(|e| ApexError::Detect(format!("invalid spec: {e}")))?;

    let mut total = 0usize;
    let mut documented = 0usize;
    let mut issues = Vec::new();

    let methods = ["get", "post", "put", "delete", "patch"];

    if let Some(paths) = spec.get("paths").and_then(|p| p.as_object()) {
        for (path, item) in paths {
            for method in &methods {
                if let Some(op) = item.get(method) {
                    total += 1;
                    let mut is_documented = true;

                    // Check description
                    if op
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .is_empty()
                        && op
                            .get("summary")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .is_empty()
                    {
                        issues.push(DocIssue {
                            path: path.clone(),
                            method: method.to_uppercase(),
                            issue: "Missing description/summary".into(),
                        });
                        is_documented = false;
                    }

                    // Check responses
                    if op.get("responses").is_none() {
                        issues.push(DocIssue {
                            path: path.clone(),
                            method: method.to_uppercase(),
                            issue: "Missing responses section".into(),
                        });
                        is_documented = false;
                    }

                    // Check parameters have descriptions
                    if let Some(params) = op.get("parameters").and_then(|p| p.as_array()) {
                        for param in params {
                            if param
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("")
                                .is_empty()
                            {
                                let param_name = param
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("?");
                                issues.push(DocIssue {
                                    path: path.clone(),
                                    method: method.to_uppercase(),
                                    issue: format!(
                                        "Parameter '{}' missing description",
                                        param_name
                                    ),
                                });
                            }
                        }
                    }

                    if is_documented {
                        documented += 1;
                    }
                }
            }
        }
    }

    let coverage_pct = if total > 0 {
        (documented as f64 / total as f64) * 100.0
    } else {
        100.0
    };

    Ok(DocCoverageReport {
        total_endpoints: total,
        documented_endpoints: documented,
        coverage_pct,
        issues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fully_documented_spec() {
        let spec = r#"{"paths":{"/users":{"get":{"description":"List users","responses":{"200":{}}}}}}"#;
        let r = analyze_doc_coverage(spec).unwrap();
        assert_eq!(r.total_endpoints, 1);
        assert_eq!(r.documented_endpoints, 1);
        assert_eq!(r.issues.len(), 0);
    }

    #[test]
    fn missing_description() {
        let spec = r#"{"paths":{"/users":{"get":{"responses":{"200":{}}}}}}"#;
        let r = analyze_doc_coverage(spec).unwrap();
        assert_eq!(r.documented_endpoints, 0);
        assert!(r.issues.iter().any(|i| i.issue.contains("description")));
    }

    #[test]
    fn missing_responses() {
        let spec = r#"{"paths":{"/users":{"get":{"description":"List users"}}}}"#;
        let r = analyze_doc_coverage(spec).unwrap();
        assert!(r.issues.iter().any(|i| i.issue.contains("responses")));
    }

    #[test]
    fn empty_spec() {
        let spec = r#"{"paths":{}}"#;
        let r = analyze_doc_coverage(spec).unwrap();
        assert_eq!(r.total_endpoints, 0);
        assert_eq!(r.coverage_pct, 100.0);
    }

    #[test]
    fn param_missing_description() {
        let spec = r#"{"paths":{"/users/{id}":{"get":{"description":"Get user","responses":{"200":{}},"parameters":[{"name":"id","in":"path"}]}}}}"#;
        let r = analyze_doc_coverage(spec).unwrap();
        assert!(r.issues.iter().any(|i| i.issue.contains("Parameter 'id'")));
    }
}
