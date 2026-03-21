//! Mutation-guided test generation (Meta ACH approach).
//!
//! Generates code mutations (operator swap, boundary shift, return change, etc.)
//! and prompts an LLM to write tests that distinguish the original from each mutant.
//! This targets mutation-score improvement rather than line/branch coverage alone.

use apex_core::types::Language;

// ---------------------------------------------------------------------------
// Generated test
// ---------------------------------------------------------------------------

/// A test generated to kill a specific mutant.
#[derive(Debug, Clone)]
pub struct GeneratedTest {
    /// The test source code.
    pub code: String,
    /// Which mutation this test is designed to catch.
    pub target_mutation: String,
    /// The language of the generated test.
    pub lang: Language,
}

// ---------------------------------------------------------------------------
// Mutation operator
// ---------------------------------------------------------------------------

/// A named mutation operator that transforms source code at a given line.
pub struct MutationOperator {
    pub name: &'static str,
    pub apply: fn(source: &str, line: u32) -> Option<String>,
}

/// Apply an arithmetic swap on the target line: `+` <-> `-`, `*` <-> `/`.
pub fn arithmetic_swap(source: &str, line: u32) -> Option<String> {
    apply_line_mutation(source, line, |l| {
        if l.contains(" + ") {
            Some(l.replacen(" + ", " - ", 1))
        } else if l.contains(" - ") {
            Some(l.replacen(" - ", " + ", 1))
        } else if l.contains(" * ") {
            Some(l.replacen(" * ", " / ", 1))
        } else if l.contains(" / ") {
            Some(l.replacen(" / ", " * ", 1))
        } else {
            None
        }
    })
}

/// Flip a comparison operator on the target line.
pub fn comparison_flip(source: &str, line: u32) -> Option<String> {
    apply_line_mutation(source, line, |l| {
        // Order matters: check two-char operators before single-char.
        if l.contains(" >= ") {
            Some(l.replacen(" >= ", " < ", 1))
        } else if l.contains(" <= ") {
            Some(l.replacen(" <= ", " > ", 1))
        } else if l.contains(" != ") {
            Some(l.replacen(" != ", " == ", 1))
        } else if l.contains(" == ") {
            Some(l.replacen(" == ", " != ", 1))
        } else if l.contains(" > ") {
            Some(l.replacen(" > ", " <= ", 1))
        } else if l.contains(" < ") {
            Some(l.replacen(" < ", " >= ", 1))
        } else {
            None
        }
    })
}

/// Change a return statement: `return x` -> `return 0` (or `return None` for Python).
pub fn return_change(source: &str, line: u32) -> Option<String> {
    apply_line_mutation(source, line, |l| {
        let trimmed = l.trim();
        if trimmed.starts_with("return ") && trimmed != "return 0" && trimmed != "return None" {
            let indent = &l[..l.len() - l.trim_start().len()];
            Some(format!("{indent}return 0"))
        } else {
            None
        }
    })
}

/// Shift a boundary constant by 1: `> 0` -> `> 1`, `>= 0` -> `> 0`.
pub fn boundary_shift(source: &str, line: u32) -> Option<String> {
    apply_line_mutation(source, line, |l| {
        // Look for patterns like `>= 0` -> `> 0` or `> 0` -> `> 1`
        if l.contains(" >= 0") {
            Some(l.replacen(" >= 0", " > 0", 1))
        } else if l.contains(" > 0") {
            Some(l.replacen(" > 0", " > 1", 1))
        } else if l.contains(" <= 0") {
            Some(l.replacen(" <= 0", " < 0", 1))
        } else if l.contains(" < 0") {
            Some(l.replacen(" < 0", " < 1", 1))
        } else {
            None
        }
    })
}

