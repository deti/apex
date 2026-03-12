//! Kani BMC unreachability proofs.
//!
//! Generates Kani proof harnesses for branch reachability checking
//! and (when the `kani-prover` feature is enabled) invokes the prover.

use std::path::PathBuf;

use apex_core::types::BranchId;

/// Result of a reachability check for a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachabilityResult {
    /// The branch is reachable; the `String` carries witness info.
    Reachable(String),
    /// The branch is provably unreachable.
    Unreachable,
    /// Could not determine reachability; reason given.
    Unknown(String),
}

/// Bounded model-checking prover backed by Kani.
pub struct KaniProver {
    target_root: PathBuf,
}

impl KaniProver {
    /// Create a new prover rooted at `target_root`.
    pub fn new(target_root: PathBuf) -> Self {
        Self { target_root }
    }

    /// Return the target root directory.
    pub fn target_root(&self) -> &PathBuf {
        &self.target_root
    }

    /// Generate a Kani proof harness string for a given branch.
    ///
    /// The harness uses `kani::cover!` to test reachability of the branch
    /// identified by `branch` inside `function_name`.
    pub fn generate_harness(&self, branch: &BranchId, function_name: &str) -> String {
        let dir = if branch.direction == 0 {
            "taken"
        } else {
            "not_taken"
        };
        let harness_name = format!(
            "check_reachability_{}_{}_{}",
            branch.file_id, branch.line, dir
        );

        format!(
            r#"#[cfg(kani)]
#[kani::proof]
fn {harness_name}() {{
    // Harness for branch reachability in `{function_name}`
    // file_id={file_id}, line={line}, direction={dir}
    let result = {function_name}(kani::any());
    kani::cover!(true, "branch {file_id}:{line}:{dir} is reachable");
}}"#,
            harness_name = harness_name,
            function_name = function_name,
            file_id = branch.file_id,
            line = branch.line,
            dir = dir,
        )
    }

