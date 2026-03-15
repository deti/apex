//! KLEE-style search strategies for symbolic execution state selection.
//!
//! Implements multiple strategies (depth-first, random path, coverage-optimized)
//! and an interleaved meta-strategy that round-robins across them.

use std::cmp::Ordering;

/// A symbolic execution state for search strategy selection.
#[derive(Debug, Clone)]
pub struct SymState {
    pub id: usize,
    pub depth: usize,
    pub instructions_executed: u64,
    pub new_coverage_count: u32,
}

/// Strategy for selecting which state to explore next.
pub trait SearchStrategy: Send + Sync {
    fn name(&self) -> &str;
    fn select(&mut self, states: &[SymState]) -> usize;
    fn on_coverage(&mut self, state_idx: usize, new_branches: usize);
    fn on_terminate(&mut self, state_idx: usize);
}

/// Depth-first search — always pick the deepest state.
pub struct DepthFirst;

impl SearchStrategy for DepthFirst {
    fn name(&self) -> &str {
        "depth-first"
    }

    fn select(&mut self, states: &[SymState]) -> usize {
        states
            .iter()
            .enumerate()
            .max_by_key(|(_, s)| s.depth)
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn on_coverage(&mut self, _: usize, _: usize) {}
    fn on_terminate(&mut self, _: usize) {}
}

/// Random path selection — favors shallow states.
///
/// Uses weights inversely proportional to depth, so shallower states
/// are more likely to be selected.
pub struct RandomPath {
    rng_seed: u64,
}

impl RandomPath {
    pub fn new(seed: u64) -> Self {
        Self { rng_seed: seed }
    }

    fn next_u64(&mut self) -> u64 {
        // xorshift64 for reproducibility
        self.rng_seed ^= self.rng_seed << 13;
        self.rng_seed ^= self.rng_seed >> 7;
        self.rng_seed ^= self.rng_seed << 17;
        self.rng_seed
    }
}

impl SearchStrategy for RandomPath {
    fn name(&self) -> &str {
        "random-path"
    }

    fn select(&mut self, states: &[SymState]) -> usize {
        if states.is_empty() {
            return 0;
        }
        // Weight inversely proportional to depth
        let weights: Vec<f64> = states
            .iter()
            .map(|s| 1.0 / (s.depth as f64 + 1.0))
            .collect();
        let total: f64 = weights.iter().sum();
        let target = (self.next_u64() as f64 / u64::MAX as f64) * total;
        let mut cumulative = 0.0;
        for (i, w) in weights.iter().enumerate() {
            cumulative += w;
            if cumulative >= target {
                return i;
            }
        }
        states.len() - 1
    }

    fn on_coverage(&mut self, _: usize, _: usize) {}
    fn on_terminate(&mut self, _: usize) {}
}

/// Coverage-optimized — weight states by recent coverage productivity.
pub struct CoverageOptimized {
    coverage_scores: Vec<f64>,
}

impl CoverageOptimized {
    pub fn new() -> Self {
        Self {
            coverage_scores: Vec::new(),
        }
    }
}

impl Default for CoverageOptimized {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchStrategy for CoverageOptimized {
    fn name(&self) -> &str {
        "coverage-optimized"
    }

