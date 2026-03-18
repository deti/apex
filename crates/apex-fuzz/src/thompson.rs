use rand::RngCore;
use rand_distr::{Beta, Distribution};

/// Bayesian bandit: each seed arm has Beta(α, β) reward distribution.
pub struct ThompsonScheduler {
    arms: Vec<(Vec<u8>, f64, f64)>, // (data, alpha, beta)
    /// Maximum value for the beta (failure) parameter — prevents permanent arm death.
    beta_cap: f64,
}

impl Default for ThompsonScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl ThompsonScheduler {
    /// Create a new scheduler with default beta cap (50.0).
    pub fn new() -> Self {
        Self::with_beta_cap(50.0)
    }

    /// Create a new scheduler with an explicit beta cap.
    pub fn with_beta_cap(beta_cap: f64) -> Self {
        ThompsonScheduler {
            arms: Vec::new(),
            beta_cap,
        }
    }

    pub fn add_seed(&mut self, data: Vec<u8>) {
        self.arms.push((data, 1.0, 1.0)); // uniform Beta(1,1) prior
    }

    pub fn arm_count(&self) -> usize {
        self.arms.len()
    }

    /// Reward arm `idx` with `new_branches` discovered.
    pub fn reward(&mut self, idx: usize, new_branches: usize) {
        if let Some(arm) = self.arms.get_mut(idx) {
            arm.1 += new_branches as f64; // alpha += successes
        }
    }

    /// Record that seed `idx` produced no new coverage.
    /// Beta is capped at `beta_cap` to prevent permanent arm death.
    pub fn penalize(&mut self, idx: usize) {
        if let Some(arm) = self.arms.get_mut(idx) {
            arm.2 = (arm.2 + 1.0).min(self.beta_cap);
        }
    }

    /// Sample each arm from its Beta distribution; return arm with highest sample.
    /// Returns `None` if there are no arms.
    pub fn select(&self, rng: &mut dyn RngCore) -> Option<usize> {
        if self.arms.is_empty() {
            return None;
        }
        self.arms
            .iter()
            .enumerate()
            .map(|(i, (_, a, b))| {
                let sample = Beta::new(*a, *b).ok().map(|d| d.sample(rng)).unwrap_or(0.0);
                (i, sample)
            })
            .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
    }

    pub fn seed_data(&self, idx: usize) -> Option<&[u8]> {
        self.arms.get(idx).map(|(d, _, _)| d.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_scheduler_has_no_arms() {
        let ts = ThompsonScheduler::new();
        assert_eq!(ts.arm_count(), 0);
    }

    #[test]
    fn add_seed_creates_arm() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"hello".to_vec());
        assert_eq!(ts.arm_count(), 1);
    }

    #[test]
    fn reward_increases_priority() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"a".to_vec());
        ts.add_seed(b"b".to_vec());
        ts.reward(0, 5); // 5 new branches from seed 0
                         // After many samples, seed 0 should dominate
        let mut rng = rand::rng();
        let picks: Vec<usize> = (0..50).map(|_| ts.select(&mut rng).unwrap()).collect();
        let count_0 = picks.iter().filter(|&&x| x == 0).count();
        assert!(count_0 > 10, "rewarded seed should be picked more often");
    }

    #[test]
    fn select_uniform_with_no_rewards() {
        let mut ts = ThompsonScheduler::new();
        for _ in 0..4 {
            ts.add_seed(b"x".to_vec());
        }
        let mut rng = rand::rng();
        let picks: Vec<usize> = (0..100).map(|_| ts.select(&mut rng).unwrap()).collect();
        for i in 0..4 {
            assert!(picks.contains(&i));
        }
    }

    #[test]
    fn select_empty_returns_none() {
        let ts = ThompsonScheduler::new();
        let mut rng = rand::rng();
        assert_eq!(ts.select(&mut rng), None);
        assert_eq!(ts.arm_count(), 0);
    }

    #[test]
    fn recovery_after_heavy_penalize_with_ceiling() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"penalized".to_vec());
        ts.add_seed(b"neutral".to_vec());
        for _ in 0..100 {
            ts.penalize(0);
        }
        assert!((ts.arms[0].2 - 50.0).abs() < f64::EPSILON);
        ts.reward(0, 50);
        let (_, a0, b0) = &ts.arms[0];
        let ev0 = a0 / (a0 + b0);
        assert!(ev0 > 0.5, "recovered arm ev should be > 0.5, got {ev0:.3}");
    }

    #[test]
    fn penalize_beta_ceiling() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"a".to_vec());
        for _ in 0..200 {
            ts.penalize(0);
        }
        assert!((ts.arms[0].2 - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reward_penalize_out_of_bounds_no_panic() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"a".to_vec());
        ts.reward(999, 10);
        ts.penalize(999);
        assert_eq!(ts.arms[0].1, 1.0);
        assert_eq!(ts.arms[0].2, 1.0);
    }

    #[test]
    fn reward_extreme_value_does_not_panic() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"a".to_vec());
        ts.reward(0, usize::MAX);
        let mut rng = rand::rng();
        assert_eq!(ts.select(&mut rng), Some(0));
    }

    #[test]
    fn seed_data_returns_correct_content() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"first".to_vec());
        ts.add_seed(b"second".to_vec());
        assert_eq!(ts.seed_data(0), Some(b"first".as_slice()));
        assert_eq!(ts.seed_data(1), Some(b"second".as_slice()));
        assert_eq!(ts.seed_data(2), None);
    }

    #[test]
    fn penalized_arm_rarely_selected() {
        use rand::{rngs::StdRng, SeedableRng};
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"bad".to_vec());
        ts.add_seed(b"good".to_vec());
        for _ in 0..50 {
            ts.penalize(0);
        }
        ts.reward(1, 10);
        let mut rng = StdRng::seed_from_u64(42);
        let mut count_0 = 0;
        for _ in 0..200 {
            if ts.select(&mut rng) == Some(0) {
                count_0 += 1;
            }
        }
        assert!(
            count_0 < 20,
            "penalized arm selected too often: {count_0}/200"
        );
    }
}
