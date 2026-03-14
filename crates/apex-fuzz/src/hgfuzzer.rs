//! HGFuzzer — directed greybox fuzzing with hierarchical distance computation.
//! Based on the HGFuzzer paper.

use apex_core::types::BranchId;
use std::collections::HashMap;

/// Directed greybox fuzzer that assigns energy based on distance to target branches.
pub struct HGFuzzer {
    pub target_branches: Vec<BranchId>,
    distance_cache: HashMap<String, f64>,
}

impl HGFuzzer {
    pub fn new(target_branches: Vec<BranchId>) -> Self {
        HGFuzzer {
            target_branches,
            distance_cache: HashMap::new(),
        }
    }

    /// Set the distance from a branch to the nearest target.
    pub fn set_distance(&mut self, branch: &BranchId, distance: f64) {
        self.distance_cache.insert(branch_key(branch), distance);
    }

    /// Assign energy to a corpus entry based on its distance to the target.
    ///
    /// Energy = 1.0 / (1.0 + distance). At target (distance=0), energy=1.0.
    /// Unknown distance gets a default low energy of 0.1.
    pub fn assign_energy(&self, branch: &BranchId) -> f64 {
        match self.distance_cache.get(&branch_key(branch)) {
            Some(&distance) => 1.0 / (1.0 + distance),
            None => 0.1, // default low energy for unknown distance
        }
    }
}

fn branch_key(b: &BranchId) -> String {
    format!("{}:{}:{}:{}", b.file_id, b.line, b.col, b.direction)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_branch(line: u32) -> BranchId {
        BranchId::new(1, line, 0, 0)
    }

    #[test]
    fn hgfuzzer_creation() {
        let targets = vec![make_branch(42)];
        let hg = HGFuzzer::new(targets.clone());
        assert_eq!(hg.target_branches.len(), 1);
    }

    #[test]
    fn assign_energy_at_target() {
        let targets = vec![make_branch(42)];
        let mut hg = HGFuzzer::new(targets);
        hg.set_distance(&make_branch(42), 0.0);
        let energy = hg.assign_energy(&make_branch(42));
        assert!((energy - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn assign_energy_far_from_target() {
        let targets = vec![make_branch(42)];
        let mut hg = HGFuzzer::new(targets);
        hg.set_distance(&make_branch(100), 10.0);
        let energy = hg.assign_energy(&make_branch(100));
        assert!(energy < 1.0);
        assert!(energy > 0.0);
    }

    #[test]
    fn assign_energy_unknown_distance() {
        let targets = vec![make_branch(42)];
        let hg = HGFuzzer::new(targets);
        // No distance set => default energy
        let energy = hg.assign_energy(&make_branch(99));
        assert!((energy - 0.1).abs() < f64::EPSILON); // default low energy
    }

    #[test]
    fn closer_gets_more_energy() {
        let targets = vec![make_branch(42)];
        let mut hg = HGFuzzer::new(targets);
        hg.set_distance(&make_branch(10), 2.0);
        hg.set_distance(&make_branch(20), 8.0);
        let e_close = hg.assign_energy(&make_branch(10));
        let e_far = hg.assign_energy(&make_branch(20));
        assert!(e_close > e_far);
    }
}