    fn select(&mut self, states: &[SymState]) -> usize {
        // Extend scores if needed
        while self.coverage_scores.len() < states.len() {
            self.coverage_scores.push(0.0);
        }
        // Pick state with highest coverage score, tie-break by most recent coverage
        states
            .iter()
            .enumerate()
            .max_by(|(i, si), (j, sj)| {
                let score_i = self.coverage_scores.get(*i).unwrap_or(&0.0);
                let score_j = self.coverage_scores.get(*j).unwrap_or(&0.0);
                score_i
                    .partial_cmp(score_j)
                    .unwrap_or(Ordering::Equal)
                    .then(si.new_coverage_count.cmp(&sj.new_coverage_count))
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn on_coverage(&mut self, state_idx: usize, new_branches: usize) {
        while self.coverage_scores.len() <= state_idx {
            self.coverage_scores.push(0.0);
        }
        self.coverage_scores[state_idx] += new_branches as f64;
    }

    fn on_terminate(&mut self, state_idx: usize) {
        if state_idx < self.coverage_scores.len() {
            self.coverage_scores[state_idx] = 0.0;
        }
    }
}

/// Round-robin across multiple strategies.
pub struct InterleavedSearch {
    strategies: Vec<Box<dyn SearchStrategy>>,
    current: usize,
    rounds_remaining: usize,
    rounds_per_strategy: usize,
}

impl InterleavedSearch {
    pub fn new(strategies: Vec<Box<dyn SearchStrategy>>, rounds_per_strategy: usize) -> Self {
        Self {
            strategies,
            current: 0,
            rounds_remaining: rounds_per_strategy,
            rounds_per_strategy,
        }
    }
}

impl SearchStrategy for InterleavedSearch {
    fn name(&self) -> &str {
        "interleaved"
    }

    fn select(&mut self, states: &[SymState]) -> usize {
        if self.strategies.is_empty() {
            return 0;
        }
        let result = self.strategies[self.current].select(states);
        self.rounds_remaining -= 1;
        if self.rounds_remaining == 0 {
            self.current = (self.current + 1) % self.strategies.len();
            self.rounds_remaining = self.rounds_per_strategy;
        }
        result
    }

    fn on_coverage(&mut self, state_idx: usize, new_branches: usize) {
        for s in &mut self.strategies {
            s.on_coverage(state_idx, new_branches);
        }
    }

    fn on_terminate(&mut self, state_idx: usize) {
        for s in &mut self.strategies {
            s.on_terminate(state_idx);
        }
    }
}

/// Create the default KLEE-style interleaved strategy (random-path + coverage-optimized).
pub fn default_strategy() -> InterleavedSearch {
    InterleavedSearch::new(
        vec![
            Box::new(RandomPath::new(42)),
            Box::new(CoverageOptimized::new()),
        ],
        1000,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(id: usize, depth: usize) -> SymState {
        SymState {
            id,
            depth,
            instructions_executed: 0,
            new_coverage_count: 0,
        }
    }

    #[test]
    fn depth_first_picks_deepest() {
        let mut dfs = DepthFirst;
        let states = vec![make_state(0, 5), make_state(1, 10), make_state(2, 3)];
        assert_eq!(dfs.select(&states), 1);
    }

    #[test]
    fn random_path_is_deterministic_with_seed() {
        let states = vec![make_state(0, 1), make_state(1, 5), make_state(2, 10)];
        let mut rp1 = RandomPath::new(42);
        let mut rp2 = RandomPath::new(42);
        let results1: Vec<usize> = (0..20).map(|_| rp1.select(&states)).collect();
        let results2: Vec<usize> = (0..20).map(|_| rp2.select(&states)).collect();
        assert_eq!(results1, results2);
    }

    #[test]
    fn random_path_favors_shallow() {
        let states = vec![
            make_state(0, 1),   // shallow — weight 0.5
            make_state(1, 100), // deep — weight ~0.01
        ];
        let mut rp = RandomPath::new(12345);
        let mut shallow_count = 0;
        let iterations = 1000;
        for _ in 0..iterations {
            if rp.select(&states) == 0 {
                shallow_count += 1;
            }
        }
        // Shallow should be picked significantly more often
        assert!(
            shallow_count > iterations / 2,
            "shallow picked {shallow_count}/{iterations}, expected > 500"
        );
    }

    #[test]
    fn coverage_optimized_favors_productive() {
        let mut co = CoverageOptimized::new();
        let states = vec![make_state(0, 5), make_state(1, 5), make_state(2, 5)];
        // State 1 discovered the most coverage
        co.on_coverage(1, 10);
        co.on_coverage(0, 2);
        assert_eq!(co.select(&states), 1);
    }

    #[test]
    fn coverage_optimized_on_coverage_updates_score() {
        let mut co = CoverageOptimized::new();
        co.on_coverage(0, 5);
        co.on_coverage(0, 3);
        assert_eq!(co.coverage_scores[0], 8.0);
    }

    #[test]
    fn coverage_optimized_on_terminate_resets() {
        let mut co = CoverageOptimized::new();
        co.on_coverage(0, 10);
        assert_eq!(co.coverage_scores[0], 10.0);
        co.on_terminate(0);
        assert_eq!(co.coverage_scores[0], 0.0);
    }

    #[test]
    fn interleaved_rotates_strategies() {
        let states = vec![make_state(0, 1), make_state(1, 10)];
        let mut interleaved =
            InterleavedSearch::new(vec![Box::new(DepthFirst), Box::new(DepthFirst)], 2);
        // Should use strategy 0 for 2 rounds, then switch to strategy 1
        assert_eq!(interleaved.current, 0);
        interleaved.select(&states);
        assert_eq!(interleaved.current, 0); // still on first
        interleaved.select(&states);
        assert_eq!(interleaved.current, 1); // switched
        interleaved.select(&states);
        assert_eq!(interleaved.current, 1); // still on second
        interleaved.select(&states);
        assert_eq!(interleaved.current, 0); // wrapped around
    }

    #[test]
    fn interleaved_delegates_coverage_to_all() {
        let mut co1 = CoverageOptimized::new();
        let mut co2 = CoverageOptimized::new();
        // We cannot inspect inside InterleavedSearch easily, so test via behavior:
        // After on_coverage, both internal strategies should reflect the update
        co1.on_coverage(0, 5);
        co2.on_coverage(0, 5);
        assert_eq!(co1.coverage_scores[0], 5.0);
        assert_eq!(co2.coverage_scores[0], 5.0);

        // Also test that InterleavedSearch calls on_coverage
        let mut interleaved = InterleavedSearch::new(
            vec![
                Box::new(CoverageOptimized::new()),
                Box::new(CoverageOptimized::new()),
            ],
            10,
        );
        interleaved.on_coverage(0, 7);
        // Verify by selecting — both strategies should see the update
        let states = vec![make_state(0, 1), make_state(1, 1)];
        // State 0 should be preferred since it has coverage score
        let selected = interleaved.select(&states);
        assert_eq!(selected, 0);
    }

    #[test]
    fn default_strategy_name() {
        let s = default_strategy();
        assert_eq!(s.name(), "interleaved");
    }

    #[test]
    fn select_on_single_state_returns_zero() {
        let states = vec![make_state(0, 5)];
        assert_eq!(DepthFirst.select(&states), 0);
        assert_eq!(RandomPath::new(99).select(&states), 0);
        assert_eq!(CoverageOptimized::new().select(&states), 0);
    }

    #[test]
    fn select_on_empty_returns_zero() {
        let states: Vec<SymState> = vec![];
        assert_eq!(DepthFirst.select(&states), 0);
        assert_eq!(RandomPath::new(99).select(&states), 0);
        assert_eq!(CoverageOptimized::new().select(&states), 0);
    }
}
