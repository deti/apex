//! PerfFuzz multi-dimensional performance feedback.
//!
//! Instead of tracking "did we cover a new branch?" (binary), PerfFeedback tracks
//! "how many times was each edge executed?" (count). An input is interesting if it
//! causes any edge to execute more times than the current maximum for that edge.
//!
//! This is the PerfFuzz (ISSTA 2018) approach: each edge is an independent
//! maximization objective, producing inputs that exercise the most-frequently-executed
//! branches.

use apex_core::types::ResourceMetrics;
use std::collections::HashMap;

/// Multi-dimensional performance feedback tracker.
///
/// Maintains per-edge maximum execution counts and global resource maximums.
/// An input is "interesting" from a performance perspective if it exceeds
/// any current maximum.
pub struct PerfFeedback {
    /// Per-edge maximum execution count seen so far.
    max_edge_counts: HashMap<u64, u64>,
    /// Maximum total instruction count across all executions.
    max_total_count: u64,
    /// Maximum peak memory across all executions.
    max_peak_memory: u64,
    /// Maximum wall-clock time across all executions.
    max_wall_time: u64,
}

impl PerfFeedback {
    pub fn new() -> Self {
        PerfFeedback {
            max_edge_counts: HashMap::new(),
            max_total_count: 0,
            max_peak_memory: 0,
            max_wall_time: 0,
        }
    }

    /// Returns true if the given metrics exceed any current maximum, meaning
    /// this input is "interesting" from a performance perspective.
    ///
    /// Updates internal maximums when returning true.
    pub fn is_interesting(&mut self, metrics: &ResourceMetrics) -> bool {
        let mut dominated = false;

        // Check per-edge execution counts (PerfFuzz multi-dimensional feedback)
        for (&edge, &count) in &metrics.edge_counts {
            let max = self.max_edge_counts.entry(edge).or_insert(0);
            if count > *max {
                *max = count;
                dominated = true;
            }
        }

        // Check total instruction count
        if let Some(ic) = metrics.instruction_count {
            if ic > self.max_total_count {
                self.max_total_count = ic;
                dominated = true;
            }
        }

        // Check peak memory
        if let Some(mem) = metrics.peak_memory_bytes {
            if mem > self.max_peak_memory {
                self.max_peak_memory = mem;
                dominated = true;
            }
        }

        // Check wall-clock time
        if metrics.wall_time_ms > self.max_wall_time {
            self.max_wall_time = metrics.wall_time_ms;
            dominated = true;
        }

        dominated
    }

    /// Score an input's resource consumption relative to known maximums.
    ///
    /// Returns a value in [0.0, 1.0+] where higher means closer to (or exceeding)
    /// the worst-case known. Used for corpus energy assignment.
    pub fn score(&self, metrics: &ResourceMetrics) -> f64 {
        let mut scores = Vec::new();

        // Per-edge scores
        for (&edge, &count) in &metrics.edge_counts {
            if let Some(&max) = self.max_edge_counts.get(&edge) {
                if max > 0 {
                    scores.push(count as f64 / max as f64);
                }
            }
        }

        // Wall time score
        if self.max_wall_time > 0 {
            scores.push(metrics.wall_time_ms as f64 / self.max_wall_time as f64);
        }

        // Peak memory score
        if let Some(mem) = metrics.peak_memory_bytes {
            if self.max_peak_memory > 0 {
                scores.push(mem as f64 / self.max_peak_memory as f64);
            }
        }

        // Instruction count score
        if let Some(ic) = metrics.instruction_count {
            if self.max_total_count > 0 {
                scores.push(ic as f64 / self.max_total_count as f64);
            }
        }

        if scores.is_empty() {
            return 0.0;
        }

        // Return the maximum score across all dimensions (PerfFuzz uses max, not average)
        scores
            .into_iter()
            .fold(0.0_f64, |acc, s| acc.max(s))
    }

    /// Returns the edge with the highest maximum execution count.
    pub fn hottest_edge(&self) -> Option<(u64, u64)> {
        self.max_edge_counts
            .iter()
            .max_by_key(|(_, &count)| count)
            .map(|(&edge, &count)| (edge, count))
    }

