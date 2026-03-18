//! FOX — stochastic fuzzing control that adapts mutation and exploration
//! rates based on recent coverage progress.
//! Based on the FOX paper.

use apex_core::config::FoxConfig;

/// Adaptive fuzzing controller that adjusts mutation and exploration rates.
pub struct FoxController {
    pub mutation_rate: f64,
    pub exploration_rate: f64,
    /// Learning rate for rate adaptation.
    alpha: f64,
}

impl FoxController {
    /// Create a new controller with default parameters.
    pub fn new() -> Self {
        Self::with_config(&FoxConfig::default())
    }

    /// Create a new controller from explicit config.
    pub fn with_config(config: &FoxConfig) -> Self {
        FoxController {
            mutation_rate: config.mutation_rate,
            exploration_rate: config.exploration_rate,
            alpha: config.alpha,
        }
    }

    /// Adapt rates based on recent coverage progress.
    ///
    /// - `coverage_delta`: fraction of new coverage gained (0.0 = none, 1.0 = all new).
    /// - `iterations_since_new`: how many iterations since last new coverage.
    pub fn adapt(&mut self, coverage_delta: f64, iterations_since_new: u64) {
        let stall_pressure = (iterations_since_new as f64 / 100.0).min(1.0);
        let progress_pressure = coverage_delta.min(1.0);

        // On stall: increase exploration and mutation aggressiveness
        // On progress: decrease exploration (exploit current direction)
        self.exploration_rate += self.alpha * (stall_pressure - progress_pressure);
        self.exploration_rate = self.exploration_rate.clamp(0.0, 1.0);

        self.mutation_rate += self.alpha * stall_pressure * 0.5;
        self.mutation_rate -= self.alpha * progress_pressure * 0.3;
        self.mutation_rate = self.mutation_rate.clamp(0.01, 1.0);
    }

    /// Whether the next iteration should explore (random/diverse) vs exploit (targeted).
    pub fn should_explore(&self) -> bool {
        self.exploration_rate >= 0.5
    }
}

impl Default for FoxController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fox_default_rates() {
        let ctrl = FoxController::new();
        assert!((ctrl.mutation_rate - 0.5).abs() < f64::EPSILON);
        assert!((ctrl.exploration_rate - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn adapt_increases_exploration_on_stall() {
        let mut ctrl = FoxController::new();
        // No new coverage for many iterations => explore more
        ctrl.adapt(0.0, 100);
        assert!(ctrl.exploration_rate > 0.5);
    }

    #[test]
    fn adapt_increases_exploitation_on_progress() {
        let mut ctrl = FoxController::new();
        // High coverage delta => exploit more (reduce exploration)
        ctrl.adapt(0.5, 0);
        assert!(ctrl.exploration_rate < 0.5);
    }

    #[test]
    fn should_explore_respects_rate() {
        let mut ctrl = FoxController::new();
        ctrl.exploration_rate = 1.0;
        // With rate=1.0, should always explore
        assert!(ctrl.should_explore());

        ctrl.exploration_rate = 0.0;
        // With rate=0.0, should never explore
        assert!(!ctrl.should_explore());
    }

    #[test]
    fn rates_stay_in_bounds() {
        let mut ctrl = FoxController::new();
        // Extreme stall
        for _ in 0..100 {
            ctrl.adapt(0.0, 10000);
        }
        assert!(ctrl.exploration_rate <= 1.0);
        assert!(ctrl.mutation_rate >= 0.0);

        // Extreme progress
        for _ in 0..100 {
            ctrl.adapt(1.0, 0);
        }
        assert!(ctrl.exploration_rate >= 0.0);
        assert!(ctrl.mutation_rate <= 1.0);
    }

    #[test]
    fn mutation_rate_increases_on_stall() {
        let mut ctrl = FoxController::new();
        let initial = ctrl.mutation_rate;
        ctrl.adapt(0.0, 50);
        assert!(ctrl.mutation_rate >= initial);
    }
}
