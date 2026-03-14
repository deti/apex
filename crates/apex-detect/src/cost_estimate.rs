//! Cost Estimation — estimates cloud costs from code patterns.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub struct CostDriver {
    pub file: PathBuf,
    pub line: u32,
    pub category: String,
    pub pattern: String,
    pub estimated_monthly_cost: Option<f64>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CostReport {
    pub drivers: Vec<CostDriver>,
    pub total_estimated_monthly: f64,
}

static DB_QUERY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:cursor\.execute|\.query\(|\.find\(|\.aggregate\()").unwrap()
});
static S3_OP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:s3\.put_object|s3\.get_object|upload_file|download_file)").unwrap()
});
static LAMBDA_INVOKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:lambda.*invoke|invoke_function)").unwrap());
static API_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:requests\.\w+|httpx\.\w+|fetch\()").unwrap()
});

pub fn estimate_costs(source_cache: &HashMap<PathBuf, String>) -> CostReport {
    let mut drivers = Vec::new();

    let patterns: Vec<(&LazyLock<Regex>, &str, &str, f64)> = vec![
        (&DB_QUERY, "database", "Database query", 0.01),
        (&S3_OP, "storage", "S3 operation", 0.005),
        (&LAMBDA_INVOKE, "compute", "Lambda invocation", 0.02),
        (&API_CALL, "network", "External API call", 0.001),
    ];

    let mut total = 0.0f64;

    for (path, source) in source_cache {
        for (line_num, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            let ln = (line_num + 1) as u32;
            for (re, cat, desc, cost_per) in &patterns {
                if re.is_match(trimmed) {
                    // Rough estimate: assume 1000 calls/day * 30 days
                    let monthly = cost_per * 30000.0;
                    total += monthly;
                    drivers.push(CostDriver {
                        file: path.clone(),
                        line: ln,
                        category: cat.to_string(),
                        pattern: trimmed.to_string(),
                        estimated_monthly_cost: Some(monthly),
                        description: desc.to_string(),
                    });
                    break;
                }
            }
        }
    }

    CostReport {
        drivers,
        total_estimated_monthly: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_db_queries() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("app.py"),
            "cursor.execute('SELECT 1')".into(),
        );
        let r = estimate_costs(&c);
        assert!(r.drivers.iter().any(|d| d.category == "database"));
    }

    #[test]
    fn detects_s3_ops() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("storage.py"),
            "s3.put_object(Bucket='b', Key='k', Body=data)".into(),
        );
        let r = estimate_costs(&c);
        assert!(r.drivers.iter().any(|d| d.category == "storage"));
    }

    #[test]
    fn estimates_monthly_cost() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("app.py"),
            "cursor.execute('SELECT 1')".into(),
        );
        let r = estimate_costs(&c);
        assert!(r.total_estimated_monthly > 0.0);
    }

    #[test]
    fn empty_source() {
        let r = estimate_costs(&HashMap::new());
        assert!(r.drivers.is_empty());
        assert_eq!(r.total_estimated_monthly, 0.0);
    }
}
