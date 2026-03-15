use rand::RngCore;
use rand_distr::{Beta, Distribution};

/// Bayesian bandit: each seed arm has Beta(α, β) reward distribution.
#[derive(Default)]
pub struct ThompsonScheduler {
    arms: Vec<(Vec<u8>, f64, f64)>, // (data, alpha, beta)
}

impl ThompsonScheduler {
    pub fn new() -> Self {
        Self::default()
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
    pub fn penalize(&mut self, idx: usize) {
        if let Some(arm) = self.arms.get_mut(idx) {
            arm.2 += 1.0; // beta += 1
        }
    }

    /// Sample each arm from its Beta distribution; return arm with highest sample.
    pub fn select(&self, rng: &mut dyn RngCore) -> usize {
        self.arms
            .iter()
            .enumerate()
            .map(|(i, (_, a, b))| {
                let sample = Beta::new(*a, *b).ok().map(|d| d.sample(rng)).unwrap_or(0.0);
                (i, sample)
            })
            .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal))
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
        for _ in 0..4 {
            ts.add_seed(b"x".to_vec());
        }
        let mut rng = rand::thread_rng();
        let picks: Vec<usize> = (0..100).map(|_| ts.select(&mut rng)).collect();
        // All 4 arms should be selected at least once
        for i in 0..4 {
            assert!(picks.contains(&i));
        }
    }

    // ==================================================================
    // Bug-hunting tests
    // ==================================================================

    /// select() on empty scheduler returns 0 — an invalid index.
    /// Any subsequent seed_data(0) returns None, but if the caller
    /// trusts the index and uses it to index into their own data
    /// structures, they'll get an out-of-bounds panic.
    #[test]
    fn bug_select_empty_returns_invalid_index() {
        let ts = ThompsonScheduler::new();
        let mut rng = rand::thread_rng();
        let idx = ts.select(&mut rng);
        // Returns 0, but there are no arms -- 0 is not a valid index
        assert_eq!(idx, 0);
        assert_eq!(ts.arm_count(), 0);
        // seed_data(0) is None, confirming 0 is invalid
        assert!(ts.seed_data(idx).is_none());
    }

    /// BUG: reward() doesn't track failures (beta), and penalize()
    /// doesn't track successes (alpha). After heavy penalization,
    /// an arm's beta grows large, making its expected value tiny.
    /// But if we then reward it once, only alpha increases by 1,
    /// which barely changes the distribution. The arm is effectively
    /// "dead" even after a big reward.
    #[test]
    fn bug_reward_after_heavy_penalize_barely_helps() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"penalized".to_vec());
        ts.add_seed(b"neutral".to_vec());

        // Heavily penalize arm 0
        for _ in 0..100 {
            ts.penalize(0);
        }
        // Now reward arm 0 with 5 new branches
        ts.reward(0, 5);

        // arm 0: alpha=6, beta=101 => expected value ~0.056
        // arm 1: alpha=1, beta=1 => expected value = 0.5
        // Despite getting 5 new branches, arm 0 is still suppressed
        let (_, a0, b0) = &ts.arms[0];
        let (_, a1, b1) = &ts.arms[1];
        let ev0 = a0 / (a0 + b0);
        let ev1 = a1 / (a1 + b1);
        assert!(
            ev0 < ev1,
            "BUG CONFIRMED: arm 0 (rewarded 5 branches) has lower expected value ({ev0:.3}) than neutral arm ({ev1:.3})"
        );
    }

    /// reward() and penalize() silently ignore out-of-bounds indices.
    #[test]
    fn reward_penalize_out_of_bounds_no_panic() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"a".to_vec());
        ts.reward(999, 10); // should not panic
        ts.penalize(999); // should not panic
        // arm 0 should be unchanged
        assert_eq!(ts.arms[0].1, 1.0); // alpha
        assert_eq!(ts.arms[0].2, 1.0); // beta
    }

    /// BUG: reward with very large new_branches can push alpha to
    /// extreme values, potentially causing numerical issues in Beta sampling.
    #[test]
    fn bug_reward_extreme_value_does_not_panic() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"a".to_vec());
        ts.reward(0, usize::MAX);
        // alpha is now 1.0 + usize::MAX as f64 -- extremely large
        let mut rng = rand::thread_rng();
        // select should not panic (Beta with huge alpha, small beta)
        let idx = ts.select(&mut rng);
        assert_eq!(idx, 0);
    }

    /// seed_data returns correct data after add_seed.
    #[test]
    fn seed_data_returns_correct_content() {
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"first".to_vec());
        ts.add_seed(b"second".to_vec());
        assert_eq!(ts.seed_data(0), Some(b"first".as_slice()));
        assert_eq!(ts.seed_data(1), Some(b"second".as_slice()));
        assert_eq!(ts.seed_data(2), None);
    }

    /// After many penalizations, the arm's beta dominates and the
    /// sampling produces values near 0.0, so a different arm is
    /// almost always selected.
    #[test]
    fn penalized_arm_rarely_selected() {
        use rand::{rngs::StdRng, SeedableRng};
        let mut ts = ThompsonScheduler::new();
        ts.add_seed(b"bad".to_vec());
        ts.add_seed(b"good".to_vec());

        // Penalize arm 0 heavily, reward arm 1
        for _ in 0..50 {
            ts.penalize(0);
        }
        ts.reward(1, 10);

        let mut rng = StdRng::seed_from_u64(42);
        let mut count_0 = 0;
        for _ in 0..200 {
            if ts.select(&mut rng) == 0 {
                count_0 += 1;
            }
        }
        // Arm 0 should almost never be picked
        assert!(
            count_0 < 20,
            "penalized arm selected too often: {count_0}/200"
        );
    }
}
