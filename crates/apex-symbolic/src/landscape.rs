//! Fitness landscape analysis for adaptive strategy switching.
//! Based on arXiv:2502.00169 — detects deceptive landscapes where
//! gradient descent fails and suggests alternative strategies.

/// Strategy recommendation from landscape analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyHint {
    /// Fitness landscape is smooth — gradient descent will work.
    GradientUseful,
    /// Fitness landscape is deceptive — switch to random exploration.
    SwitchToRandom,
    /// Fitness landscape is flat — need a solver (SMT/LLM).
    NeedsSolver,
}

/// Analyzes fitness landscape from sampled (input, fitness) pairs.
pub struct LandscapeAnalyzer {
    samples: Vec<(Vec<u8>, f64)>,
}

impl LandscapeAnalyzer {
    pub fn new() -> Self {
        LandscapeAnalyzer {
            samples: Vec::new(),
        }
    }

    pub fn add_sample(&mut self, input: Vec<u8>, fitness: f64) {
        self.samples.push((input, fitness));
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Detect if the landscape is deceptive (gradient doesn't reliably lead to target).
    ///
    /// Measures the ratio of sign changes in consecutive fitness deltas.
    /// High ratio of sign changes = oscillating = deceptive.
    pub fn is_deceptive(&self) -> bool {
        if self.samples.len() < 4 {
            return false;
        }

        let deltas: Vec<f64> = self.samples.windows(2).map(|w| w[1].1 - w[0].1).collect();

        let sign_changes = deltas
            .windows(2)
            .filter(|w| (w[0] > 0.0) != (w[1] > 0.0) && w[0] != 0.0 && w[1] != 0.0)
            .count();

        let total_transitions = deltas.len().saturating_sub(1);
        if total_transitions == 0 {
            return false;
        }

        let ratio = sign_changes as f64 / total_transitions as f64;
        ratio > 0.5
    }

    /// Suggest a strategy based on landscape shape.
    pub fn suggest_strategy(&self) -> StrategyHint {
        if self.samples.len() < 3 {
            return StrategyHint::GradientUseful; // not enough data, try gradient
        }

        // Check for plateau (all fitness values within epsilon)
        let fitnesses: Vec<f64> = self.samples.iter().map(|(_, f)| *f).collect();
        let min = fitnesses.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = fitnesses.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        if (max - min).abs() < 0.01 {
            return StrategyHint::NeedsSolver;
        }

        if self.is_deceptive() {
            return StrategyHint::SwitchToRandom;
        }

        StrategyHint::GradientUseful
    }
}

impl Default for LandscapeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_hint_debug() {
        assert_eq!(
            format!("{:?}", StrategyHint::GradientUseful),
            "GradientUseful"
        );
        assert_eq!(
            format!("{:?}", StrategyHint::SwitchToRandom),
            "SwitchToRandom"
        );
        assert_eq!(format!("{:?}", StrategyHint::NeedsSolver), "NeedsSolver");
    }

    #[test]
    fn empty_analyzer_not_deceptive() {
        let analyzer = LandscapeAnalyzer::new();
        assert!(!analyzer.is_deceptive());
    }

    #[test]
    fn monotonic_improvement_not_deceptive() {
        let mut analyzer = LandscapeAnalyzer::new();
        analyzer.add_sample(vec![0], 1.0);
        analyzer.add_sample(vec![1], 0.8);
        analyzer.add_sample(vec![2], 0.5);
        analyzer.add_sample(vec![3], 0.2);
        analyzer.add_sample(vec![4], 0.0);
        assert!(!analyzer.is_deceptive());
    }

    #[test]
    fn oscillating_fitness_is_deceptive() {
        let mut analyzer = LandscapeAnalyzer::new();
        // Fitness goes up and down — gradient is misleading
        for i in 0..20 {
            let fitness = if i % 2 == 0 { 0.8 } else { 0.2 };
            analyzer.add_sample(vec![i as u8], fitness);
        }
        assert!(analyzer.is_deceptive());
    }

    #[test]
    fn suggest_gradient_when_monotonic() {
        let mut analyzer = LandscapeAnalyzer::new();
        analyzer.add_sample(vec![0], 1.0);
        analyzer.add_sample(vec![1], 0.5);
        analyzer.add_sample(vec![2], 0.1);
        assert_eq!(analyzer.suggest_strategy(), StrategyHint::GradientUseful);
    }

    #[test]
    fn suggest_random_when_deceptive() {
        let mut analyzer = LandscapeAnalyzer::new();
        for i in 0..20 {
            let fitness = if i % 2 == 0 { 0.9 } else { 0.1 };
            analyzer.add_sample(vec![i as u8], fitness);
        }
        assert_eq!(analyzer.suggest_strategy(), StrategyHint::SwitchToRandom);
    }

    #[test]
    fn suggest_solver_when_plateau() {
        let mut analyzer = LandscapeAnalyzer::new();
        // All samples have the same fitness — plateau
        for i in 0..10 {
            analyzer.add_sample(vec![i as u8], 0.5);
        }
        assert_eq!(analyzer.suggest_strategy(), StrategyHint::NeedsSolver);
    }

    #[test]
    fn add_sample_grows_collection() {
        let mut analyzer = LandscapeAnalyzer::new();
        assert_eq!(analyzer.sample_count(), 0);
        analyzer.add_sample(vec![1], 0.5);
        assert_eq!(analyzer.sample_count(), 1);
        analyzer.add_sample(vec![2], 0.3);
        assert_eq!(analyzer.sample_count(), 2);
    }
}
