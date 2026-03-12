//! Shared bitmap→BranchId mapping logic.
//!
//! Converts an AFL++-compatible coverage bitmap into a list of newly-covered
//! `BranchId`s by consulting the [`CoverageOracle`].

use apex_core::types::{BranchId, BranchState};
use apex_coverage::CoverageOracle;

/// Map an AFL++-compatible coverage bitmap to newly-covered BranchIds.
///
/// `bitmap[i] > 0` means the branch at `branch_index[i]` was hit.
/// Only branches currently `Uncovered` in the oracle are returned.
pub fn bitmap_to_new_branches(
    bitmap: &[u8],
    branch_index: &[BranchId],
    oracle: &CoverageOracle,
) -> Vec<BranchId> {
    bitmap
        .iter()
        .enumerate()
        .filter(|(_, &byte)| byte > 0)
        .filter_map(|(idx, _)| branch_index.get(idx))
        .filter(|b| matches!(oracle.state_of(b), Some(BranchState::Uncovered)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::SeedId;
    fn make_branch(line: u32, dir: u8) -> BranchId {
        BranchId::new(1, line, 0, dir)
    }

    #[test]
    fn empty_bitmap_returns_empty() {
        let oracle = CoverageOracle::new();
        let b0 = make_branch(1, 0);
        oracle.register_branches([b0.clone()]);
        let index = vec![b0];

        let result = bitmap_to_new_branches(&[], &index, &oracle);
        assert!(result.is_empty());
    }

    #[test]
    fn all_zero_bitmap_returns_empty() {
        let oracle = CoverageOracle::new();
        let b0 = make_branch(1, 0);
        let b1 = make_branch(2, 0);
        oracle.register_branches([b0.clone(), b1.clone()]);
        let index = vec![b0, b1];

        let bitmap = vec![0u8, 0u8];
        let result = bitmap_to_new_branches(&bitmap, &index, &oracle);
        assert!(result.is_empty());
    }

    #[test]
    fn short_bitmap_only_maps_available_entries() {
        let oracle = CoverageOracle::new();
        let b0 = make_branch(1, 0);
        let b1 = make_branch(2, 0);
        let b2 = make_branch(3, 0);
        oracle.register_branches([b0.clone(), b1.clone(), b2.clone()]);
        let index = vec![b0.clone(), b1, b2];

        // Bitmap shorter than index — only first entry matched.
        let bitmap = vec![1u8];
        let result = bitmap_to_new_branches(&bitmap, &index, &oracle);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], b0);
    }

    #[test]
    fn normal_case_filters_uncovered() {
        let oracle = CoverageOracle::new();
        let b0 = make_branch(1, 0);
        let b1 = make_branch(2, 0);
        let b2 = make_branch(3, 0);
        oracle.register_branches([b0.clone(), b1.clone(), b2.clone()]);

        // Mark b0 as already covered.
        oracle.mark_covered(&b0, SeedId::new());

        let index = vec![b0.clone(), b1.clone(), b2.clone()];
        // All three hit in bitmap, but b0 is already covered.
        let bitmap = vec![1, 1, 1];
        let result = bitmap_to_new_branches(&bitmap, &index, &oracle);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&b1));
        assert!(result.contains(&b2));
        assert!(!result.contains(&b0));
    }

    #[test]
    fn bitmap_longer_than_index_ignores_extra() {
        let oracle = CoverageOracle::new();
        let b0 = make_branch(1, 0);
        oracle.register_branches([b0.clone()]);
        let index = vec![b0.clone()];

        let bitmap = vec![1, 1, 1, 1, 1];
        let result = bitmap_to_new_branches(&bitmap, &index, &oracle);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], b0);
    }

    #[test]
    fn empty_index_returns_empty() {
        let oracle = CoverageOracle::new();
        let bitmap = vec![1, 1, 1];
        let result = bitmap_to_new_branches(&bitmap, &[], &oracle);
        assert!(result.is_empty());
    }

    #[test]
    fn unregistered_branch_in_oracle_returns_none_state() {
        // Branch in index but NOT registered in oracle — state_of returns None
        let oracle = CoverageOracle::new();
        let b0 = make_branch(1, 0);
        // Do NOT register b0 in oracle
        let index = vec![b0];
        let bitmap = vec![1u8];
        let result = bitmap_to_new_branches(&bitmap, &index, &oracle);
        // state_of returns None (not Some(Uncovered)), so branch is filtered out
        assert!(result.is_empty());
    }

    #[test]
    fn mixed_registered_and_unregistered_branches() {
        let oracle = CoverageOracle::new();
        let b0 = make_branch(1, 0);
        let b1 = make_branch(2, 0);
        // Only register b1
        oracle.register_branches([b1.clone()]);
        let index = vec![b0, b1.clone()];
        let bitmap = vec![1, 1];
        let result = bitmap_to_new_branches(&bitmap, &index, &oracle);
        // b0 not registered (None), b1 registered and uncovered
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], b1);
    }

    #[test]
    fn all_branches_already_covered_returns_empty() {
        let oracle = CoverageOracle::new();
        let b0 = make_branch(1, 0);
        let b1 = make_branch(2, 0);
        oracle.register_branches([b0.clone(), b1.clone()]);
        oracle.mark_covered(&b0, SeedId::new());
        oracle.mark_covered(&b1, SeedId::new());
        let index = vec![b0, b1];
        let bitmap = vec![1, 1];
        let result = bitmap_to_new_branches(&bitmap, &index, &oracle);
        assert!(result.is_empty());
    }

    #[test]
    fn high_hit_count_values_treated_as_hit() {
        let oracle = CoverageOracle::new();
        let b0 = make_branch(1, 0);
        oracle.register_branches([b0.clone()]);
        let index = vec![b0.clone()];
        // bitmap value 255 (max u8) still counts as hit
        let bitmap = vec![255u8];
        let result = bitmap_to_new_branches(&bitmap, &index, &oracle);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], b0);
    }

    #[test]
    fn both_empty_bitmap_and_index() {
        let oracle = CoverageOracle::new();
        let result = bitmap_to_new_branches(&[], &[], &oracle);
        assert!(result.is_empty());
    }
}
