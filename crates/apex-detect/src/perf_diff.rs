//! Performance Regression Detection — compares timing data against baselines.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfBaseline {
    pub entries: HashMap<String, PerfEntry>, // function_name -> timing
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfEntry {
    pub mean_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerfRegression {
    pub function: String,
    pub metric: String, // "mean", "p95", "p99"
    pub baseline: f64,
    pub current: f64,
    pub change_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerfDiffReport {
    pub regressions: Vec<PerfRegression>,
    pub improvements: Vec<PerfRegression>,
    pub unchanged: usize,
    pub threshold_pct: f64,
}

pub fn diff_perf(
    baseline: &PerfBaseline,
    current: &PerfBaseline,
    threshold_pct: f64,
) -> PerfDiffReport {
    let mut regressions = Vec::new();
    let mut improvements = Vec::new();
    let mut unchanged = 0usize;

    for (func, base_entry) in &baseline.entries {
        if let Some(cur_entry) = current.entries.get(func) {
            let checks = [
                ("mean", base_entry.mean_ms, cur_entry.mean_ms),
                ("p95", base_entry.p95_ms, cur_entry.p95_ms),
                ("p99", base_entry.p99_ms, cur_entry.p99_ms),
            ];

            let mut has_regression = false;
            let mut has_improvement = false;

            for (metric, base_val, cur_val) in &checks {
                if *base_val == 0.0 {
                    continue;
                }
                let change_pct = ((cur_val - base_val) / base_val) * 100.0;

                if change_pct > threshold_pct {
                    regressions.push(PerfRegression {
                        function: func.clone(),
                        metric: metric.to_string(),
                        baseline: *base_val,
                        current: *cur_val,
                        change_pct,
                    });
                    has_regression = true;
                } else if change_pct < -threshold_pct {
                    improvements.push(PerfRegression {
                        function: func.clone(),
                        metric: metric.to_string(),
                        baseline: *base_val,
                        current: *cur_val,
                        change_pct,
                    });
                    has_improvement = true;
                }
            }

            if !has_regression && !has_improvement {
                unchanged += 1;
            }
        }
    }

    // Sort regressions by severity (largest change first)
    regressions.sort_by(|a, b| {
        b.change_pct
            .partial_cmp(&a.change_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    PerfDiffReport {
        regressions,
        improvements,
        unchanged,
        threshold_pct,
    }
}

/// Parse a perf baseline from JSON.
pub fn parse_baseline(json: &str) -> Option<PerfBaseline> {
    serde_json::from_str(json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_baseline(entries: Vec<(&str, f64)>) -> PerfBaseline {
        PerfBaseline {
            entries: entries
                .into_iter()
                .map(|(name, ms)| {
                    (
                        name.to_string(),
                        PerfEntry {
                            mean_ms: ms,
                            p95_ms: ms * 1.5,
                            p99_ms: ms * 2.0,
                            samples: 100,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn detects_regression() {
        let base = make_baseline(vec![("func_a", 100.0)]);
        let current = make_baseline(vec![("func_a", 130.0)]); // 30% slower
        let report = diff_perf(&base, &current, 10.0);
        assert!(!report.regressions.is_empty());
        assert!(report.regressions[0].change_pct > 25.0);
    }

    #[test]
    fn detects_improvement() {
        let base = make_baseline(vec![("func_a", 100.0)]);
        let current = make_baseline(vec![("func_a", 70.0)]); // 30% faster
        let report = diff_perf(&base, &current, 10.0);
        assert!(!report.improvements.is_empty());
    }

    #[test]
    fn no_change_within_threshold() {
        let base = make_baseline(vec![("func_a", 100.0)]);
        let current = make_baseline(vec![("func_a", 105.0)]); // 5% — within 10% threshold
        let report = diff_perf(&base, &current, 10.0);
        assert!(report.regressions.is_empty());
        assert_eq!(report.unchanged, 1);
    }

    #[test]
    fn missing_function_in_current_ignored() {
        let base = make_baseline(vec![("func_a", 100.0), ("func_b", 50.0)]);
        let current = make_baseline(vec![("func_a", 100.0)]);
        let report = diff_perf(&base, &current, 10.0);
        // func_b is missing in current — not counted as regression
        assert_eq!(report.unchanged, 1);
    }

    #[test]
    fn parse_baseline_json() {
        let json =
            r#"{"entries":{"test":{"mean_ms":10.0,"p95_ms":15.0,"p99_ms":20.0,"samples":50}}}"#;
        let baseline = parse_baseline(json).unwrap();
        assert_eq!(baseline.entries.len(), 1);
    }

    #[test]
    fn empty_baselines() {
        let base = PerfBaseline {
            entries: HashMap::new(),
        };
        let current = PerfBaseline {
            entries: HashMap::new(),
        };
        let report = diff_perf(&base, &current, 10.0);
        assert!(report.regressions.is_empty());
        assert_eq!(report.unchanged, 0);
    }

    #[test]
    fn regressions_sorted_by_severity() {
        let mut base_entries = HashMap::new();
        base_entries.insert(
            "slow".into(),
            PerfEntry {
                mean_ms: 100.0,
                p95_ms: 150.0,
                p99_ms: 200.0,
                samples: 100,
            },
        );
        base_entries.insert(
            "slower".into(),
            PerfEntry {
                mean_ms: 100.0,
                p95_ms: 150.0,
                p99_ms: 200.0,
                samples: 100,
            },
        );
        let base = PerfBaseline {
            entries: base_entries,
        };

        let mut cur_entries = HashMap::new();
        cur_entries.insert(
            "slow".into(),
            PerfEntry {
                mean_ms: 120.0,
                p95_ms: 180.0,
                p99_ms: 240.0,
                samples: 100,
            },
        );
        cur_entries.insert(
            "slower".into(),
            PerfEntry {
                mean_ms: 200.0,
                p95_ms: 300.0,
                p99_ms: 400.0,
                samples: 100,
            },
        );
        let current = PerfBaseline {
            entries: cur_entries,
        };

        let report = diff_perf(&base, &current, 10.0);
        assert!(report.regressions.len() >= 2);
        // "slower" should be first (100% change vs 20%)
        assert!(report.regressions[0].change_pct >= report.regressions[1].change_pct);
    }
}
