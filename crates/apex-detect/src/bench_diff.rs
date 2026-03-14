//! Benchmark Regression — compares benchmark results against baselines.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResult {
    pub name: String,
    pub mean_ns: f64,
    pub stddev_ns: f64,
    pub iterations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchBaseline {
    pub benchmarks: Vec<BenchResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchChange {
    pub name: String,
    pub baseline_ns: f64,
    pub current_ns: f64,
    pub change_pct: f64,
    pub regression: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchDiffReport {
    pub changes: Vec<BenchChange>,
    pub regression_count: usize,
    pub improvement_count: usize,
    pub unchanged_count: usize,
    pub threshold_pct: f64,
}

pub fn diff_benchmarks(
    baseline: &BenchBaseline,
    current: &BenchBaseline,
    threshold_pct: f64,
) -> BenchDiffReport {
    let base_map: HashMap<&str, f64> = baseline
        .benchmarks
        .iter()
        .map(|b| (b.name.as_str(), b.mean_ns))
        .collect();

    let mut changes = Vec::new();
    let mut regression_count = 0;
    let mut improvement_count = 0;
    let mut unchanged_count = 0;

    for bench in &current.benchmarks {
        if let Some(&base_ns) = base_map.get(bench.name.as_str()) {
            if base_ns == 0.0 {
                continue;
            }
            let change_pct = ((bench.mean_ns - base_ns) / base_ns) * 100.0;
            let regression = change_pct > threshold_pct;

            if regression {
                regression_count += 1;
            } else if change_pct < -threshold_pct {
                improvement_count += 1;
            } else {
                unchanged_count += 1;
            }

            changes.push(BenchChange {
                name: bench.name.clone(),
                baseline_ns: base_ns,
                current_ns: bench.mean_ns,
                change_pct,
                regression,
            });
        }
    }

    // Sort: regressions first, by severity
    changes.sort_by(|a, b| {
        b.regression.cmp(&a.regression).then(
            b.change_pct
                .partial_cmp(&a.change_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    BenchDiffReport {
        changes,
        regression_count,
        improvement_count,
        unchanged_count,
        threshold_pct,
    }
}

/// Parse criterion-style JSON output.
pub fn parse_criterion_baseline(json: &str) -> Option<BenchBaseline> {
    serde_json::from_str(json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_baseline(items: Vec<(&str, f64)>) -> BenchBaseline {
        BenchBaseline {
            benchmarks: items
                .into_iter()
                .map(|(name, ns)| BenchResult {
                    name: name.into(),
                    mean_ns: ns,
                    stddev_ns: ns * 0.1,
                    iterations: 1000,
                })
                .collect(),
        }
    }

    #[test]
    fn detects_regression() {
        let base = make_baseline(vec![("bench_a", 1000.0)]);
        let cur = make_baseline(vec![("bench_a", 1200.0)]); // 20% slower
        let r = diff_benchmarks(&base, &cur, 5.0);
        assert_eq!(r.regression_count, 1);
        assert!(r.changes[0].regression);
    }

    #[test]
    fn detects_improvement() {
        let base = make_baseline(vec![("bench_a", 1000.0)]);
        let cur = make_baseline(vec![("bench_a", 800.0)]); // 20% faster
        let r = diff_benchmarks(&base, &cur, 5.0);
        assert_eq!(r.improvement_count, 1);
        assert!(!r.changes[0].regression);
    }

    #[test]
    fn within_threshold_unchanged() {
        let base = make_baseline(vec![("bench_a", 1000.0)]);
        let cur = make_baseline(vec![("bench_a", 1030.0)]); // 3% — within 5%
        let r = diff_benchmarks(&base, &cur, 5.0);
        assert_eq!(r.unchanged_count, 1);
        assert_eq!(r.regression_count, 0);
    }

    #[test]
    fn empty_baselines() {
        let base = BenchBaseline {
            benchmarks: vec![],
        };
        let cur = BenchBaseline {
            benchmarks: vec![],
        };
        let r = diff_benchmarks(&base, &cur, 5.0);
        assert_eq!(r.changes.len(), 0);
    }

    #[test]
    fn new_benchmark_not_in_baseline_ignored() {
        let base = make_baseline(vec![]);
        let cur = make_baseline(vec![("new_bench", 500.0)]);
        let r = diff_benchmarks(&base, &cur, 5.0);
        assert_eq!(r.changes.len(), 0); // no baseline to compare
    }

    #[test]
    fn regressions_sorted_first() {
        let base = make_baseline(vec![("a", 1000.0), ("b", 1000.0)]);
        let cur = make_baseline(vec![("a", 800.0), ("b", 1500.0)]); // a improved, b regressed
        let r = diff_benchmarks(&base, &cur, 5.0);
        assert!(r.changes[0].regression); // b first (regression)
        assert!(!r.changes[1].regression); // a second (improvement)
    }
}
