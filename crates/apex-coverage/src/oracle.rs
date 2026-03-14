use crate::heuristic::BranchHeuristic;
use apex_core::types::{BranchId, BranchState, CoverageLevel, ExecutionResult, SeedId};
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct DeltaCoverage {
    pub newly_covered: Vec<BranchId>,
}

/// Thread-safe store of branch coverage state.
pub struct CoverageOracle {
    branches: DashMap<BranchId, BranchState>,
    /// Insertion-ordered list of branch IDs for stable bitmap indexing.
    branch_order: Mutex<Vec<BranchId>>,
    covered_count: AtomicUsize,
    total_count: AtomicUsize,
    level: Mutex<CoverageLevel>,
    heuristics: DashMap<BranchId, BranchHeuristic>,
}

impl CoverageOracle {
    pub fn new() -> Self {
        CoverageOracle {
            branches: DashMap::new(),
            branch_order: Mutex::new(Vec::new()),
            covered_count: AtomicUsize::new(0),
            total_count: AtomicUsize::new(0),
            level: Mutex::new(CoverageLevel::Branch),
            heuristics: DashMap::new(),
        }
    }

    /// Set the coverage analysis level.
    pub fn set_coverage_level(&self, level: CoverageLevel) {
        *self.level.lock().unwrap_or_else(|e| e.into_inner()) = level;
    }

