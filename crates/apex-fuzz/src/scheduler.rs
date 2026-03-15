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

    pub fn mutate_with_index(
        &mut self,
        input: &[u8],
        mutator_idx: usize,
        rng: &mut dyn RngCore,
    ) -> Vec<u8> {
        let idx = mutator_idx % self.mutators.len().max(1);
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

// ---------------------------------------------------------------------------
// PSO-based MOpt Adaptive Mutation Scheduler
// ---------------------------------------------------------------------------

/// Particle-swarm-optimization-based adaptive mutation scheduler.
///
/// Tracks per-operator efficiency (coverage finds / attempts) and uses PSO
/// velocity updates to shift probability mass toward productive operators.
pub struct PsoMOptScheduler {
    /// Current probability for each operator.
    pub operator_probs: Vec<f64>,
    /// Coverage gains attributed to each operator in this epoch.
    pub operator_finds: Vec<u64>,
    /// Total applications of each operator in this epoch.
    pub operator_attempts: Vec<u64>,
    /// PSO velocity per operator.
    velocities: Vec<f64>,
    /// Best local probabilities seen per operator.
    local_best: Vec<f64>,
    /// Global best probability distribution.
    global_best: Vec<f64>,
    /// Best efficiency achieved per operator (for local best tracking).
    local_best_score: Vec<f64>,
    /// Best overall efficiency sum (for global best tracking).
    global_best_score: f64,
}

impl PsoMOptScheduler {
    /// PSO inertia weight.
    const W: f64 = 0.7;
    /// PSO cognitive coefficient (local best attraction).
    const C1: f64 = 1.5;
    /// PSO social coefficient (global best attraction).
    const C2: f64 = 1.5;
    /// Minimum probability floor to prevent operator starvation.
    const PROB_MIN: f64 = 0.01;

    /// Create a new scheduler with `num_operators` operators at uniform probability.
    pub fn new(num_operators: usize) -> Self {
        let n = num_operators.max(1);
        let uniform = 1.0 / n as f64;
        PsoMOptScheduler {
            operator_probs: vec![uniform; n],
            operator_finds: vec![0; n],
            operator_attempts: vec![0; n],
            velocities: vec![0.0; n],
            local_best: vec![uniform; n],
            global_best: vec![uniform; n],
            local_best_score: vec![0.0; n],
            global_best_score: 0.0,
        }
    }

    /// Update probabilities using PSO velocity equations.
    ///
    /// For each operator:
    ///   efficiency = finds / (attempts + 1)
    ///   velocity = w * velocity + c1 * r1 * (local_best - current) + c2 * r2 * (global_best - current)
    ///   new_prob = current + velocity, clamped to [0.01, 1.0], then normalized
    pub fn update_probabilities(&mut self) {
        self.update_probabilities_with_rng(&mut rand::thread_rng());
    }

    /// Deterministic variant for testing — accepts an explicit RNG.
    pub fn update_probabilities_with_rng(&mut self, rng: &mut impl rand::Rng) {
        let n = self.operator_probs.len();

        // Compute efficiency for each operator.
        let efficiencies: Vec<f64> = (0..n)
            .map(|i| self.operator_finds[i] as f64 / (self.operator_attempts[i] as f64 + 1.0))
            .collect();

        // Compute an efficiency-proportional target distribution for PSO to
        // pull toward.  This is normalized so it sums to 1.
        let eff_total: f64 = efficiencies.iter().sum();
        let eff_target: Vec<f64> = if eff_total > 0.0 {
            efficiencies.iter().map(|e| e / eff_total).collect()
        } else {
            vec![1.0 / n as f64; n]
        };

        // Update local bests per operator.
        for i in 0..n {
            if efficiencies[i] > self.local_best_score[i] {
                self.local_best_score[i] = efficiencies[i];
                self.local_best[i] = eff_target[i];
            }
        }

        // Update global best.
        let total_efficiency: f64 = efficiencies.iter().sum();
        if total_efficiency > self.global_best_score {
            self.global_best_score = total_efficiency;
            self.global_best = eff_target;
        }

        // PSO velocity update and position update.
        for i in 0..n {
            let r1: f64 = rng.gen();
            let r2: f64 = rng.gen();
            self.velocities[i] = Self::W * self.velocities[i]
                + Self::C1 * r1 * (self.local_best[i] - self.operator_probs[i])
                + Self::C2 * r2 * (self.global_best[i] - self.operator_probs[i]);
            self.operator_probs[i] =
                (self.operator_probs[i] + self.velocities[i]).clamp(Self::PROB_MIN, 1.0);
        }

        // Normalize so probabilities sum to 1.
        let total: f64 = self.operator_probs.iter().sum();
        if total > 0.0 {
            for p in &mut self.operator_probs {
                *p /= total;
            }
        }
    }

    /// Select an operator via weighted random sampling.
    pub fn select_operator(&self, rng: &mut impl rand::Rng) -> usize {
        let total: f64 = self.operator_probs.iter().sum();
        let mut pick = rng.gen::<f64>() * total;
        for (i, &w) in self.operator_probs.iter().enumerate() {
            pick -= w;
            if pick <= 0.0 {
                return i;
            }
        }
        self.operator_probs.len() - 1
    }

    /// Record the result of applying operator `op_id`.
    pub fn record(&mut self, op_id: usize, found_new: bool) {
        if op_id >= self.operator_attempts.len() {
            return;
        }
        self.operator_attempts[op_id] += 1;
        if found_new {
            self.operator_finds[op_id] += 1;
        }
    }

    /// Read current probabilities.
    pub fn probabilities(&self) -> &[f64] {
        &self.operator_probs
    }

    /// Reset finds and attempts for a new epoch.
    pub fn reset_counts(&mut self) {
        for v in &mut self.operator_finds {
            *v = 0;
        }
        for v in &mut self.operator_attempts {
            *v = 0;
        }
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
    fn mutate_with_index_uses_specified_mutator() {
        let mut scheduler = make_scheduler(3);
        let mut rng = StdRng::seed_from_u64(0);
        let input = b"test";
        let _ = scheduler.mutate_with_index(input, 1, &mut rng);
        // Only mutator 1 should have an application
        assert_eq!(scheduler.stats[0].applications, 0);
        assert_eq!(scheduler.stats[1].applications, 1);
        assert_eq!(scheduler.stats[2].applications, 0);
    }

    #[test]
    fn mutate_with_index_wraps_around() {
        let mut scheduler = make_scheduler(3);
        let mut rng = StdRng::seed_from_u64(0);
        let input = b"test";
        let _ = scheduler.mutate_with_index(input, 5, &mut rng); // 5 % 3 = 2
        assert_eq!(scheduler.stats[2].applications, 1);
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

    // ==================================================================
    // Bug-hunting tests
    // ==================================================================

    /// BUG: mutate() on an empty scheduler panics.
    /// select() returns 0 for empty, but then mutate() indexes stats[0]
    /// and mutators[0] on empty vecs => index out of bounds.
    #[test]
    #[should_panic]
    fn bug_mutate_empty_scheduler_panics() {
        let mut scheduler = make_scheduler(0);
        let mut rng = StdRng::seed_from_u64(0);
        // This should ideally not panic, but it does because select()
        // returns 0 and then stats[0] is accessed on an empty vec.
        let _ = scheduler.mutate(b"data", &mut rng);
    }

    /// BUG: report_hit increments coverage_hits but yield_now uses the
    /// OLD coverage_hits/applications ratio. When applications==0,
    /// yield_now is 0.0 even though we just recorded a hit (coverage_hits=1).
    /// The hit is "lost" in the EMA calculation.
    #[test]
    fn bug_report_hit_with_zero_applications_ignores_hit() {
        let mut scheduler = make_scheduler(1);
        assert_eq!(scheduler.stats[0].applications, 0);
        assert_eq!(scheduler.stats[0].coverage_hits, 0);
        scheduler.report_hit(0);
        // coverage_hits was incremented to 1
        assert_eq!(scheduler.stats[0].coverage_hits, 1);
        // But yield_now = coverage_hits/applications = 1/0 => takes the else branch => 0.0
        // So EMA = 0.1*0.0 + 0.9*1.0 = 0.9
        // The hit didn't increase ema_yield at all -- it actually DECREASED it from 1.0 to 0.9.
        // This is a bug: recording a successful hit should not decrease the yield estimate.
        assert!(
            scheduler.stats[0].ema_yield < 1.0,
            "BUG CONFIRMED: report_hit with 0 applications decreased ema from 1.0 to {}",
            scheduler.stats[0].ema_yield
        );
    }

    /// BUG: report_hit doesn't increment applications, so the yield
    /// ratio (coverage_hits / applications) grows unboundedly after
    /// repeated hits without calls to mutate().
    /// After 10 hits with applications=1, yield = 10/1 = 10.0 (>1.0).
    #[test]
    fn bug_report_hit_yield_exceeds_one() {
        let mut scheduler = make_scheduler(1);
        scheduler.stats[0].applications = 1;
        // Call report_hit 10 times without incrementing applications
        for _ in 0..10 {
            scheduler.report_hit(0);
        }
        // coverage_hits = 10, applications = 1, yield = 10.0
        // EMA should be close to 10.0 after many updates with yield=10.0
        // A yield > 1.0 makes no probabilistic sense (>100% success rate)
        assert!(
            scheduler.stats[0].ema_yield > 1.0,
            "BUG CONFIRMED: ema_yield exceeded 1.0: {}",
            scheduler.stats[0].ema_yield
        );
    }

    /// Verify that the PsoMOptScheduler with 0 operators doesn't panic
    /// but creates a degenerate scheduler with 1 operator (max(1)).
    #[test]
    fn pso_zero_operators_creates_one() {
        let s = PsoMOptScheduler::new(0);
        // new() does num_operators.max(1), so we get 1 operator
        assert_eq!(s.operator_probs.len(), 1);
        assert!((s.operator_probs[0] - 1.0).abs() < 1e-9);
    }

    /// BUG: PsoMOptScheduler record() with out-of-bounds op_id silently
    /// drops the record. This means if the caller passes the wrong index,
    /// statistics are silently lost with no indication.
    #[test]
    fn pso_record_out_of_bounds_silently_drops() {
        let mut s = PsoMOptScheduler::new(3);
        s.record(99, true); // silently dropped
        // All attempts should still be 0
        for i in 0..3 {
            assert_eq!(s.operator_attempts[i], 0);
            assert_eq!(s.operator_finds[i], 0);
        }
    }

    /// PsoMOptScheduler: after many updates with zero activity across
    /// all operators, probabilities should remain valid (sum to 1.0,
    /// all positive).
    #[test]
    fn pso_update_with_zero_activity_stays_valid() {
        let mut s = PsoMOptScheduler::new(4);
        let mut rng = StdRng::seed_from_u64(42);
        // No records at all, just update 50 times
        for _ in 0..50 {
            s.update_probabilities_with_rng(&mut rng);
        }
        let sum: f64 = s.operator_probs.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "probs should sum to 1.0 after zero-activity updates, got {sum}"
        );
        for (i, &p) in s.operator_probs.iter().enumerate() {
            assert!(p > 0.0, "operator {i} prob should be > 0, got {p}");
            assert!(!p.is_nan(), "operator {i} prob is NaN");
        }
    }

    // ==================================================================
    // PsoMOptScheduler tests
    // ==================================================================

    #[test]
    fn mopt_uniform_initial_probabilities() {
        let s = PsoMOptScheduler::new(4);
        let expected = 1.0 / 4.0;
        for &p in s.probabilities() {
            assert!((p - expected).abs() < 1e-9, "expected {expected}, got {p}");
        }
    }

    #[test]
    fn mopt_probabilities_sum_to_one() {
        let mut s = PsoMOptScheduler::new(5);
        let mut rng = StdRng::seed_from_u64(77);
        // Record some activity and update.
        for i in 0..5 {
            for _ in 0..(i + 1) {
                s.record(i, i % 2 == 0);
            }
        }
        s.update_probabilities_with_rng(&mut rng);
        let sum: f64 = s.probabilities().iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "probabilities should sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn mopt_record_increments_counts() {
        let mut s = PsoMOptScheduler::new(3);
        s.record(0, true);
        s.record(0, false);
        s.record(1, true);
        assert_eq!(s.operator_attempts[0], 2);
        assert_eq!(s.operator_finds[0], 1);
        assert_eq!(s.operator_attempts[1], 1);
        assert_eq!(s.operator_finds[1], 1);
        assert_eq!(s.operator_attempts[2], 0);
        assert_eq!(s.operator_finds[2], 0);
    }

    #[test]
    fn mopt_reset_counts() {
        let mut s = PsoMOptScheduler::new(3);
        s.record(0, true);
        s.record(1, false);
        s.record(2, true);
        s.reset_counts();
        for i in 0..3 {
            assert_eq!(s.operator_finds[i], 0);
            assert_eq!(s.operator_attempts[i], 0);
        }
    }

    #[test]
    fn mopt_select_returns_valid_index() {
        let s = PsoMOptScheduler::new(7);
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..200 {
            let idx = s.select_operator(&mut rng);
            assert!(idx < 7, "index {idx} out of range");
        }
    }

    #[test]
    fn mopt_boosts_successful_operators() {
        let mut s = PsoMOptScheduler::new(3);
        let mut rng = StdRng::seed_from_u64(123);
        // Operator 0 is very successful, others are not.
        for _ in 0..100 {
            s.record(0, true);
        }
        for _ in 0..100 {
            s.record(1, false);
        }
        for _ in 0..100 {
            s.record(2, false);
        }
        // Run several PSO updates to let probabilities converge.
        for _ in 0..20 {
            s.update_probabilities_with_rng(&mut rng);
        }
        // Operator 0 should have the highest probability.
        assert!(
            s.operator_probs[0] > s.operator_probs[1],
            "op0 ({}) should be > op1 ({})",
            s.operator_probs[0],
            s.operator_probs[1]
        );
        assert!(
            s.operator_probs[0] > s.operator_probs[2],
            "op0 ({}) should be > op2 ({})",
            s.operator_probs[0],
            s.operator_probs[2]
        );
    }

    #[test]
    fn mopt_maintains_minimum_probability() {
        let mut s = PsoMOptScheduler::new(4);
        let mut rng = StdRng::seed_from_u64(999);
        // Only operator 0 finds anything; others get nothing.
        for _ in 0..200 {
            s.record(0, true);
        }
        for i in 1..4 {
            for _ in 0..200 {
                s.record(i, false);
            }
        }
        for _ in 0..50 {
            s.update_probabilities_with_rng(&mut rng);
        }
        // After normalization, check that no operator is starved.
        // The minimum raw value is PROB_MIN (0.01), but after normalization
        // the actual floor depends on the total. We check it stays > 0.
        for (i, &p) in s.operator_probs.iter().enumerate() {
            assert!(p > 0.0, "operator {i} probability should be > 0, got {p}");
        }
    }
}
