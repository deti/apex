//! Per-branch seed archive for driller escalation.
//!
//! Maps each branch ID to the best seed that came closest to covering it.
//! When fuzzing escalates to concolic execution, the archive provides the
//! optimal starting seed — the one that reached the uncovered branch with
//! the fewest remaining hops.

use apex_core::types::{BranchId, ExecutionResult, InputSeed};
use std::collections::HashMap;

/// The best seed recorded for a specific branch target.
#[derive(Debug, Clone)]
pub struct ArchivedSeed {
    /// The input seed itself.
    pub seed: InputSeed,
    /// How many branches away from the target this seed got.
    /// 0 = seed directly covered the branch; lower is better.
    pub distance: u32,
    /// All branches covered when this seed was executed.
    pub covered_branches: Vec<BranchId>,
}

/// Maps each branch ID to the best seed that came closest to covering it.
///
/// Updated after every sandbox execution. Used by the driller to pick the
/// most promising starting point for concolic escalation.
#[derive(Debug, Default)]
pub struct SeedArchive {
    archive: HashMap<BranchId, ArchivedSeed>,
}

impl SeedArchive {
    /// Create an empty archive.
    pub fn new() -> Self {
        Self {
            archive: HashMap::new(),
        }
    }

    /// Update the archive after executing `seed` which produced `result`.
    ///
    /// For each branch covered by the result, this seed has distance 0.
    /// For each branch already tracked, we keep whichever seed has the
    /// smaller distance (best approximation of "closest").
    ///
    /// The seed must have `input` bytes available in the result; if
    /// `result.input` is `None` the update is a no-op.
    pub fn update(&mut self, seed: &InputSeed, result: &ExecutionResult) {
        if result.input.is_none() {
            return;
        }

        let covered = &result.new_branches;

        // For branches newly covered by this run, distance = 0.
        for branch in covered {
            let entry = self
                .archive
                .entry(branch.clone())
                .or_insert_with(|| ArchivedSeed {
                    seed: seed.clone(),
                    distance: u32::MAX,
                    covered_branches: covered.to_vec(),
                });
            // distance 0 is optimal — always replace if we can do better
            if entry.distance > 0 {
                entry.seed = seed.clone();
                entry.distance = 0;
                entry.covered_branches = covered.to_vec();
            }
        }

        // For branches we're tracking that weren't hit this run, compute a
        // proxy distance: how many tracked branches separate this execution
        // from the target. We use a simple heuristic: if none of the covered
        // branches overlap with what the target needs, we set distance =
        // max(existing, 1) so we don't overwrite a better seed.
        //
        // This lightweight approach avoids a full CFG traversal while still
        // letting the archive converge over time toward better seeds.
        let covered_set: std::collections::HashSet<&BranchId> = covered.iter().collect();
        for (target_branch, entry) in self.archive.iter_mut() {
            if covered_set.contains(target_branch) {
                // Already handled above (distance = 0).
                continue;
            }
            // Compute overlap count as a proximity proxy.
            // More overlap → this seed got "closer" to the target's neighbourhood.
            let overlap: u32 = entry
                .covered_branches
                .iter()
                .filter(|b| covered_set.contains(b))
                .count() as u32;

            // New candidate distance: if there's some overlap, distance =
            // max(1, existing_covered_count - overlap). Otherwise large value.
            let candidate_distance = if covered.is_empty() {
                u32::MAX
            } else if overlap > 0 {
                // Better proximity — fewer steps away
                let d = entry
                    .covered_branches
                    .len()
                    .saturating_sub(overlap as usize) as u32;
                d.max(1)
            } else {
                // No overlap — arbitrarily worse than 1, but respect existing
                entry.distance
            };

            if candidate_distance < entry.distance {
                entry.seed = seed.clone();
                entry.distance = candidate_distance;
                entry.covered_branches = covered.to_vec();
            }
        }
    }

    /// Return the best archived seed for `branch`, if any.
    ///
    /// "Best" means lowest `distance` — the seed that got closest to
    /// covering the target branch before the run ended.
    pub fn best_seed_for(&self, branch: &BranchId) -> Option<&ArchivedSeed> {
        self.archive.get(branch)
    }

    /// Return the best seeds for a set of branches, deduplicated and sorted
    /// by ascending distance (closest first).
    ///
    /// Seeds that cover multiple requested branches appear only once — the
    /// entry with the lowest distance wins.
    pub fn closest_seeds(&self, branches: &[BranchId]) -> Vec<&ArchivedSeed> {
        // Collect the best archived seed per requested branch, then sort.
        let mut candidates: Vec<&ArchivedSeed> = branches
            .iter()
            .filter_map(|b| self.archive.get(b))
            .collect();

        // Deduplicate by seed id (same seed may cover multiple targets).
        candidates.sort_by_key(|a| a.distance);
        candidates.dedup_by_key(|a| a.seed.id.0);

        candidates
    }

    /// Total number of branches tracked in the archive.
    pub fn len(&self) -> usize {
        self.archive.len()
    }

