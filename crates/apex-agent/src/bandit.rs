/// Thompson sampling bandit over synthesis strategies.
///
/// Based on T-Scheduler: learns which `PromptStrategy` variant performs best
/// on each `BranchDifficulty` class, converging to optimal routing without
/// fixed rules. Extends T-Scheduler from seeds to synthesis strategies.
use rand::RngCore;
use rand_distr::{Beta, Distribution};
use std::collections::HashMap;

pub struct StrategyBandit {
    arms: Vec<String>,
    alpha: HashMap<String, f64>,
    beta_val: HashMap<String, f64>,
}

impl StrategyBandit {
    pub fn new(strategies: Vec<String>) -> Self {
        let mut alpha = HashMap::new();
        let mut beta_val = HashMap::new();
        for s in &strategies {
            alpha.insert(s.clone(), 1.0);
            beta_val.insert(s.clone(), 1.0);
        }
        Self {
            arms: strategies,
            alpha,
            beta_val,
        }
    }

    pub fn strategy_count(&self) -> usize {
        self.arms.len()
    }

    pub fn reward(&mut self, strategy: &str, value: f64) {
        if let Some(a) = self.alpha.get_mut(strategy) {
            *a += value;
        }
    }

    pub fn penalize(&mut self, strategy: &str) {
        if let Some(b) = self.beta_val.get_mut(strategy) {
            *b += 1.0;
        }
    }

    pub fn select<'a>(&'a self, rng: &mut dyn RngCore) -> &'a str {
        self.arms
            .iter()
            .max_by(|a, b| {
                let sa = self.sample_arm(a, rng);
                let sb = self.sample_arm(b, rng);
                sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    fn sample_arm(&self, name: &str, rng: &mut dyn RngCore) -> f64 {
        let a = self.alpha.get(name).copied().unwrap_or(1.0);
        let b = self.beta_val.get(name).copied().unwrap_or(1.0);
        Beta::new(a, b).ok().map(|d| d.sample(rng)).unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_bandit_registers_strategies() {
        let bandit = StrategyBandit::new(vec!["coverup".into(), "telpa".into()]);
        assert_eq!(bandit.strategy_count(), 2);
    }

    #[test]
    fn reward_shifts_selection_probability() {
        let mut bandit = StrategyBandit::new(vec!["a".into(), "b".into()]);
        bandit.reward("b", 10.0);
        let mut rng = rand::rng();
        let picks: Vec<&str> = (0..30).map(|_| bandit.select(&mut rng)).collect();
        let b_count = picks.iter().filter(|&&s| s == "b").count();
        assert!(
            b_count > 5,
            "rewarded strategy should be picked more: {b_count}"
        );
    }

    #[test]
    fn unknown_strategy_reward_is_noop() {
        let mut bandit = StrategyBandit::new(vec!["a".into()]);
        bandit.reward("nonexistent", 100.0); // should not panic
    }
}
