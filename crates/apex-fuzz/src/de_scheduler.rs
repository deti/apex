use rand::RngCore;

/// Differential Evolution mutator weight scheduler.
pub struct DeScheduler {
    weights: Vec<f64>,
    rewards: Vec<f64>,
}

impl DeScheduler {
    pub fn new(n_operators: usize) -> Self {
        let w = 1.0 / n_operators as f64;
        Self {
            weights: vec![w; n_operators],
            rewards: vec![0.0; n_operators],
        }
    }

    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// Record `reward` (e.g. new branches found) for operator `idx`.
    pub fn update_reward(&mut self, idx: usize, reward: f64) {
        if idx < self.rewards.len() {
            self.rewards[idx] += reward;
            self.recompute_weights();
        }
    }

    fn recompute_weights(&mut self) {
        let total: f64 = self.rewards.iter().sum();
        if total > 0.0 {
            for (w, r) in self.weights.iter_mut().zip(&self.rewards) {
                *w = r / total;
            }
        }
    }

    /// Weighted random selection of an operator index.
    pub fn select(&self, rng: &mut dyn RngCore) -> usize {
        let mut r = (rng.next_u64() as f64) / (u64::MAX as f64);
        for (i, &w) in self.weights.iter().enumerate() {
            r -= w;
            if r <= 0.0 {
                return i;
            }
        }
        self.weights.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_weights_uniform() {
        let de = DeScheduler::new(4);
        let w = de.weights();
        assert_eq!(w.len(), 4);
        // All weights equal initially
        assert!(w.iter().all(|&x| (x - w[0]).abs() < 1e-9));
    }

    #[test]
    fn update_increases_rewarded_weight() {
        let mut de = DeScheduler::new(3);
        let before = de.weights()[0];
        de.update_reward(0, 10.0);
        assert!(de.weights()[0] > before);
    }

    #[test]
    fn weights_sum_to_one_after_update() {
        let mut de = DeScheduler::new(4);
        de.update_reward(1, 5.0);
        let sum: f64 = de.weights().iter().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn select_returns_valid_index() {
        let de = DeScheduler::new(5);
        let mut rng = rand::rng();
        let idx = de.select(&mut rng);
        assert!(idx < 5);
    }
}
