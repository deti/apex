//! LLM-assisted flaky test repair suggestions.
//! Given a test identified as flaky (nondeterministic branch coverage),
//! asks an LLM to suggest a fix based on the test source code.

use crate::analysis::FlakyTest;

/// Generates LLM prompts for flaky test repair.
pub struct FlakyRepair;

impl FlakyRepair {
    /// Build a prompt for an LLM to suggest a fix for a flaky test.
    pub fn build_prompt(candidate: &FlakyTest, test_source: &str) -> String {
        let mut prompt = String::new();
        prompt.push_str("You are a test reliability expert. The following test is flaky ");
        prompt.push_str("(produces nondeterministic results across runs).\n\n");
        prompt.push_str(&format!("**Test name:** `{}`\n", candidate.test_name));
        prompt.push_str(&format!(
            "**Divergent runs:** {}/{}\n\n",
            candidate.divergent_runs, candidate.total_runs
        ));

        if !candidate.divergent_branches.is_empty() {
            prompt.push_str("**Divergent branches (hit inconsistently):**\n");
            for db in &candidate.divergent_branches {
                prompt.push_str(&format!(
                    "- Line {}, hit ratio: {}\n",
                    db.branch.line, db.hit_ratio
                ));
            }
            prompt.push('\n');
        }

        if !test_source.is_empty() {
            prompt.push_str("**Test source code:**\n```\n");
            prompt.push_str(test_source);
            prompt.push_str("\n```\n\n");
        }

        prompt.push_str(
            "Identify the root cause of flakiness and suggest a minimal code fix. \
             Common causes: timing dependencies, random ordering, shared mutable state, \
             filesystem race conditions, floating-point comparisons.\n\n\
             Reply with:\n1. Root cause\n2. Suggested fix (code diff)\n",
        );

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::DivergentBranch;
    use apex_core::types::BranchId;

    fn make_flaky() -> FlakyTest {
        FlakyTest {
            test_name: "test_random_order".to_string(),
            divergent_branches: vec![DivergentBranch {
                branch: BranchId::new(1, 42, 0, 0),
                file_path: None,
                hit_ratio: "3/5".to_string(),
            }],
            divergent_runs: 5,
            total_runs: 5,
        }
    }

    #[test]
    fn build_prompt_contains_test_name() {
        let flaky = make_flaky();
        let source =
            "def test_random_order():\n    items = list(set([1,2,3]))\n    assert items[0] == 1";
        let prompt = FlakyRepair::build_prompt(&flaky, source);
        assert!(prompt.contains("test_random_order"));
        assert!(prompt.contains("3/5"));
    }

    #[test]
    fn build_prompt_contains_source() {
        let flaky = make_flaky();
        let source = "def test_random_order():\n    pass";
        let prompt = FlakyRepair::build_prompt(&flaky, source);
        assert!(prompt.contains("def test_random_order"));
    }

    #[test]
    fn build_prompt_handles_empty_source() {
        let flaky = make_flaky();
        let prompt = FlakyRepair::build_prompt(&flaky, "");
        assert!(prompt.contains("test_random_order"));
    }
}
