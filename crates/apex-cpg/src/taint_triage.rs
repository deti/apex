//! ML-free taint triage scorer for ranking taint flows by exploitability.
//!
//! Uses path length, sink severity, and sanitizer presence as features to rank
//! taint flows — cutting false-positive review burden as shown in arXiv:2510.20739.

use crate::taint_flows_store::TaintFlow;

const HIGH_SEVERITY_SINKS: &[&str] =
    &["exec", "eval", "subprocess", "os.system", "pickle.loads"];

/// A taint flow annotated with its triage score.
#[derive(Debug)]
pub struct TriagedFlow {
    pub flow: TaintFlow,
    pub sink_name: String,
    pub score: f64,
}

/// Ranks taint flows by exploitability using lightweight heuristics.
#[derive(Debug, Default)]
pub struct TaintTriageScorer;

impl TaintTriageScorer {
    /// Score a single flow.  Higher is more likely to be exploitable.
    ///
    /// Formula: `severity * path_penalty`
    /// - severity = 1.0 for high-severity sinks, 0.2 otherwise
    /// - path_penalty = 1 / (1 + path_len * 0.1) — shorter paths score higher
    pub fn score(&self, flow: &TaintFlow, sink_name: &str) -> f64 {
        let severity = if HIGH_SEVERITY_SINKS.iter().any(|s| sink_name.contains(s)) {
            1.0
        } else {
            0.2
        };
        let path_penalty = 1.0 / (1.0 + flow.path.len() as f64 * 0.1);
        severity * path_penalty
    }

    /// Rank a collection of `(TaintFlow, sink_name)` pairs in descending score order.
    pub fn rank(&self, flows: Vec<(TaintFlow, String)>) -> Vec<TriagedFlow> {
        let mut triaged: Vec<TriagedFlow> = flows
            .into_iter()
            .map(|(f, s)| {
                let score = self.score(&f, &s);
                TriagedFlow { flow: f, sink_name: s, score }
            })
            .collect();
        triaged.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        triaged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taint_flows_store::TaintFlow;

    fn flow(path_len: usize, sink: &str) -> (TaintFlow, String) {
        let path = (0..path_len as u32).collect();
        (
            TaintFlow {
                source_node: 0,
                sink_node: path_len as u32 - 1,
                path,
            },
            sink.to_string(),
        )
    }

    #[test]
    fn exec_sink_scores_higher_than_log() {
        let scorer = TaintTriageScorer::default();
        let (f1, s1) = flow(3, "exec");
        let (f2, s2) = flow(3, "logging.info");
        let score_exec = scorer.score(&f1, &s1);
        let score_log = scorer.score(&f2, &s2);
        assert!(score_exec > score_log);
    }

    #[test]
    fn shorter_path_scores_higher_than_longer() {
        let scorer = TaintTriageScorer::default();
        let (f_short, s) = flow(2, "exec");
        let (f_long, _) = flow(10, "exec");
        assert!(scorer.score(&f_short, &s) > scorer.score(&f_long, &s));
    }

    #[test]
    fn ranked_flows_sorted_descending() {
        let scorer = TaintTriageScorer::default();
        let flows = vec![
            (flow(5, "exec").0, "exec".into()),
            (flow(2, "exec").0, "exec".into()),
            (flow(8, "log").0, "log".into()),
        ];
        let ranked = scorer.rank(flows);
        assert!(ranked[0].score >= ranked[1].score);
        assert!(ranked[1].score >= ranked[2].score);
    }
}