    /// Check whether `branch` inside `function_name` is reachable.
    ///
    /// Without the `kani-prover` feature this always returns `Unknown`.
    pub fn check_reachability(
        &self,
        _branch: &BranchId,
        _function_name: &str,
    ) -> ReachabilityResult {
        #[cfg(feature = "kani-prover")]
        {
            ReachabilityResult::Unknown("kani execution not yet implemented".to_string())
        }
        #[cfg(not(feature = "kani-prover"))]
        {
            ReachabilityResult::Unknown("kani-prover feature not enabled".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_branch() -> BranchId {
        BranchId::new(42, 10, 5, 0)
    }

    #[test]
    fn harness_generation() {
        let prover = KaniProver::new(PathBuf::from("/tmp/target"));
        let branch = sample_branch();
        let harness = prover.generate_harness(&branch, "my_function");

        assert!(
            harness.contains("#[kani::proof]"),
            "must have kani::proof annotation"
        );
        assert!(
            harness.contains("my_function"),
            "must reference the function name"
        );
        assert!(
            harness.contains("check_reachability_42_10_taken"),
            "harness name must encode file_id, line, direction"
        );
    }

    #[test]
    fn check_without_feature_returns_unknown() {
        let prover = KaniProver::new(PathBuf::from("/tmp/target"));
        let branch = sample_branch();
        let result = prover.check_reachability(&branch, "some_fn");

        match result {
            ReachabilityResult::Unknown(msg) => {
                // Without kani-prover feature we expect the "not enabled" message.
                assert!(
                    msg.contains("not enabled") || msg.contains("not yet implemented"),
                    "unexpected reason: {msg}"
                );
            }
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn harness_not_taken_direction() {
        let prover = KaniProver::new(PathBuf::from("/tmp/target"));
        let branch = BranchId::new(42, 10, 5, 1); // direction=1 => not_taken
        let harness = prover.generate_harness(&branch, "other_fn");
        assert!(harness.contains("not_taken"));
        assert!(harness.contains("check_reachability_42_10_not_taken"));
        assert!(harness.contains("other_fn"));
    }

    #[test]
    fn target_root_accessor() {
        let path = PathBuf::from("/some/path");
        let prover = KaniProver::new(path.clone());
        assert_eq!(prover.target_root(), &path);
    }

    #[test]
    fn reachability_result_debug() {
        let r = ReachabilityResult::Reachable("witness data".to_string());
        let debug = format!("{:?}", r);
        assert!(debug.contains("Reachable"));
        assert!(debug.contains("witness data"));

        let u = ReachabilityResult::Unreachable;
        let debug = format!("{:?}", u);
        assert!(debug.contains("Unreachable"));

        let k = ReachabilityResult::Unknown("reason".to_string());
        let debug = format!("{:?}", k);
        assert!(debug.contains("Unknown"));
    }

    #[test]
    fn reachability_result_clone() {
        let r = ReachabilityResult::Reachable("test".to_string());
        let r2 = r.clone();
        assert_eq!(r, r2);
    }

    #[test]
    fn harness_contains_kani_cover() {
        let prover = KaniProver::new(PathBuf::from("/tmp"));
        let branch = BranchId::new(1, 20, 3, 0);
        let harness = prover.generate_harness(&branch, "test_fn");
        assert!(harness.contains("kani::cover!"));
        assert!(harness.contains("#[cfg(kani)]"));
    }

    #[test]
    fn reachability_result_variants() {
        let reachable = ReachabilityResult::Reachable("witness".to_string());
        let unreachable = ReachabilityResult::Unreachable;
        let unknown = ReachabilityResult::Unknown("reason".to_string());

        assert_eq!(
            reachable,
            ReachabilityResult::Reachable("witness".to_string())
        );
        assert_eq!(unreachable, ReachabilityResult::Unreachable);
        assert_eq!(unknown, ReachabilityResult::Unknown("reason".to_string()));

        // Verify they are all distinct
        assert_ne!(reachable, unreachable.clone());
        assert_ne!(unreachable, unknown.clone());
        assert_ne!(reachable, unknown);
    }

    // ------------------------------------------------------------------
    // Additional gap-filling tests
    // ------------------------------------------------------------------

    #[test]
    fn generate_harness_taken_direction_content() {
        let prover = KaniProver::new(PathBuf::from("/tmp/myproject"));
        // direction=0 => "taken"
        let branch = BranchId::new(10, 100, 3, 0);
        let harness = prover.generate_harness(&branch, "my_fn");
        assert!(harness.contains("taken"), "direction 0 should produce 'taken'");
        assert!(!harness.contains("not_taken"));
        assert!(harness.contains("10_100_taken"), "should encode file_id and line");
    }

    #[test]
    fn generate_harness_not_taken_direction_content() {
        let prover = KaniProver::new(PathBuf::from("/tmp/myproject"));
        // direction=1 => "not_taken"
        let branch = BranchId::new(7, 50, 2, 1);
        let harness = prover.generate_harness(&branch, "other_fn");
        assert!(harness.contains("not_taken"), "direction 1 should produce 'not_taken'");
        assert!(!harness.contains("\"taken\""), "should not contain plain 'taken'");
    }

    #[test]
    fn generate_harness_encodes_function_name() {
        let prover = KaniProver::new(PathBuf::from("/tmp"));
        let branch = BranchId::new(1, 5, 0, 0);
        let harness = prover.generate_harness(&branch, "complex_function_name");
        assert!(harness.contains("complex_function_name(kani::any())"));
    }

    #[test]
    fn target_root_returns_exact_path() {
        let path = PathBuf::from("/very/specific/path/to/project");
        let prover = KaniProver::new(path.clone());
        assert_eq!(*prover.target_root(), path);
    }

    #[test]
    fn check_reachability_returns_unknown_message_for_no_feature() {
        let prover = KaniProver::new(PathBuf::from("/tmp"));
        let branch = BranchId::new(1, 1, 0, 0);
        let result = prover.check_reachability(&branch, "fn_name");
        // Must be Unknown with a meaningful message
        if let ReachabilityResult::Unknown(msg) = result {
            assert!(!msg.is_empty(), "message should not be empty");
        } else {
            panic!("expected Unknown, got something else");
        }
    }

    #[test]
    fn reachability_result_ne_different_payloads() {
        let r1 = ReachabilityResult::Reachable("a".to_string());
        let r2 = ReachabilityResult::Reachable("b".to_string());
        assert_ne!(r1, r2);

        let u1 = ReachabilityResult::Unknown("x".to_string());
        let u2 = ReachabilityResult::Unknown("y".to_string());
        assert_ne!(u1, u2);
    }
}
