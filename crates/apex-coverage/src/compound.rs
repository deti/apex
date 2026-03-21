//! Compound coverage oracle — Bayesian log-odds signal combiner.
//!
//! Combines heterogeneous coverage signals (instrumented, imported, mutation,
//! concolic, static, AI) into a single confidence score per branch using
//! log-odds addition and sigmoid normalization.

use apex_core::types::BranchId;
use std::collections::HashMap;

/// A coverage signal from one of the available evidence sources.
#[derive(Debug, Clone, PartialEq)]
pub enum CoverageSignal {
    /// Branch was observed covered via runtime instrumentation.
    Instrumented(bool),
    /// Branch was marked covered in an imported coverage file.
    Imported(bool),
    /// A mutant injected at the branch was killed by tests.
    MutationKilled,
    /// A mutant injected at the branch survived testing.
    MutationSurvived,
    /// Concolic/symbolic solver found a feasible path to this branch.
    ConcolicReached,
    /// A test name heuristically matches the function containing this branch.
    StaticTestMatch,
    /// No test name matches the function containing this branch.
    StaticNoMatch,
}

impl CoverageSignal {
    /// Direction: +1.0 for signals indicating coverage, -1.0 for uncovered.
    fn direction(&self) -> f64 {
        match self {
            CoverageSignal::Instrumented(true) => 1.0,
            CoverageSignal::Instrumented(false) => -1.0,
            CoverageSignal::Imported(true) => 1.0,
            CoverageSignal::Imported(false) => -1.0,
            CoverageSignal::MutationKilled => 1.0,
            CoverageSignal::MutationSurvived => -1.0,
            CoverageSignal::ConcolicReached => 1.0,
            CoverageSignal::StaticTestMatch => 1.0,
            CoverageSignal::StaticNoMatch => -1.0,
        }
    }

    /// Default confidence tier for each signal type.
    fn default_confidence(&self) -> f64 {
        match self {
            CoverageSignal::Instrumented(_) => 1.0,
            CoverageSignal::Imported(_) => 0.95,
            CoverageSignal::MutationKilled | CoverageSignal::MutationSurvived => 0.85,
            CoverageSignal::ConcolicReached => 0.80,
            CoverageSignal::StaticTestMatch | CoverageSignal::StaticNoMatch => 0.50,
        }
    }
}

/// Bayesian log-odds coverage signal combiner.
///
/// Collects multiple heterogeneous signals per branch and combines them into
/// a single coverage confidence in \[0, 1\] via log-odds addition + sigmoid.
pub struct CompoundOracle {
    signals: HashMap<BranchId, Vec<(CoverageSignal, f64)>>,
}

impl CompoundOracle {
    pub fn new() -> Self {
        CompoundOracle {
            signals: HashMap::new(),
        }
    }

    /// Add a coverage signal for `branch` with the given `confidence` (0..1).
    /// Confidence is clamped to \[0.001, 0.999\] to avoid infinite log-odds.
    pub fn add_signal(&mut self, branch: BranchId, signal: CoverageSignal, confidence: f64) {
        let clamped = confidence.clamp(0.001, 0.999);
        self.signals
            .entry(branch)
            .or_default()
            .push((signal, clamped));
    }

    /// Add a signal using the default confidence tier for that signal type.
    pub fn add_default_signal(&mut self, branch: BranchId, signal: CoverageSignal) {
        let conf = signal.default_confidence();
        self.add_signal(branch, signal, conf);
    }

    /// Compute combined coverage confidence for a branch, in \[0, 1\].
    ///
    /// Uses log-odds combination: each signal contributes
    /// `direction * ln(conf / (1 - conf))`. The final value is `sigmoid(sum)`.
    ///
    /// Returns 0.5 (no evidence) if no signals exist for the branch.
    pub fn coverage_confidence(&self, branch: &BranchId) -> f64 {
        let Some(signals) = self.signals.get(branch) else {
            return 0.5;
        };
        if signals.is_empty() {
            return 0.5;
        }

        let log_odds_sum: f64 = signals
            .iter()
            .map(|(signal, conf)| {
                let direction = signal.direction();
                let odds = conf / (1.0 - conf);
                direction * odds.ln()
            })
            .sum();

        sigmoid(log_odds_sum)
    }