    /// Returns the current maximum wall-clock time in milliseconds.
    pub fn max_wall_time_ms(&self) -> u64 {
        self.max_wall_time
    }

    /// Returns the current maximum peak memory in bytes.
    pub fn max_peak_memory_bytes(&self) -> u64 {
        self.max_peak_memory
    }

    /// Returns the number of edges being tracked.
    pub fn tracked_edges(&self) -> usize {
        self.max_edge_counts.len()
    }
}

impl Default for PerfFeedback {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_metrics(wall_time: u64, edges: &[(u64, u64)]) -> ResourceMetrics {
        ResourceMetrics {
            wall_time_ms: wall_time,
            edge_counts: edges.iter().cloned().collect(),
            ..Default::default()
        }
    }

    #[test]
    fn new_edge_count_above_max_is_interesting() {
        let mut fb = PerfFeedback::new();
        let m1 = make_metrics(10, &[(1, 100)]);
        assert!(fb.is_interesting(&m1)); // First input is always interesting

        let m2 = make_metrics(10, &[(1, 200)]);
        assert!(fb.is_interesting(&m2)); // Higher count → interesting

        let m3 = make_metrics(10, &[(1, 150)]);
        assert!(!fb.is_interesting(&m3)); // Below max → not interesting
    }

    #[test]
    fn new_edge_is_always_interesting() {
        let mut fb = PerfFeedback::new();
        let m1 = make_metrics(10, &[(1, 50)]);
        assert!(fb.is_interesting(&m1));

        // Different edge, even with low count
        let m2 = make_metrics(10, &[(2, 1)]);
        assert!(fb.is_interesting(&m2));
    }

    #[test]
    fn wall_time_increase_is_interesting() {
        let mut fb = PerfFeedback::new();
        let m1 = make_metrics(100, &[]);
        assert!(fb.is_interesting(&m1));

        let m2 = make_metrics(200, &[]);
        assert!(fb.is_interesting(&m2));

        let m3 = make_metrics(150, &[]);
        assert!(!fb.is_interesting(&m3));
    }

    #[test]
    fn memory_increase_is_interesting() {
        let mut fb = PerfFeedback::new();
        let mut m1 = make_metrics(10, &[]);
        m1.peak_memory_bytes = Some(1000);
        assert!(fb.is_interesting(&m1));

        let mut m2 = make_metrics(10, &[]);
        m2.peak_memory_bytes = Some(2000);
        assert!(fb.is_interesting(&m2));

        let mut m3 = make_metrics(10, &[]);
        m3.peak_memory_bytes = Some(1500);
        assert!(!fb.is_interesting(&m3));
    }

    #[test]
    fn score_increases_with_resource_consumption() {
        let mut fb = PerfFeedback::new();
        let m1 = make_metrics(100, &[(1, 100)]);
        fb.is_interesting(&m1);

        let low = make_metrics(50, &[(1, 50)]);
        let high = make_metrics(90, &[(1, 90)]);

        assert!(fb.score(&high) > fb.score(&low));
    }

    #[test]
    fn score_is_zero_with_no_data() {
        let fb = PerfFeedback::new();
        let m = make_metrics(0, &[]);
        assert_eq!(fb.score(&m), 0.0);
    }

    #[test]
    fn hottest_edge_returns_max() {
        let mut fb = PerfFeedback::new();
        fb.is_interesting(&make_metrics(10, &[(1, 100), (2, 500), (3, 200)]));
        let (edge, count) = fb.hottest_edge().unwrap();
        assert_eq!(edge, 2);
        assert_eq!(count, 500);
    }

    #[test]
    fn hottest_edge_none_when_empty() {
        let fb = PerfFeedback::new();
        assert!(fb.hottest_edge().is_none());
    }

    #[test]
    fn tracked_edges_count() {
        let mut fb = PerfFeedback::new();
        assert_eq!(fb.tracked_edges(), 0);
        fb.is_interesting(&make_metrics(10, &[(1, 10), (2, 20)]));
        assert_eq!(fb.tracked_edges(), 2);
        fb.is_interesting(&make_metrics(10, &[(3, 30)]));
        assert_eq!(fb.tracked_edges(), 3);
    }
}
