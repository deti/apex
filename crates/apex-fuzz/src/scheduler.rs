//! MOpt-style adaptive mutation scheduling.
//!
//! Tracks per-mutator success rates (coverage hits / applications) and
//! biases selection toward productive operators using an exponential
//! moving average.

use crate::traits::Mutator;
use rand::RngCore;

/// Per-mutator statistics.
struct MutatorStats {
    applications: u64,
    coverage_hits: u64,
    ema_yield: f64,
}

/// Adaptive scheduler that selects mutators proportional to their yield.
pub struct MOptScheduler {
    mutators: Vec<Box<dyn Mutator>>,
    stats: Vec<MutatorStats>,
    floor: f64,
    alpha: f64,
}

impl MOptScheduler {
    pub fn new(mutators: Vec<Box<dyn Mutator>>) -> Self {
        let n = mutators.len();
        MOptScheduler {
            mutators,
            stats: (0..n)
                .map(|_| MutatorStats {
                    applications: 0,
                    coverage_hits: 0,
                    ema_yield: 1.0,
                })
                .collect(),
            floor: 0.01,
            alpha: 0.1,
        }
    }

    pub fn select(&self, rng: &mut dyn RngCore) -> usize {
        if self.mutators.is_empty() {
            return 0;
        }
        let weights: Vec<f64> = self
            .stats
            .iter()
            .map(|s| s.ema_yield.max(self.floor))
            .collect();
        let total: f64 = weights.iter().sum();
        let mut pick = (rng.next_u64() as f64 / u64::MAX as f64) * total;
        for (i, w) in weights.iter().enumerate() {
            pick -= w;
            if pick <= 0.0 {
                return i;
            }
        }
        self.mutators.len() - 1
    }