    /// Adjust a base severity score using coverage confidence.
    ///
    /// Formula: `base * (2.0 - coverage_confidence)`, capped at 10.0.
    ///
    /// Higher coverage confidence means lower severity (the branch is likely tested).
    /// Lower confidence means the severity stays high or increases.
    pub fn adjusted_severity(&self, branch: &BranchId, base_severity: f64) -> f64 {
        let conf = self.coverage_confidence(branch);
        let adjusted = base_severity * (2.0 - conf);
        adjusted.min(10.0)
    }
}

impl Default for CompoundOracle {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard sigmoid function: 1 / (1 + e^(-x)).
fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(line: u32) -> BranchId {
        BranchId::new(1, line, 0, 0)
    }

    #[test]
    fn single_instrumented_covered() {
        let mut oracle = CompoundOracle::new();
        let b = branch(1);
        oracle.add_default_signal(b.clone(), CoverageSignal::Instrumented(true));
        let conf = oracle.coverage_confidence(&b);
        // Instrumented(true) with conf=0.999 (clamped from 1.0) should be very high
        assert!(conf > 0.99, "expected high confidence, got {conf}");
    }

    #[test]
    fn single_instrumented_not_covered() {
        let mut oracle = CompoundOracle::new();
        let b = branch(2);
        oracle.add_default_signal(b.clone(), CoverageSignal::Instrumented(false));
        let conf = oracle.coverage_confidence(&b);
        // Instrumented(false) with conf=0.999 should yield very low confidence
        assert!(conf < 0.01, "expected low confidence, got {conf}");
    }

    #[test]
    fn multiple_agreeing_signals_increase_confidence() {
        let mut oracle = CompoundOracle::new();
        let b = branch(3);
        oracle.add_default_signal(b.clone(), CoverageSignal::Instrumented(true));
        oracle.add_default_signal(b.clone(), CoverageSignal::MutationKilled);
        oracle.add_default_signal(b.clone(), CoverageSignal::ConcolicReached);
        let conf = oracle.coverage_confidence(&b);
        assert!(conf > 0.999, "expected very high confidence, got {conf}");
    }

    #[test]
    fn contradictory_signals_moderate_confidence() {
        let mut oracle = CompoundOracle::new();
        let b = branch(4);
        // Static says covered, but mutation says NOT covered
        oracle.add_default_signal(b.clone(), CoverageSignal::StaticTestMatch);
        oracle.add_default_signal(b.clone(), CoverageSignal::MutationSurvived);
        let conf = oracle.coverage_confidence(&b);
        // Result should be somewhere between 0 and 1, not extreme
        assert!(
            conf > 0.05 && conf < 0.95,
            "expected moderate confidence from contradiction, got {conf}"
        );
    }

    #[test]
    fn empty_branch_returns_neutral() {
        let oracle = CompoundOracle::new();
        let b = branch(5);
        let conf = oracle.coverage_confidence(&b);
        assert!(
            (conf - 0.5).abs() < 1e-10,
            "expected 0.5 for no signals, got {conf}"
        );
    }

    #[test]
    fn severity_adjustment_high_confidence_lowers_severity() {
        let mut oracle = CompoundOracle::new();
        let b = branch(6);
        oracle.add_default_signal(b.clone(), CoverageSignal::Instrumented(true));
        let severity = oracle.adjusted_severity(&b, 8.0);
        // conf ~0.999 → severity ~8.0 * (2.0 - 0.999) = ~8.008
        assert!(severity < 8.1, "expected severity near base, got {severity}");
    }