/// Negate a condition: `if x` -> `if not x` (Python) / `if !x` (other langs).
pub fn negate_condition(source: &str, line: u32) -> Option<String> {
    apply_line_mutation(source, line, |l| {
        let trimmed = l.trim();
        if let Some(condition) = trimmed.strip_prefix("if ") {
            let indent = &l[..l.len() - l.trim_start().len()];
            // If it already has a negation, remove it.
            if let Some(rest) = condition.strip_prefix("not ") {
                Some(format!("{indent}if {rest}"))
            } else if let Some(rest) = condition.strip_prefix('!') {
                Some(format!("{indent}if {rest}"))
            } else {
                Some(format!("{indent}if not {condition}"))
            }
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Apply a mutation function to a specific 1-indexed line. Returns `None` if
/// the line does not exist or the mutator returns `None`.
fn apply_line_mutation<F>(source: &str, line: u32, mutate: F) -> Option<String>
where
    F: FnOnce(&str) -> Option<String>,
{
    let lines: Vec<&str> = source.lines().collect();
    let idx = (line as usize).checked_sub(1)?;
    if idx >= lines.len() {
        return None;
    }
    let mutated_line = mutate(lines[idx])?;
    let mut result: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    result[idx] = mutated_line;
    Some(result.join("\n"))
}

/// All built-in mutation operators.
pub fn default_operators() -> Vec<MutationOperator> {
    vec![
        MutationOperator {
            name: "arithmetic_swap",
            apply: arithmetic_swap,
        },
        MutationOperator {
            name: "comparison_flip",
            apply: comparison_flip,
        },
        MutationOperator {
            name: "return_change",
            apply: return_change,
        },
        MutationOperator {
            name: "boundary_shift",
            apply: boundary_shift,
        },
        MutationOperator {
            name: "negate_condition",
            apply: negate_condition,
        },
    ]
}

// ---------------------------------------------------------------------------
// Mutation
// ---------------------------------------------------------------------------

/// A single mutation applied to source code.
#[derive(Debug, Clone)]
pub struct Mutation {
    /// Name of the operator that produced this mutation.
    pub operator_name: String,
    /// 1-indexed line where the mutation was applied.
    pub line: u32,
    /// The mutated source code.
    pub mutated_source: String,
}

// ---------------------------------------------------------------------------
// MutationTestGenerator
// ---------------------------------------------------------------------------

/// Maximum number of mutations to generate per function (cost control).
const MAX_MUTATIONS_PER_FUNCTION: usize = 5;

/// Generates tests guided by source-code mutations (Meta ACH approach).
///
/// For each function, applies mutation operators to find interesting mutants,
/// then prompts an LLM to write tests that distinguish original from mutant.
pub struct MutationTestGenerator {
    /// Optional LLM endpoint URL. When `None`, uses `build_prompt` for
    /// offline/testing scenarios.
    pub llm_endpoint: Option<String>,
}

impl MutationTestGenerator {
    /// Create a new generator with no LLM endpoint (prompt-only mode).
    pub fn new() -> Self {
        Self {
            llm_endpoint: None,
        }
    }

    /// Create a new generator with a specific LLM endpoint.
    pub fn with_endpoint(endpoint: String) -> Self {
        Self {
            llm_endpoint: Some(endpoint),
        }
    }

    /// Generate all applicable mutations for a function, up to `MAX_MUTATIONS_PER_FUNCTION`.
    pub fn generate_mutations(&self, function_source: &str) -> Vec<Mutation> {
        let operators = default_operators();
        let line_count = function_source.lines().count() as u32;
        let mut mutations = Vec::new();

        for op in &operators {
            for line in 1..=line_count {
                if let Some(mutated) = (op.apply)(function_source, line) {
                    mutations.push(Mutation {
                        operator_name: op.name.to_string(),
                        line,
                        mutated_source: mutated,
                    });
                    if mutations.len() >= MAX_MUTATIONS_PER_FUNCTION {
                        return mutations;
                    }
                }
            }
        }

        mutations
    }

    /// Build a prompt asking the LLM to write a test that kills a specific mutant.
    pub fn build_prompt(
        &self,
        original_source: &str,
        mutation: &Mutation,
        lang: Language,
    ) -> String {
        format!(
            "Original function:\n\
             ```\n{original}\n```\n\n\
             Mutated function (change on line {line}, operator: {op}):\n\
             ```\n{mutated}\n```\n\n\
             Write a test that passes on the original but fails on the mutant.\n\
             Language: {lang}\n\
             Respond with only the test code, no explanation.",
            original = original_source,
            line = mutation.line,
            op = mutation.operator_name,
            mutated = mutation.mutated_source,
            lang = lang,
        )
    }

    /// Build a single batched prompt covering multiple mutations (cost control).
    pub fn build_batched_prompt(
        &self,
        original_source: &str,
        mutations: &[Mutation],
        lang: Language,
    ) -> String {
        let mut prompt = format!(
            "Original function:\n```\n{original}\n```\n\n\
             Below are {count} mutants. For each, write a test that passes on the \
             original but fails on that mutant. Label each test with the mutation \
             number.\n\nLanguage: {lang}\n\n",
            original = original_source,
            count = mutations.len(),
            lang = lang,
        );

        for (i, m) in mutations.iter().enumerate() {
            prompt.push_str(&format!(
                "Mutation {n} (line {line}, {op}):\n```\n{mutated}\n```\n\n",
                n = i + 1,
                line = m.line,
                op = m.operator_name,
                mutated = m.mutated_source,
            ));
        }

        prompt.push_str("Respond with only the test code, no explanation.");
        prompt
    }

    /// Generate tests for a function using a callback for LLM interaction.
    ///
    /// The `llm_call` callback receives a prompt string and returns generated
    /// test code. This design keeps the generator testable without real LLM deps.
    pub fn generate_tests_with<F>(
        &self,
        function_source: &str,
        lang: Language,
        llm_call: F,
    ) -> Vec<GeneratedTest>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mutations = self.generate_mutations(function_source);
        if mutations.is_empty() {
            return Vec::new();
        }

        // Use batched prompt for cost control.
        let prompt = self.build_batched_prompt(function_source, &mutations, lang);
        if let Some(response) = llm_call(&prompt) {
            // Split response into per-mutation tests if possible.
            let tests = self.parse_batched_response(&response, &mutations, lang);
            if !tests.is_empty() {
                return tests;
            }
            // Fallback: treat entire response as one test for the first mutation.
            return vec![GeneratedTest {
                code: response,
                target_mutation: mutations[0].operator_name.clone(),
                lang,
            }];
        }

        Vec::new()
    }

    /// Parse a batched LLM response, splitting by "Mutation N" markers.
    fn parse_batched_response(
        &self,
        response: &str,
        mutations: &[Mutation],
        lang: Language,
    ) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();

        for (i, mutation) in mutations.iter().enumerate() {
            let marker = format!("Mutation {}", i + 1);
            let next_marker = format!("Mutation {}", i + 2);

            if let Some(start) = response.find(&marker) {
                let after_marker = start + marker.len();
                let end = if i + 1 < mutations.len() {
                    response[after_marker..]
                        .find(&next_marker)
                        .map(|p| after_marker + p)
                        .unwrap_or(response.len())
                } else {
                    response.len()
                };

                let section = response[after_marker..end].trim();
                if !section.is_empty() {
                    tests.push(GeneratedTest {
                        code: section.to_string(),
                        target_mutation: mutation.operator_name.clone(),
                        lang,
                    });
                }
            }
        }

        tests
    }
}

impl Default for MutationTestGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_FUNCTION: &str = "\
def calculate(x, y):
    if x > 0:
        return x + y
    return 0";

    #[test]
    fn arithmetic_swap_plus_to_minus() {
        let result = arithmetic_swap(SAMPLE_FUNCTION, 3).unwrap();
        assert!(result.contains(" - "), "expected + swapped to -");
        assert!(!result.contains(" + "), "original + should be gone");
    }

    #[test]
    fn arithmetic_swap_no_operator_returns_none() {
        let result = arithmetic_swap(SAMPLE_FUNCTION, 1);
        assert!(result.is_none(), "line 1 has no arithmetic operator");
    }

    #[test]
    fn comparison_flip_gt_to_le() {
        let result = comparison_flip(SAMPLE_FUNCTION, 2).unwrap();
        assert!(
            result.contains(" <= "),
            "expected > flipped to <=, got: {result}"
        );
    }

    #[test]
    fn return_change_replaces_return() {
        let result = return_change(SAMPLE_FUNCTION, 3).unwrap();
        assert!(result.contains("return 0"), "expected return changed to 0");
    }

    #[test]
    fn return_change_skips_already_zero() {
        // Line 4 is already `return 0`, should return None.
        let result = return_change(SAMPLE_FUNCTION, 4);
        assert!(result.is_none(), "return 0 should not be mutated again");
    }

    #[test]
    fn boundary_shift_gt_zero() {
        let src = "def f(x):\n    if x > 0:\n        pass";
        let result = boundary_shift(src, 2).unwrap();
        assert!(
            result.contains(" > 1"),
            "expected > 0 shifted to > 1, got: {result}"
        );
    }

    #[test]
    fn negate_condition_adds_not() {
        let result = negate_condition(SAMPLE_FUNCTION, 2).unwrap();
        assert!(
            result.contains("if not x > 0"),
            "expected negation, got: {result}"
        );
    }

    #[test]
    fn negate_condition_removes_existing_not() {
        let src = "def f(x):\n    if not x:\n        pass";
        let result = negate_condition(src, 2).unwrap();
        assert!(
            result.contains("if x:"),
            "expected negation removed, got: {result}"
        );
    }

    #[test]
    fn generate_mutations_respects_limit() {
        let gen = MutationTestGenerator::new();
        let mutations = gen.generate_mutations(SAMPLE_FUNCTION);
        assert!(
            mutations.len() <= MAX_MUTATIONS_PER_FUNCTION,
            "should not exceed {} mutations, got {}",
            MAX_MUTATIONS_PER_FUNCTION,
            mutations.len()
        );
        assert!(!mutations.is_empty(), "should find at least one mutation");
    }

    #[test]
    fn build_prompt_contains_original_and_mutant() {
        let gen = MutationTestGenerator::new();
        let mutation = Mutation {
            operator_name: "arithmetic_swap".to_string(),
            line: 3,
            mutated_source: "    return x - y".to_string(),
        };
        let prompt = gen.build_prompt(SAMPLE_FUNCTION, &mutation, Language::Python);
        assert!(prompt.contains("Original function:"));
        assert!(prompt.contains("return x - y"));
        assert!(prompt.contains("line 3"));
        assert!(prompt.contains("python"));
    }

    #[test]
    fn build_batched_prompt_includes_all_mutations() {
        let gen = MutationTestGenerator::new();
        let mutations = gen.generate_mutations(SAMPLE_FUNCTION);
        let prompt = gen.build_batched_prompt(SAMPLE_FUNCTION, &mutations, Language::Python);
        assert!(prompt.contains("Original function:"));
        for (i, _) in mutations.iter().enumerate() {
            assert!(
                prompt.contains(&format!("Mutation {}", i + 1)),
                "missing Mutation {} marker",
                i + 1
            );
        }
    }

    #[test]
    fn generate_tests_with_callback() {
        let gen = MutationTestGenerator::new();
        let tests = gen.generate_tests_with(SAMPLE_FUNCTION, Language::Python, |_prompt| {
            Some("Mutation 1\ndef test_add(): assert calculate(1, 2) == 3\n\nMutation 2\ndef test_compare(): assert calculate(-1, 0) == 0".to_string())
        });
        assert!(
            !tests.is_empty(),
            "should produce at least one test from callback"
        );
    }

    #[test]
    fn generate_tests_no_mutations_returns_empty() {
        let gen = MutationTestGenerator::new();
        // Source with nothing mutable.
        let tests = gen.generate_tests_with("pass", Language::Python, |_| {
            Some("test code".into())
        });
        assert!(tests.is_empty(), "no mutations => no tests");
    }

    #[test]
    fn generate_tests_llm_returns_none() {
        let gen = MutationTestGenerator::new();
        let tests =
            gen.generate_tests_with(SAMPLE_FUNCTION, Language::Python, |_| None);
        assert!(tests.is_empty(), "LLM returning None => no tests");
    }

    #[test]
    fn with_endpoint_stores_url() {
        let gen = MutationTestGenerator::with_endpoint("http://localhost:8080".into());
        assert_eq!(
            gen.llm_endpoint.as_deref(),
            Some("http://localhost:8080")
        );
    }

    #[test]
    fn default_has_no_endpoint() {
        let gen = MutationTestGenerator::default();
        assert!(gen.llm_endpoint.is_none());
    }

    #[test]
    fn apply_line_mutation_out_of_bounds() {
        let result = arithmetic_swap("single line", 99);
        assert!(result.is_none());
    }

    #[test]
    fn apply_line_mutation_zero_line() {
        let result = arithmetic_swap("a + b", 0);
        assert!(result.is_none(), "line 0 should be out of bounds");
    }

    #[test]
    fn parse_batched_response_fallback() {
        let gen = MutationTestGenerator::new();
        // Response without markers => fallback to whole response.
        let tests = gen.generate_tests_with(SAMPLE_FUNCTION, Language::Python, |_| {
            Some("def test_all(): pass".into())
        });
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].code, "def test_all(): pass");
    }
}