    /// True when the archive has no entries.
    pub fn is_empty(&self) -> bool {
        self.archive.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{BranchId, ExecutionStatus, InputSeed, SeedId, SeedOrigin};

    fn make_seed(data: &[u8]) -> InputSeed {
        InputSeed::new(data.to_vec(), SeedOrigin::Fuzzer)
    }

    fn make_result(input: Option<Vec<u8>>, new_branches: Vec<BranchId>) -> ExecutionResult {
        ExecutionResult {
            seed_id: SeedId::new(),
            status: ExecutionStatus::Pass,
            new_branches,
            trace: None,
            duration_ms: 1,
            stdout: String::new(),
            stderr: String::new(),
            input,
            resource_metrics: None,
        }
    }

    #[test]
    fn new_archive_is_empty() {
        let archive = SeedArchive::new();
        assert!(archive.is_empty());
        assert_eq!(archive.len(), 0);
    }

    #[test]
    fn update_with_no_input_is_noop() {
        let mut archive = SeedArchive::new();
        let seed = make_seed(b"test");
        let b = BranchId::new(1, 10, 0, 0);
        let result = make_result(None, vec![b]);
        archive.update(&seed, &result);
        assert!(archive.is_empty());
    }

    #[test]
    fn update_records_covered_branch_at_distance_zero() {
        let mut archive = SeedArchive::new();
        let seed = make_seed(b"hello");
        let b = BranchId::new(1, 10, 0, 0);
        let result = make_result(Some(b"hello".to_vec()), vec![b.clone()]);
        archive.update(&seed, &result);

        let entry = archive.best_seed_for(&b).expect("should have entry");
        assert_eq!(entry.distance, 0);
        assert_eq!(entry.seed.data.as_ref(), b"hello");
    }

    #[test]
    fn best_seed_for_returns_none_for_unknown_branch() {
        let archive = SeedArchive::new();
        let b = BranchId::new(99, 1, 0, 0);
        assert!(archive.best_seed_for(&b).is_none());
    }

    #[test]
    fn update_keeps_better_distance_zero_seed() {
        let mut archive = SeedArchive::new();
        let b = BranchId::new(1, 10, 0, 0);

        // First seed covers branch — distance 0
        let seed1 = make_seed(b"first");
        let result1 = make_result(Some(b"first".to_vec()), vec![b.clone()]);
        archive.update(&seed1, &result1);

        // Second seed also covers same branch — should keep first (already distance 0)
        let seed2 = make_seed(b"second");
        let result2 = make_result(Some(b"second".to_vec()), vec![b.clone()]);
        archive.update(&seed2, &result2);

        // Distance stays 0; either seed is valid (we keep first since 0 == 0)
        let entry = archive.best_seed_for(&b).unwrap();
        assert_eq!(entry.distance, 0);
    }

    #[test]
    fn covered_branches_field_reflects_execution() {
        let mut archive = SeedArchive::new();
        let b1 = BranchId::new(1, 1, 0, 0);
        let b2 = BranchId::new(1, 2, 0, 0);
        let seed = make_seed(b"data");
        let result = make_result(Some(b"data".to_vec()), vec![b1.clone(), b2.clone()]);
        archive.update(&seed, &result);

        let e1 = archive.best_seed_for(&b1).unwrap();
        assert_eq!(e1.covered_branches.len(), 2);
        assert!(e1.covered_branches.contains(&b1));
        assert!(e1.covered_branches.contains(&b2));
    }

    #[test]
    fn closest_seeds_empty_branches_returns_empty() {
        let archive = SeedArchive::new();
        let result = archive.closest_seeds(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn closest_seeds_returns_sorted_by_distance() {
        let mut archive = SeedArchive::new();
        let b1 = BranchId::new(1, 1, 0, 0);
        let b2 = BranchId::new(1, 2, 0, 0);
        let b3 = BranchId::new(1, 3, 0, 0);

        // seed_a covers b1 (distance 0)
        let seed_a = make_seed(b"aaa");
        archive.update(
            &seed_a,
            &make_result(Some(b"aaa".to_vec()), vec![b1.clone()]),
        );

        // seed_b covers b2 (distance 0)
        let seed_b = make_seed(b"bbb");
        archive.update(
            &seed_b,
            &make_result(Some(b"bbb".to_vec()), vec![b2.clone()]),
        );

        // b3 not covered — will have no archive entry
        let closest = archive.closest_seeds(&[b1.clone(), b2.clone(), b3.clone()]);
        // b3 has no entry → 2 results
        assert_eq!(closest.len(), 2);
        // Both at distance 0 — order may be either way but must be valid
        for entry in &closest {
            assert_eq!(entry.distance, 0);
        }
    }

    #[test]
    fn closest_seeds_deduplicates_same_seed() {
        let mut archive = SeedArchive::new();
        let b1 = BranchId::new(1, 1, 0, 0);
        let b2 = BranchId::new(1, 2, 0, 0);

        // One seed covers both branches
        let seed = make_seed(b"covers-both");
        archive.update(
            &seed,
            &make_result(Some(b"covers-both".to_vec()), vec![b1.clone(), b2.clone()]),
        );

        let closest = archive.closest_seeds(&[b1, b2]);
        // The same seed appears for both branches but dedup removes duplicates
        assert_eq!(closest.len(), 1);
    }

    #[test]
    fn archive_tracks_multiple_branches() {
        let mut archive = SeedArchive::new();
        for i in 0..5u32 {
            let b = BranchId::new(1, i, 0, 0);
            let data = vec![i as u8; 4];
            let seed = make_seed(&data);
            archive.update(&seed, &make_result(Some(data), vec![b]));
        }
        assert_eq!(archive.len(), 5);
    }
}