    #[test]
    fn severity_adjustment_low_confidence_raises_severity() {
        let mut oracle = CompoundOracle::new();
        let b = branch(7);
        oracle.add_default_signal(b.clone(), CoverageSignal::Instrumented(false));
        let severity = oracle.adjusted_severity(&b, 6.0);
        // conf ~0.001 → severity ~6.0 * (2.0 - 0.001) = ~11.994 → capped at 10.0
        assert!(
            (severity - 10.0).abs() < 1e-10,
            "expected severity capped at 10.0, got {severity}"
        );
    }

    #[test]
    fn severity_capped_at_10() {
        let mut oracle = CompoundOracle::new();
        let b = branch(8);
        oracle.add_default_signal(b.clone(), CoverageSignal::StaticNoMatch);
        let severity = oracle.adjusted_severity(&b, 9.0);
        assert!(
            severity <= 10.0,
            "severity must be capped at 10.0, got {severity}"
        );
    }

    #[test]
    fn severity_no_signals_uses_neutral() {
        let oracle = CompoundOracle::new();
        let b = branch(9);
        // No signals → conf = 0.5 → severity = base * 1.5
        let severity = oracle.adjusted_severity(&b, 4.0);
        assert!(
            (severity - 6.0).abs() < 1e-10,
            "expected 6.0 with neutral confidence, got {severity}"
        );
    }

    #[test]
    fn custom_confidence_overrides_default() {
        let mut oracle = CompoundOracle::new();
        let b = branch(10);
        // Use very low custom confidence for an instrumented signal
        oracle.add_signal(b.clone(), CoverageSignal::Instrumented(true), 0.55);
        let conf = oracle.coverage_confidence(&b);
        // With confidence=0.55, direction=+1, log-odds = ln(0.55/0.45) ≈ 0.2
        // sigmoid(0.2) ≈ 0.55 — should be moderate, not extreme
        assert!(
            conf > 0.5 && conf < 0.7,
            "expected moderate confidence with low-confidence signal, got {conf}"
        );
    }

    #[test]
    fn confidence_clamped_to_valid_range() {
        let mut oracle = CompoundOracle::new();
        let b = branch(11);
        // Confidence of 0.0 should be clamped to 0.001
        oracle.add_signal(b.clone(), CoverageSignal::Instrumented(true), 0.0);
        let conf = oracle.coverage_confidence(&b);
        // Should still produce a finite result, not NaN/Inf
        assert!(conf.is_finite(), "expected finite confidence, got {conf}");

        let mut oracle2 = CompoundOracle::new();
        let b2 = branch(12);
        // Confidence of 1.0 should be clamped to 0.999
        oracle2.add_signal(b2.clone(), CoverageSignal::Instrumented(true), 1.0);
        let conf2 = oracle2.coverage_confidence(&b2);
        assert!(conf2.is_finite(), "expected finite confidence, got {conf2}");
    }

    #[test]
    fn imported_signal_high_confidence() {
        let mut oracle = CompoundOracle::new();
        let b = branch(13);
        oracle.add_default_signal(b.clone(), CoverageSignal::Imported(true));
        let conf = oracle.coverage_confidence(&b);
        // Imported(true) with conf=0.95 → high but not as extreme as instrumented
        assert!(conf > 0.9, "expected high confidence for imported, got {conf}");
    }

    #[test]
    fn concolic_and_static_combine() {
        let mut oracle = CompoundOracle::new();
        let b = branch(14);
        oracle.add_default_signal(b.clone(), CoverageSignal::ConcolicReached);
        oracle.add_default_signal(b.clone(), CoverageSignal::StaticTestMatch);
        let conf = oracle.coverage_confidence(&b);
        // Both positive signals — combined should be >= either alone
        let mut single = CompoundOracle::new();
        single.add_default_signal(b.clone(), CoverageSignal::ConcolicReached);
        let single_conf = single.coverage_confidence(&b);
        assert!(
            conf >= single_conf,
            "combined ({conf}) should be >= single signal ({single_conf})"
        );
        // And combined must still be above neutral
        assert!(conf > 0.5, "combined should be above neutral, got {conf}");
    }
}