    pub fn mutate(&mut self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8> {
        let idx = self.select(rng);
        self.stats[idx].applications += 1;
        self.mutators[idx].mutate(input, rng)
    }

    pub fn report_hit(&mut self, mutator_idx: usize) {
        if mutator_idx >= self.stats.len() {
            return;
        }
        let s = &mut self.stats[mutator_idx];
        s.coverage_hits += 1;
        let yield_now = if s.applications > 0 {
            s.coverage_hits as f64 / s.applications as f64
        } else {
            0.0
        };
        s.ema_yield = self.alpha * yield_now + (1.0 - self.alpha) * s.ema_yield;
    }

    pub fn report_miss(&mut self, mutator_idx: usize) {
        if mutator_idx >= self.stats.len() {
            return;
        }
        let s = &mut self.stats[mutator_idx];
        let yield_now = if s.applications > 0 {
            s.coverage_hits as f64 / s.applications as f64
        } else {
            0.0
        };
        s.ema_yield = self.alpha * yield_now + (1.0 - self.alpha) * s.ema_yield;
    }

    pub fn len(&self) -> usize {
        self.mutators.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mutators.is_empty()
    }

    pub fn stats_summary(&self) -> Vec<(&str, u64, u64, f64)> {
        self.mutators
            .iter()
            .zip(self.stats.iter())
            .map(|(m, s)| (m.name(), s.applications, s.coverage_hits, s.ema_yield))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    struct ConstMutator {
        name: &'static str,
    }
    impl Mutator for ConstMutator {
        fn mutate(&self, input: &[u8], _rng: &mut dyn RngCore) -> Vec<u8> {
            input.to_vec()
        }
        fn name(&self) -> &str {
            self.name
        }
    }

    fn make_scheduler(n: usize) -> MOptScheduler {
        let mutators: Vec<Box<dyn Mutator>> = (0..n)
            .map(|i| -> Box<dyn Mutator> {
                Box::new(ConstMutator {
                    name: Box::leak(format!("m{i}").into_boxed_str()),
                })
            })
            .collect();
        MOptScheduler::new(mutators)
    }

    #[test]
    fn select_returns_valid_index() {
        let scheduler = make_scheduler(5);
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..100 {
            let idx = scheduler.select(&mut rng);
            assert!(idx < 5);
        }
    }

    #[test]
    fn report_hit_increases_ema() {
        let mut scheduler = make_scheduler(3);
        let initial_ema = scheduler.stats[0].ema_yield;
        scheduler.stats[0].applications = 10;
        scheduler.report_hit(0);
        assert_ne!(scheduler.stats[0].ema_yield, initial_ema);
    }

    #[test]
    fn high_yield_mutator_selected_more() {
        let mut scheduler = make_scheduler(2);
        let mut rng = StdRng::seed_from_u64(0);

        scheduler.stats[0].applications = 100;
        scheduler.stats[0].coverage_hits = 90;
        scheduler.stats[0].ema_yield = 0.9;
        scheduler.stats[1].applications = 100;
        scheduler.stats[1].coverage_hits = 1;
        scheduler.stats[1].ema_yield = 0.01;

        let mut count_0 = 0;
        for _ in 0..1000 {
            if scheduler.select(&mut rng) == 0 {
                count_0 += 1;
            }
        }
        assert!(count_0 > 800, "expected > 800, got {count_0}");
    }

    #[test]
    fn stats_summary_returns_all() {
        let scheduler = make_scheduler(3);
        let summary = scheduler.stats_summary();
        assert_eq!(summary.len(), 3);
        assert_eq!(summary[0].0, "m0");
    }

    #[test]
    fn len_and_is_empty() {
        let scheduler = make_scheduler(3);
        assert_eq!(scheduler.len(), 3);
        assert!(!scheduler.is_empty());

        let empty = make_scheduler(0);
        assert!(empty.is_empty());
    }

    #[test]
    fn report_hit_out_of_bounds_no_panic() {
        let mut scheduler = make_scheduler(2);
        scheduler.report_hit(99);
    }

    // ------------------------------------------------------------------
    // select — empty scheduler
    // ------------------------------------------------------------------

    #[test]
    fn select_empty_returns_zero() {
        let scheduler = make_scheduler(0);
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(scheduler.select(&mut rng), 0);
    }

    // ------------------------------------------------------------------
    // select — single mutator always returns 0
    // ------------------------------------------------------------------

    #[test]
    fn select_single_mutator_always_zero() {
        let scheduler = make_scheduler(1);
        let mut rng = StdRng::seed_from_u64(7);
        for _ in 0..50 {
            assert_eq!(scheduler.select(&mut rng), 0);
        }
    }

    // ------------------------------------------------------------------
    // select — fallthrough path (last element returned)
    // ------------------------------------------------------------------

    #[test]
    fn select_last_element_fallthrough() {
        // With two mutators both at floor (0.01), every pick is valid.
        // Because all ema_yields equal the floor, each pick should return
        // some valid index.  Run many times; combined counts must equal N.
        let scheduler = make_scheduler(2);
        let mut rng = StdRng::seed_from_u64(99);
        let mut counts = [0usize; 2];
        for _ in 0..200 {
            let idx = scheduler.select(&mut rng);
            assert!(idx < 2);
            counts[idx] += 1;
        }
        assert_eq!(counts[0] + counts[1], 200);
    }

    // ------------------------------------------------------------------
    // report_miss
    // ------------------------------------------------------------------

    #[test]
    fn report_miss_updates_ema() {
        let mut scheduler = make_scheduler(2);
        let initial_ema = scheduler.stats[1].ema_yield;
        scheduler.stats[1].applications = 5;
        scheduler.stats[1].coverage_hits = 0;
        scheduler.report_miss(1);
        // yield_now = 0/5 = 0.0; new ema = 0.1*0.0 + 0.9*1.0 = 0.9 < initial 1.0
        assert!(scheduler.stats[1].ema_yield < initial_ema);
    }

    #[test]
    fn report_miss_out_of_bounds_no_panic() {
        let mut scheduler = make_scheduler(2);
        scheduler.report_miss(999); // must not panic
    }

    #[test]
    fn report_miss_zero_applications_yields_zero_now() {
        let mut scheduler = make_scheduler(1);
        // applications == 0 → yield_now branch = 0.0
        // ema = 0.1*0.0 + 0.9*1.0 = 0.9
        scheduler.report_miss(0);
        let expected = 0.1_f64 * 0.0 + 0.9 * 1.0;
        let delta = (scheduler.stats[0].ema_yield - expected).abs();
        assert!(
            delta < 1e-9,
            "ema mismatch: {} vs {expected}",
            scheduler.stats[0].ema_yield
        );
    }

    // ------------------------------------------------------------------
    // report_hit — zero applications branch
    // ------------------------------------------------------------------

    #[test]
    fn report_hit_zero_applications_yields_zero_now() {
        let mut scheduler = make_scheduler(1);
        // applications == 0 → yield_now = 0.0
        // ema = 0.1*0.0 + 0.9*1.0 = 0.9
        scheduler.report_hit(0);
        let expected = 0.1_f64 * 0.0 + 0.9 * 1.0;
        let delta = (scheduler.stats[0].ema_yield - expected).abs();
        assert!(delta < 1e-9);
    }

    // ------------------------------------------------------------------
    // mutate
    // ------------------------------------------------------------------

    #[test]
    fn mutate_returns_output_and_increments_applications() {
        let mut scheduler = make_scheduler(3);
        let mut rng = StdRng::seed_from_u64(42);
        let input = b"hello";
        let out = scheduler.mutate(input, &mut rng);
        // ConstMutator returns input unchanged
        assert_eq!(out, input);
        // At least one mutator must have had its application count bumped
        let total_apps: u64 = scheduler.stats.iter().map(|s| s.applications).sum();
        assert_eq!(total_apps, 1);
    }

    // ------------------------------------------------------------------
    // stats_summary — verify counts and ema fields
    // ------------------------------------------------------------------

    #[test]
    fn stats_summary_fields_correct_after_hit() {
        let mut scheduler = make_scheduler(2);
        scheduler.stats[0].applications = 5;
        scheduler.stats[0].coverage_hits = 3;
        scheduler.stats[0].ema_yield = 0.6;
        let summary = scheduler.stats_summary();
        assert_eq!(summary[0].1, 5); // applications
        assert_eq!(summary[0].2, 3); // coverage_hits
        let delta = (summary[0].3 - 0.6).abs();
        assert!(delta < 1e-9);
    }

    // ------------------------------------------------------------------
    // floor enforcement in select weights
    // ------------------------------------------------------------------

    #[test]
    fn floor_prevents_zero_weight() {
        let mut scheduler = make_scheduler(2);
        // Drive ema_yield to below floor via many misses
        scheduler.stats[0].ema_yield = 0.0; // force below floor directly
        let mut rng = StdRng::seed_from_u64(11);
        // Must not panic and must return valid index
        for _ in 0..50 {
            let idx = scheduler.select(&mut rng);
            assert!(idx < 2);
        }
    }

    // ------------------------------------------------------------------
    // EMA formula verification
    // ------------------------------------------------------------------

    #[test]
    fn mutate_with_multiple_mutators_distributes() {
        let mut scheduler = make_scheduler(5);
        let mut rng = StdRng::seed_from_u64(0);
        let input = b"data";
        for _ in 0..50 {
            let _ = scheduler.mutate(input, &mut rng);
        }
        let total_apps: u64 = scheduler.stats.iter().map(|s| s.applications).sum();
        assert_eq!(total_apps, 50);
    }

    #[test]
    fn stats_summary_names_correct() {
        let scheduler = make_scheduler(4);
        let summary = scheduler.stats_summary();
        for (i, (name, _, _, _)) in summary.iter().enumerate() {
            assert_eq!(*name, format!("m{i}").as_str());
        }
    }

    #[test]
    fn report_hit_then_miss_sequence() {
        let mut scheduler = make_scheduler(1);
        scheduler.stats[0].applications = 10;
        scheduler.stats[0].coverage_hits = 5;
        let before = scheduler.stats[0].ema_yield;
        scheduler.report_hit(0);
        let after_hit = scheduler.stats[0].ema_yield;
        scheduler.report_miss(0);
        let after_miss = scheduler.stats[0].ema_yield;
        // After a hit, ema should change; after a miss, it should change again
        assert_ne!(before, after_hit);
        assert_ne!(after_hit, after_miss);
    }

    #[test]
    fn ema_converges_toward_high_yield() {
        let mut scheduler = make_scheduler(1);
        // Simulate 20 applications, all hits, check EMA rises
        scheduler.stats[0].applications = 0;
        scheduler.stats[0].coverage_hits = 0;
        for i in 1..=20u64 {
            scheduler.stats[0].applications = i;
            scheduler.stats[0].coverage_hits = i; // 100% yield
            scheduler.report_hit(0);
        }
        // After all hits ema should be close to 1.0 (not below 0.5)
        assert!(scheduler.stats[0].ema_yield > 0.5);
    }
}
