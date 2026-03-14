use rand::RngCore;
use rand_distr::{Beta, Distribution};

/// Bayesian bandit: each seed arm has Beta(α, β) reward distribution.
pub struct ThompsonScheduler {
    arms: Vec<(Vec<u8>, f64, f64)>, // (data, alpha, beta)
}

impl ThompsonScheduler {
    pub fn new() -> Self { Self { arms: Vec::new() } }

    pub fn add_seed(&mut self, data: Vec<u8>) {
        self.arms.push((data, 1.0, 1.0)); // uniform Beta(1,1) prior
    }

    pub fn arm_count(&self) -> usize { self.arms.len() }

    /// Reward arm `idx` with `new_branches` discovered.
    pub fn reward(&mut self, idx: usize, new_branches: usize) {
        if let Some(arm) = self.arms.get_mut(idx) {
            arm.1 += new_branches as f64; // alpha += successes
        }
    }

    /// Record that seed `idx` produced no new coverage.
    pub fn penalize(&mut self, idx: usize) {
        if let Some(arm) = self.arms.get_mut(idx) {
            arm.2 += 1.0; // beta += 1
        }
    }

    /// Sample each arm from its Beta distribution; return arm with highest sample.
    pub fn select(&self, rng: &mut dyn RngCore) -> usize {
        self.arms.iter().enumerate().map(|(i, (_, a, b))| {
            let sample = Beta::new(*a, *b).ok()
                .map(|d| d.sample(rng))
                .unwrap_or(0.0);
            (i, sample)
        })
        .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(0)
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
        let mut rng = rand::thread_rng();
        let picks: Vec<usize> = (0..50).map(|_| ts.select(&mut rng)).collect();
        let count_0 = picks.iter().filter(|&&x| x == 0).count();
        assert!(count_0 > 10, "rewarded seed should be picked more often");
    }

    #[test]
    fn select_uniform_with_no_rewards() {
        let mut ts = ThompsonScheduler::new();
        for _ in 0..4 { ts.add_seed(b"x".to_vec()); }
        let mut rng = rand::thread_rng();
        let picks: Vec<usize> = (0..100).map(|_| ts.select(&mut rng)).collect();
        // All 4 arms should be selected at least once
        for i in 0..4 { assert!(picks.contains(&i)); }
    }
}