    /// Get the current coverage analysis level.
    pub fn coverage_level(&self) -> CoverageLevel {
        *self.level.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Return MC/DC independence pairs for a given compound decision (file_id, line).
    ///
    /// An independence pair for condition `i` is a pair of BranchIds that:
    /// - Share the same file_id, line, col, and condition_index
    /// - Differ only in direction (true vs false)
    /// - One is covered and the other is not
    ///
    /// Only considers branches that have `condition_index.is_some()`.
    pub fn mcdc_independence_pairs(&self, file_id: u64, line: u32) -> Vec<(BranchId, BranchId)> {
        // Collect all MC/DC branches at (file_id, line)
        let mcdc_branches: Vec<(BranchId, BranchState)> = self
            .branches
            .iter()
            .filter(|r| {
                let b = r.key();
                b.file_id == file_id && b.line == line && b.condition_index.is_some()
            })
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect();

        let mut pairs = Vec::new();

        // Group by (col, condition_index) and find pairs differing in direction
        for i in 0..mcdc_branches.len() {
            for j in (i + 1)..mcdc_branches.len() {
                let (ref bi, ref si) = mcdc_branches[i];
                let (ref bj, ref sj) = mcdc_branches[j];

                // Same condition, different direction
                if bi.col == bj.col
                    && bi.condition_index == bj.condition_index
                    && bi.direction != bj.direction
                {
                    let i_covered = matches!(si, BranchState::Covered { .. });
                    let j_covered = matches!(sj, BranchState::Covered { .. });

                    // Independence pair: one covered, one not
                    if i_covered != j_covered {
                        pairs.push((bi.clone(), bj.clone()));
                    }
                }
            }
        }

        pairs
    }

    /// Register all known branches (e.g. from static analysis / instrumentation).
    pub fn register_branches(&self, ids: impl IntoIterator<Item = BranchId>) {
        let mut order = self.branch_order.lock().unwrap();
        for id in ids {
            self.branches.entry(id.clone()).or_insert_with(|| {
                self.total_count.fetch_add(1, Ordering::Relaxed);
                order.push(id);
                BranchState::Uncovered
            });
        }
    }

    /// Mark a branch as covered by `seed_id`. Returns true if this is a new coverage.
    pub fn mark_covered(&self, id: &BranchId, seed_id: SeedId) -> bool {
        let mut entry = self.branches.entry(id.clone()).or_insert_with(|| {
            self.total_count.fetch_add(1, Ordering::Relaxed);
            BranchState::Uncovered
        });
        match *entry {
            BranchState::Uncovered => {
                *entry = BranchState::Covered {
                    hit_count: 1,
                    first_seed_id: seed_id,
                };
                self.covered_count.fetch_add(1, Ordering::Relaxed);
                true
            }
            BranchState::Covered {
                ref mut hit_count, ..
            } => {
                *hit_count += 1;
                false
            }
            _ => false,
        }
    }

    /// Merge an AFL++ style coverage bitmap (one byte per edge, non-zero = hit).
    /// Branch IDs are resolved by index position in the insertion-ordered branch list,
    /// ensuring deterministic mapping regardless of DashMap iteration order.
    pub fn merge_bitmap(&self, bitmap: &[u8], seed_id: SeedId) -> DeltaCoverage {
        let mut delta = DeltaCoverage::default();
        let order = self.branch_order.lock().unwrap();
        for (idx, &byte) in bitmap.iter().enumerate() {
            if byte > 0 {
                if let Some(branch) = order.get(idx) {
                    if self.mark_covered(branch, seed_id) {
                        delta.newly_covered.push(branch.clone());
                    }
                }
            }
        }
        delta
    }

    /// Merge coverage data from an `ExecutionResult`.
    pub fn merge_from_result(&self, result: &ExecutionResult) -> DeltaCoverage {
        let mut delta = DeltaCoverage::default();
        for branch in &result.new_branches {
            if self.mark_covered(branch, result.seed_id) {
                delta.newly_covered.push(branch.clone());
            }
        }
        delta
    }

    /// Return all currently uncovered branches, sorted by file_id then line.
    pub fn uncovered_branches(&self) -> Vec<BranchId> {
        let mut uncovered: Vec<BranchId> = self
            .branches
            .iter()
            .filter(|r| matches!(*r.value(), BranchState::Uncovered))
            .map(|r| r.key().clone())
            .collect();
        uncovered.sort_by_key(|b| (b.file_id, b.line, b.col, b.direction));
        uncovered
    }

    pub fn coverage_percent(&self) -> f64 {
        let total = self.total_count.load(Ordering::Relaxed);
        if total == 0 {
            return 100.0;
        }
        let covered = self.covered_count.load(Ordering::Relaxed);
        (covered as f64 / total as f64) * 100.0
    }

    pub fn covered_count(&self) -> usize {
        self.covered_count.load(Ordering::Relaxed)
    }

    pub fn total_count(&self) -> usize {
        self.total_count.load(Ordering::Relaxed)
    }

    pub fn state_of(&self, id: &BranchId) -> Option<BranchState> {
        self.branches.get(id).map(|r| r.value().clone())
    }

    /// Record a branch heuristic, keeping only the best (highest score) per branch.
    pub fn record_heuristic(&self, h: BranchHeuristic) {
        let entry = self.heuristics.entry(h.branch_id.clone());
        entry
            .and_modify(|existing| {
                if h.score > existing.score {
                    *existing = h.clone();
                }
            })
            .or_insert(h);
    }

    /// Retrieve the full heuristic for a branch, if any has been recorded.
    pub fn heuristic_for(&self, branch: &BranchId) -> Option<BranchHeuristic> {
        self.heuristics.get(branch).map(|r| r.value().clone())
    }

    /// Return the best heuristic score for a branch, or 0.0 if unknown.
    pub fn best_heuristic(&self, branch: &BranchId) -> f64 {
        self.heuristics.get(branch).map(|r| r.score).unwrap_or(0.0)
    }

    /// Mark a branch as proven unreachable (Z3 unsat).
    pub fn mark_unreachable(&self, id: &BranchId) {
        if let Some(mut entry) = self.branches.get_mut(id) {
            if matches!(*entry, BranchState::Uncovered) {
                *entry = BranchState::Unreachable;
                // Treat as "covered" for percentage purposes.
                self.covered_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Suppress a branch (user-excluded via config).
    pub fn suppress(&self, id: &BranchId) {
        if let Some(mut entry) = self.branches.get_mut(id) {
            if matches!(*entry, BranchState::Uncovered) {
                *entry = BranchState::Suppressed;
                self.covered_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl Default for CoverageOracle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_branch(line: u32, dir: u8) -> BranchId {
        BranchId::new(1, line, 0, dir)
    }

    #[test]
    fn test_register_and_cover() {
        let oracle = CoverageOracle::new();
        let b1 = make_branch(10, 0);
        let b2 = make_branch(10, 1);
        oracle.register_branches([b1.clone(), b2.clone()]);
        assert_eq!(oracle.total_count(), 2);
        assert_eq!(oracle.covered_count(), 0);

        let seed = SeedId::new();
        assert!(oracle.mark_covered(&b1, seed));
        assert!(!oracle.mark_covered(&b1, seed)); // idempotent
        assert_eq!(oracle.covered_count(), 1);
        assert_eq!(oracle.coverage_percent(), 50.0);
    }

    #[test]
    fn test_uncovered_sorted() {
        let oracle = CoverageOracle::new();
        let branches: Vec<_> = (1u32..=5).map(|l| make_branch(l, 0)).collect();
        oracle.register_branches(branches.clone());
        oracle.mark_covered(&branches[2], SeedId::new());
        let uncov = oracle.uncovered_branches();
        assert_eq!(uncov.len(), 4);
        assert!(!uncov.contains(&branches[2]));
    }

    #[test]
    fn test_merge_bitmap() {
        let oracle = CoverageOracle::new();
        let bs: Vec<_> = (0u32..4).map(|l| make_branch(l, 0)).collect();
        oracle.register_branches(bs.clone());
        let bitmap = vec![1u8, 0, 1, 0];
        let seed = SeedId::new();
        let delta = oracle.merge_bitmap(&bitmap, seed);
        assert_eq!(delta.newly_covered.len(), 2);
    }

    #[test]
    fn test_merge_from_result() {
        let oracle = CoverageOracle::new();
        let b1 = make_branch(1, 0);
        let b2 = make_branch(2, 0);
        oracle.register_branches([b1.clone(), b2.clone()]);

        let result = ExecutionResult {
            seed_id: SeedId::new(),
            status: apex_core::types::ExecutionStatus::Pass,
            new_branches: vec![b1.clone()],
            trace: None,
            duration_ms: 10,
            stdout: String::new(),
            stderr: String::new(),
            input: None,
        };
        let delta = oracle.merge_from_result(&result);
        assert_eq!(delta.newly_covered.len(), 1);
        assert_eq!(oracle.covered_count(), 1);
    }

    #[test]
    fn test_mark_unreachable() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone()]);
        assert_eq!(oracle.covered_count(), 0);

        oracle.mark_unreachable(&b);
        assert_eq!(oracle.covered_count(), 1);
        assert!(matches!(
            oracle.state_of(&b),
            Some(BranchState::Unreachable)
        ));
        assert!(oracle.uncovered_branches().is_empty());
    }

    #[test]
    fn test_suppress() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone()]);

        oracle.suppress(&b);
        assert_eq!(oracle.covered_count(), 1);
        assert!(matches!(oracle.state_of(&b), Some(BranchState::Suppressed)));
    }

    #[test]
    fn test_empty_oracle_100_percent() {
        let oracle = CoverageOracle::new();
        assert_eq!(oracle.coverage_percent(), 100.0);
    }

    #[test]
    fn test_duplicate_register() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone(), b.clone()]);
        assert_eq!(oracle.total_count(), 1); // deduped
    }

    #[test]
    fn test_hit_count_increments() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone()]);
        let seed = SeedId::new();
        assert!(oracle.mark_covered(&b, seed)); // first hit
        assert!(!oracle.mark_covered(&b, seed)); // second hit
        assert!(!oracle.mark_covered(&b, seed)); // third hit
        match oracle.state_of(&b) {
            Some(BranchState::Covered { hit_count, .. }) => assert_eq!(hit_count, 3),
            other => panic!("expected Covered, got {other:?}"),
        }
    }

    #[test]
    fn test_mark_unreachable_on_nonexistent() {
        let oracle = CoverageOracle::new();
        let b = make_branch(99, 0);
        oracle.mark_unreachable(&b); // should not panic
        assert_eq!(oracle.covered_count(), 0);
    }

    #[test]
    fn test_suppress_on_nonexistent() {
        let oracle = CoverageOracle::new();
        let b = make_branch(99, 0);
        oracle.suppress(&b); // should not panic
        assert_eq!(oracle.covered_count(), 0);
    }

    #[test]
    fn test_mark_unreachable_on_already_covered() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, SeedId::new());
        oracle.mark_unreachable(&b); // should be no-op
        assert!(matches!(
            oracle.state_of(&b),
            Some(BranchState::Covered { .. })
        ));
        assert_eq!(oracle.covered_count(), 1); // not double-counted
    }

    #[test]
    fn test_suppress_on_already_covered() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, SeedId::new());
        oracle.suppress(&b); // should be no-op
        assert!(matches!(
            oracle.state_of(&b),
            Some(BranchState::Covered { .. })
        ));
    }

    #[test]
    fn test_state_of_returns_none_for_unknown() {
        let oracle = CoverageOracle::new();
        let b = make_branch(999, 0);
        assert!(oracle.state_of(&b).is_none());
    }

    #[test]
    fn test_mark_covered_auto_registers() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        // Not registered, but mark_covered should auto-register via or_insert_with
        assert!(oracle.mark_covered(&b, SeedId::new()));
        assert_eq!(oracle.total_count(), 1);
        assert_eq!(oracle.covered_count(), 1);
    }

    #[test]
    fn test_merge_bitmap_oversized() {
        let oracle = CoverageOracle::new();
        let bs: Vec<_> = (0u32..2).map(|l| make_branch(l, 0)).collect();
        oracle.register_branches(bs);
        // Bitmap larger than registered branches — extra entries ignored
        let bitmap = vec![1u8, 1, 1, 1, 1];
        let delta = oracle.merge_bitmap(&bitmap, SeedId::new());
        assert_eq!(delta.newly_covered.len(), 2);
        assert_eq!(oracle.covered_count(), 2);
    }

    #[test]
    fn test_merge_bitmap_all_zeros() {
        let oracle = CoverageOracle::new();
        let bs: Vec<_> = (0u32..3).map(|l| make_branch(l, 0)).collect();
        oracle.register_branches(bs);
        let bitmap = vec![0u8; 3];
        let delta = oracle.merge_bitmap(&bitmap, SeedId::new());
        assert!(delta.newly_covered.is_empty());
        assert_eq!(oracle.covered_count(), 0);
    }

    #[test]
    fn test_delta_coverage_default() {
        let delta = DeltaCoverage::default();
        assert!(delta.newly_covered.is_empty());
    }

    #[test]
    fn test_default_trait() {
        let oracle = CoverageOracle::default();
        assert_eq!(oracle.total_count(), 0);
        assert_eq!(oracle.covered_count(), 0);
    }

    #[test]
    fn test_full_coverage() {
        let oracle = CoverageOracle::new();
        let bs: Vec<_> = (0u32..5).map(|l| make_branch(l, 0)).collect();
        oracle.register_branches(bs.clone());
        let seed = SeedId::new();
        for b in &bs {
            oracle.mark_covered(b, seed);
        }
        assert_eq!(oracle.coverage_percent(), 100.0);
        assert!(oracle.uncovered_branches().is_empty());
    }

    #[test]
    fn mark_covered_on_unreachable_returns_false() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_unreachable(&b);
        // Attempting to mark_covered on Unreachable hits the `_ => false` arm
        assert!(!oracle.mark_covered(&b, SeedId::new()));
        assert!(matches!(
            oracle.state_of(&b),
            Some(BranchState::Unreachable)
        ));
    }

    #[test]
    fn mark_covered_on_suppressed_returns_false() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone()]);
        oracle.suppress(&b);
        // Attempting to mark_covered on Suppressed hits the `_ => false` arm
        assert!(!oracle.mark_covered(&b, SeedId::new()));
        assert!(matches!(oracle.state_of(&b), Some(BranchState::Suppressed)));
    }

    #[test]
    fn merge_from_result_no_new_coverage() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_covered(&b, SeedId::new());
        // Second merge of same branch → no new coverage
        let result = ExecutionResult {
            seed_id: SeedId::new(),
            status: apex_core::types::ExecutionStatus::Pass,
            new_branches: vec![b],
            trace: None,
            duration_ms: 5,
            stdout: String::new(),
            stderr: String::new(),
            input: None,
        };
        let delta = oracle.merge_from_result(&result);
        assert!(delta.newly_covered.is_empty());
    }

    #[test]
    fn merge_bitmap_already_covered_no_new() {
        let oracle = CoverageOracle::new();
        let bs: Vec<_> = (0u32..3).map(|l| make_branch(l, 0)).collect();
        oracle.register_branches(bs.clone());
        let seed1 = SeedId::new();
        oracle.mark_covered(&bs[0], seed1);
        oracle.mark_covered(&bs[1], seed1);
        // Bitmap marks all 3 as hit, but 2 are already covered
        let bitmap = vec![1u8, 1, 1];
        let delta = oracle.merge_bitmap(&bitmap, SeedId::new());
        assert_eq!(delta.newly_covered.len(), 1); // only the 3rd is new
    }

    #[test]
    fn test_suppress_on_unreachable_is_noop() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.register_branches([b.clone()]);
        oracle.mark_unreachable(&b);
        oracle.suppress(&b); // already Unreachable, not Uncovered
        assert!(matches!(
            oracle.state_of(&b),
            Some(BranchState::Unreachable)
        ));
        assert_eq!(oracle.covered_count(), 1); // not double-counted
    }

    // -----------------------------------------------------------------------
    // MC/DC tests
    // -----------------------------------------------------------------------

    #[test]
    fn mcdc_independence_pairs_simple_compound() {
        let oracle = CoverageOracle::new();
        let b_a_true = BranchId::new_mcdc(1, 10, 0, 0, Some(0));
        let b_a_false = BranchId::new_mcdc(1, 10, 0, 1, Some(0));
        let b_b_true = BranchId::new_mcdc(1, 10, 0, 0, Some(1));
        let b_b_false = BranchId::new_mcdc(1, 10, 0, 1, Some(1));
        oracle.register_branches([
            b_a_true.clone(),
            b_a_false.clone(),
            b_b_true.clone(),
            b_b_false.clone(),
        ]);
        let seed = SeedId::new();
        oracle.mark_covered(&b_a_true, seed);
        oracle.mark_covered(&b_b_true, seed);
        oracle.mark_covered(&b_a_false, seed);
        // condition 0: both directions covered — not an independence pair
        // condition 1: only true covered, false uncovered — independence pair
        let pairs = oracle.mcdc_independence_pairs(1, 10);
        assert!(!pairs.is_empty());
    }

    #[test]
    fn mcdc_independence_pairs_no_mcdc_branches() {
        let oracle = CoverageOracle::new();
        let b = BranchId::new(1, 10, 0, 0);
        oracle.register_branches([b]);
        let pairs = oracle.mcdc_independence_pairs(1, 10);
        assert!(pairs.is_empty());
    }

    #[test]
    fn coverage_level_filtering() {
        let oracle = CoverageOracle::new();
        oracle.set_coverage_level(CoverageLevel::Branch);
        assert_eq!(oracle.coverage_level(), CoverageLevel::Branch);
    }

    #[test]
    fn coverage_level_default_is_branch() {
        let oracle = CoverageOracle::new();
        assert_eq!(oracle.coverage_level(), CoverageLevel::Branch);
    }

    #[test]
    fn coverage_level_set_to_mcdc() {
        let oracle = CoverageOracle::new();
        oracle.set_coverage_level(CoverageLevel::Mcdc);
        assert_eq!(oracle.coverage_level(), CoverageLevel::Mcdc);
    }

    #[test]
    fn mcdc_independence_pairs_all_covered_is_empty() {
        let oracle = CoverageOracle::new();
        let b_true = BranchId::new_mcdc(1, 10, 0, 0, Some(0));
        let b_false = BranchId::new_mcdc(1, 10, 0, 1, Some(0));
        oracle.register_branches([b_true.clone(), b_false.clone()]);
        let seed = SeedId::new();
        oracle.mark_covered(&b_true, seed);
        oracle.mark_covered(&b_false, seed);
        // Both covered — no independence pair
        let pairs = oracle.mcdc_independence_pairs(1, 10);
        assert!(pairs.is_empty());
    }

    // Both directions registered but neither covered → no independence pair.
    // This exercises the BranchState::Covered match returning false on line 75
    // and the i_covered != j_covered check being false (both false).
    #[test]
    fn mcdc_independence_pairs_both_uncovered_no_pair() {
        let oracle = CoverageOracle::new();
        let b_true = BranchId::new_mcdc(1, 20, 0, 0, Some(0));
        let b_false = BranchId::new_mcdc(1, 20, 0, 1, Some(0));
        oracle.register_branches([b_true.clone(), b_false.clone()]);
        // Neither branch is covered — both BranchState::Uncovered
        // i_covered = false, j_covered = false → i_covered != j_covered is false → no pair
        let pairs = oracle.mcdc_independence_pairs(1, 20);
        assert!(
            pairs.is_empty(),
            "both-uncovered should produce no independence pair, got: {pairs:?}"
        );
    }

    #[test]
    fn mcdc_independence_pairs_wrong_file_id() {
        let oracle = CoverageOracle::new();
        let b_true = BranchId::new_mcdc(1, 10, 0, 0, Some(0));
        let b_false = BranchId::new_mcdc(1, 10, 0, 1, Some(0));
        oracle.register_branches([b_true.clone(), b_false.clone()]);
        oracle.mark_covered(&b_true, SeedId::new());
        // Query with wrong file_id
        let pairs = oracle.mcdc_independence_pairs(99, 10);
        assert!(pairs.is_empty());
    }

    // -----------------------------------------------------------------------
    // Heuristic tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_record_and_retrieve_heuristic() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        let h = BranchHeuristic {
            branch_id: b.clone(),
            score: 0.75,
            operand_a: Some(40),
            operand_b: Some(42),
        };
        oracle.record_heuristic(h);
        assert_eq!(oracle.best_heuristic(&b), 0.75);
    }

    #[test]
    fn test_heuristic_keeps_best() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.record_heuristic(BranchHeuristic {
            branch_id: b.clone(),
            score: 0.5,
            operand_a: None,
            operand_b: None,
        });
        oracle.record_heuristic(BranchHeuristic {
            branch_id: b.clone(),
            score: 0.9,
            operand_a: None,
            operand_b: None,
        });
        oracle.record_heuristic(BranchHeuristic {
            branch_id: b.clone(),
            score: 0.3,
            operand_a: None,
            operand_b: None,
        });
        assert_eq!(oracle.best_heuristic(&b), 0.9);
    }

    #[test]
    fn test_best_heuristic_unknown_branch() {
        let oracle = CoverageOracle::new();
        let b = make_branch(999, 0);
        assert_eq!(oracle.best_heuristic(&b), 0.0);
    }

    #[test]
    fn test_heuristic_for_returns_full_struct() {
        let oracle = CoverageOracle::new();
        let b = make_branch(1, 0);
        oracle.record_heuristic(BranchHeuristic {
            branch_id: b.clone(),
            score: 0.75,
            operand_a: Some(40),
            operand_b: Some(42),
        });
        let h = oracle.heuristic_for(&b).expect("should exist");
        assert_eq!(h.score, 0.75);
        assert_eq!(h.operand_a, Some(40));
        assert_eq!(h.operand_b, Some(42));
    }

    #[test]
    fn test_heuristic_for_unknown_returns_none() {
        let oracle = CoverageOracle::new();
        let b = make_branch(999, 0);
        assert!(oracle.heuristic_for(&b).is_none());
    }

    #[test]
    fn merge_bitmap_uses_stable_ordering() {
        // Create two oracles with the same branches in the same order.
        // Merge the same bitmap into both and verify identical results.
        let branches: Vec<_> = (0u32..8).map(|l| make_branch(l, 0)).collect();
        let bitmap = vec![1u8, 0, 1, 0, 1, 0, 1, 0];

        let mut results = Vec::new();
        for _ in 0..10 {
            let oracle = CoverageOracle::new();
            oracle.register_branches(branches.clone());
            let seed = SeedId::new();
            let delta = oracle.merge_bitmap(&bitmap, seed);
            results.push(delta.newly_covered);
        }
        // All runs must produce identical newly_covered lists.
        for r in &results[1..] {
            assert_eq!(
                &results[0], r,
                "merge_bitmap produced non-deterministic results"
            );
        }
        // Verify the correct branches were covered (indices 0, 2, 4, 6).
        assert_eq!(results[0].len(), 4);
        assert_eq!(results[0][0], branches[0]);
        assert_eq!(results[0][1], branches[2]);
        assert_eq!(results[0][2], branches[4]);
        assert_eq!(results[0][3], branches[6]);
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_coverage_percent_in_range(n in 1..1000usize, covered in 0..1000usize) {
            let oracle = CoverageOracle::new();
            let branches: Vec<_> = (0..n as u32).map(|l| make_branch(l, 0)).collect();
            oracle.register_branches(branches.clone());
            let to_cover = covered.min(n);
            let seed = SeedId::new();
            for b in branches.iter().take(to_cover) {
                oracle.mark_covered(b, seed);
            }
            let pct = oracle.coverage_percent();
            prop_assert!((0.0..=100.0).contains(&pct));
        }

        #[test]
        fn prop_mark_covered_idempotent(line in 0..1000u32) {
            let oracle = CoverageOracle::new();
            let b = make_branch(line, 0);
            oracle.register_branches([b.clone()]);
            let seed = SeedId::new();
            oracle.mark_covered(&b, seed);
            oracle.mark_covered(&b, seed);
            oracle.mark_covered(&b, seed);
            prop_assert_eq!(oracle.covered_count(), 1);
        }

        #[test]
        fn prop_register_idempotent(n in 1..100usize) {
            let oracle = CoverageOracle::new();
            let branches: Vec<_> = (0..n as u32).map(|l| make_branch(l, 0)).collect();
            oracle.register_branches(branches.clone());
            oracle.register_branches(branches.clone()); // duplicate
            prop_assert_eq!(oracle.total_count(), n);
        }
    }
}
