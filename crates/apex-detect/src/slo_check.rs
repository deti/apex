//! SLO Validation — checks code against declared SLO targets.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SloDefinition {
    pub name: String,
    pub latency_ms: Option<u64>,
    pub error_rate_pct: Option<f64>,
    pub availability_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SloIssue {
    pub slo_name: String,
    pub file: PathBuf,
    pub line: u32,
    pub issue: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SloReport {
    pub slo_count: usize,
    pub issues: Vec<SloIssue>,
    pub timeouts_found: Vec<(PathBuf, u32, u64)>, // file, line, timeout_ms
    pub retries_found: Vec<(PathBuf, u32, u32)>,   // file, line, count
    pub health_check_found: bool,
}

static TIMEOUT_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)timeout\s*[:=]\s*(\d+)").unwrap());

static RETRY_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:retries?|retry_count|max_retries)\s*[:=]\s*(\d+)").unwrap()
});

static HEALTH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:/health|/healthz|/readyz|health.check|readiness.probe)"#).unwrap()
});

pub fn check_slos(slos: &[SloDefinition], source_cache: &HashMap<PathBuf, String>) -> SloReport {
    let mut issues = Vec::new();
    let mut timeouts_found = Vec::new();
    let mut retries_found = Vec::new();
    let mut health_check_found = false;

    // Scan source for timeout and retry configurations
    for (path, source) in source_cache {
        for (line_num, line) in source.lines().enumerate() {
            let ln = (line_num + 1) as u32;

            if let Some(cap) = TIMEOUT_PATTERN.captures(line) {
                if let Ok(ms) = cap[1].parse::<u64>() {
                    timeouts_found.push((path.clone(), ln, ms));
                }
            }

            if let Some(cap) = RETRY_PATTERN.captures(line) {
                if let Ok(count) = cap[1].parse::<u32>() {
                    retries_found.push((path.clone(), ln, count));
                }
            }

            if HEALTH_PATTERN.is_match(line) {
                health_check_found = true;
            }
        }
    }

    // Validate against SLO targets
    for slo in slos {
        if let Some(latency_target) = slo.latency_ms {
            for (path, line, timeout_ms) in &timeouts_found {
                if *timeout_ms > latency_target {
                    issues.push(SloIssue {
                        slo_name: slo.name.clone(),
                        file: path.clone(),
                        line: *line,
                        issue: format!(
                            "Timeout {}ms exceeds SLO latency target {}ms",
                            timeout_ms, latency_target
                        ),
                        suggestion: format!(
                            "Reduce timeout to <={}ms or adjust SLO",
                            latency_target
                        ),
                    });
                }
            }
        }

        if slo.latency_ms.is_some() && !health_check_found {
            issues.push(SloIssue {
                slo_name: slo.name.clone(),
                file: PathBuf::new(),
                line: 0,
                issue: "No health check endpoint found".into(),
                suggestion: "Add /health or /healthz endpoint for SLO monitoring".into(),
            });
        }
    }

    SloReport {
        slo_count: slos.len(),
        issues,
        timeouts_found,
        retries_found,
        health_check_found,
    }
}

/// Parse SLO definitions from a JSON string.
pub fn parse_slo_file(content: &str) -> Vec<SloDefinition> {
    // Try JSON first
    if let Ok(slos) = serde_json::from_str::<Vec<SloDefinition>>(content) {
        return slos;
    }
    // Fallback: empty
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_exceeds_slo() {
        let slos = vec![SloDefinition {
            name: "api-latency".into(),
            latency_ms: Some(500),
            error_rate_pct: None,
            availability_pct: None,
        }];
        let mut src = HashMap::new();
        src.insert(PathBuf::from("app.py"), "timeout = 5000".into());
        let r = check_slos(&slos, &src);
        assert!(!r.issues.is_empty());
        assert!(r.issues[0].issue.contains("exceeds SLO"));
    }

    #[test]
    fn timeout_within_slo() {
        let slos = vec![SloDefinition {
            name: "api-latency".into(),
            latency_ms: Some(5000),
            error_rate_pct: None,
            availability_pct: None,
        }];
        let mut src = HashMap::new();
        src.insert(PathBuf::from("app.py"), "timeout = 1000".into());
        let r = check_slos(&slos, &src);
        // Only health check issue, no timeout issue
        assert!(r.issues.iter().all(|i| !i.issue.contains("exceeds")));
    }

    #[test]
    fn detects_health_check() {
        let slos = vec![];
        let mut src = HashMap::new();
        src.insert(PathBuf::from("app.py"), "@app.get('/healthz')".into());
        let r = check_slos(&slos, &src);
        assert!(r.health_check_found);
    }

    #[test]
    fn detects_retries() {
        let slos = vec![];
        let mut src = HashMap::new();
        src.insert(PathBuf::from("cfg.py"), "max_retries = 3".into());
        let r = check_slos(&slos, &src);
        assert_eq!(r.retries_found.len(), 1);
        assert_eq!(r.retries_found[0].2, 3);
    }

    #[test]
    fn parse_slo_json() {
        let json = r#"[{"name":"api","latency_ms":500,"error_rate_pct":1.0,"availability_pct":99.9}]"#;
        let slos = parse_slo_file(json);
        assert_eq!(slos.len(), 1);
        assert_eq!(slos[0].name, "api");
    }

    #[test]
    fn no_slos_no_issues() {
        let r = check_slos(&[], &HashMap::new());
        assert!(r.issues.is_empty());
    }
}
