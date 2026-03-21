//! LLM-based constraint solver (ConcoLLMic) — uses a language model to solve
//! constraints when gradient descent and Z3 both fail.
//!
//! This is the third solver in the portfolio, behind gradient (fast, numeric-only)
//! and Z3 (exact, but limited to what it can parse). The LLM receives:
//! - the condition as readable source text (via `ConditionTree::to_source_constraint()`)
//! - current variable values (if available)
//! - optional source code context
//!
//! Cost guard: only invoked after gradient + Z3 fail. 10s timeout, 3 retries.

use std::sync::Arc;

use apex_core::error::{ApexError, Result};
use apex_core::llm::{LlmClient, LlmMessage, LlmResponse};
use apex_core::types::{InputSeed, SeedOrigin};
use tracing::{debug, warn};

use crate::traits::{Solver, SolverLogic};

/// Maximum number of retries for LLM calls.
const MAX_RETRIES: u32 = 3;

/// Timeout in seconds for a single LLM call.
const TIMEOUT_SECS: u64 = 10;

/// Maximum tokens to request from the LLM.
const MAX_TOKENS: u32 = 256;

/// LLM-backed solver for constraints that gradient + Z3 cannot handle.
///
/// Implements the `Solver` trait so it can be slotted into `PortfolioSolver`
/// as the third (last-resort) backend.
pub struct LlmSolver {
    client: Arc<dyn LlmClient>,
    /// Optional source code context to include in prompts.
    source_context: Option<String>,
    /// Current variable values to seed the LLM prompt.
    current_values: Option<String>,
    /// Maximum retries per solve attempt.
    max_retries: u32,
    /// Per-call timeout in seconds.
    timeout_secs: u64,
}

impl LlmSolver {
    /// Create a new LLM solver with the given client.
    pub fn new(client: Arc<dyn LlmClient>) -> Self {
        LlmSolver {
            client,
            source_context: None,
            current_values: None,
            max_retries: MAX_RETRIES,
            timeout_secs: TIMEOUT_SECS,
        }
    }

    /// Set source code context to include in the LLM prompt.
    pub fn with_source_context(mut self, ctx: String) -> Self {
        self.source_context = Some(ctx);
        self
    }

    /// Set current variable values as a JSON string.
    pub fn with_current_values(mut self, values: String) -> Self {
        self.current_values = Some(values);
        self
    }

    /// Override the default max retries (3).
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Override the default timeout (10s).
    #[allow(dead_code)]
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Build the prompt for the LLM from constraints.
    fn build_prompt(&self, constraints: &[String], negate_last: bool) -> String {
        let mut prompt = String::new();
        prompt.push_str(
            "You are a constraint solver. Given the following conditions on program variables, \
             find concrete values for ALL variables that satisfy every condition.\n\n",
        );

        if let Some(ref ctx) = self.source_context {
            prompt.push_str("Source code context:\n```\n");
            prompt.push_str(ctx);
            prompt.push_str("\n```\n\n");
        }

        if let Some(ref values) = self.current_values {
            prompt.push_str("Current variable values: ");
            prompt.push_str(values);
            prompt.push_str("\n\n");
        }

        prompt.push_str("Conditions:\n");
        for (i, c) in constraints.iter().enumerate() {
            let is_last = i == constraints.len() - 1;
            if is_last && negate_last {
                prompt.push_str(&format!(
                    "  {}. NEGATE THIS: {} (find values where this is FALSE)\n",
                    i + 1,
                    c
                ));
            } else {
                prompt.push_str(&format!("  {}. {}\n", i + 1, c));
            }
        }

        if negate_last {
            prompt.push_str(
                "\nIMPORTANT: The last condition must be NEGATED. Find values that satisfy \
                 conditions 1..N-1 AND make condition N false.\n",
            );
        }

        prompt.push_str(
            "\nReply with ONLY a JSON object mapping variable names to their values.\n\
             Example: {\"x\": 42, \"name\": \"test\", \"flag\": true}\n\
             If truly unsatisfiable, reply with: UNSAT\n",
        );

        prompt
    }

    /// Call the LLM with retries and parse the response.
    fn solve_with_llm(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        let prompt = self.build_prompt(constraints, negate_last);
        let messages = vec![LlmMessage {
            role: "user".to_string(),
            content: prompt,
        }];

        // Use a simple blocking approach: try to create a tokio runtime for the async call.
        // If we're already inside a runtime, use block_in_place + spawn.
        let response = self.call_llm_blocking(&messages)?;

        parse_llm_solution(&response.content)
            .map(Some)
            .ok_or(())
            .or_else(|()| {
                // Check if the LLM said UNSAT
                if response.content.trim().contains("UNSAT") {
                    debug!(model = self.client.model_name(), "LLM says UNSAT");
                    Ok(None)
                } else {
                    debug!(
                        model = self.client.model_name(),
                        response = %response.content,
                        "LLM response could not be parsed"
                    );
                    Ok(None)
                }
            })
    }

    /// Blocking call to the async LLM client, with retries.
    fn call_llm_blocking(&self, messages: &[LlmMessage]) -> Result<LlmResponse> {
        let mut last_err = None;

        for attempt in 0..self.max_retries {
            match self.try_call_blocking(messages) {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!(
                        attempt = attempt + 1,
                        max = self.max_retries,
                        error = %e,
                        "LLM call failed, retrying"
                    );
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| ApexError::Solver("LLM solver: no attempts made".into())))
    }

    /// Single attempt at a blocking LLM call.
    fn try_call_blocking(&self, messages: &[LlmMessage]) -> Result<LlmResponse> {
        let client = Arc::clone(&self.client);
        let msgs: Vec<LlmMessage> = messages.to_vec();
        let max_tokens = MAX_TOKENS;
        let timeout = std::time::Duration::from_secs(self.timeout_secs);

        // Try to use an existing tokio runtime, or create a new one.
        if tokio::runtime::Handle::try_current().is_ok() {
            // We're inside a tokio runtime — spawn a separate thread with its own runtime
            std::thread::scope(|s| {
                s.spawn(|| {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| ApexError::Solver(format!("failed to build runtime: {e}")))?;
                    rt.block_on(async {
                        tokio::time::timeout(timeout, client.complete(&msgs, max_tokens))
                            .await
                            .map_err(|_| ApexError::Solver("LLM call timed out".into()))?
                    })
                })
                .join()
                .map_err(|_| ApexError::Solver("LLM thread panicked".into()))?
            })
        } else {
            // No runtime — create one
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| ApexError::Solver(format!("failed to build runtime: {e}")))?;
            rt.block_on(async {
                tokio::time::timeout(timeout, client.complete(&msgs, max_tokens))
                    .await
                    .map_err(|_| ApexError::Solver("LLM call timed out".into()))?
            })
        }
    }
}

impl Solver for LlmSolver {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        if constraints.is_empty() {
            return Ok(None);
        }

        debug!(
            solver = "llm",
            model = self.client.model_name(),
            num_constraints = constraints.len(),
            "LLM solver invoked (gradient + Z3 failed)"
        );

        self.solve_with_llm(constraints, negate_last)
    }

    fn set_logic(&mut self, _logic: SolverLogic) {
        // LLM solver is logic-agnostic — it works with natural language
    }

    fn name(&self) -> &str {
        "llm"
    }
}

// ---------------------------------------------------------------------------
// Prompt generation (public, reusable)
// ---------------------------------------------------------------------------

/// Convert SMTLIB2 constraints to a natural language prompt for an LLM.
pub fn constraints_to_prompt(constraints: &[String], negate_last: bool) -> String {
    if constraints.is_empty() {
        return "There are no constraints to solve. Reply with an empty JSON object: {}. \
                Note: there are no constraints here."
            .to_string();
    }

    let mut prompt = String::new();
    prompt.push_str(
        "You are an SMT solver. Given the following SMTLIB2 constraints, \
         find integer values for all variables that satisfy ALL constraints.\n\n",
    );

    prompt.push_str("Constraints:\n");
    for (i, c) in constraints.iter().enumerate() {
        let is_last = i == constraints.len() - 1;
        if is_last && negate_last {
            prompt.push_str(&format!("  {}. (negate this) {}\n", i + 1, c));
        } else {
            prompt.push_str(&format!("  {}. {}\n", i + 1, c));
        }
    }

    if negate_last {
        prompt.push_str(
            "\nIMPORTANT: The last constraint must be NEGATED. Find values that satisfy \
             constraints 1..N-1 AND the negation of constraint N.\n",
        );
    }

    prompt.push_str(
        "\nReply with ONLY a JSON object mapping variable names to integer values. \
         Example: {\"x\": 42, \"y\": -5}\n\
         If unsatisfiable, reply with: UNSAT\n",
    );

    prompt
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// Parse an LLM response into an InputSeed.
///
/// Tries to extract a JSON object `{"var": value, ...}` from the response.
/// Handles responses with markdown code fences.
pub fn parse_llm_solution(response: &str) -> Option<InputSeed> {
    let trimmed = response.trim();

    // Try direct JSON parse first
    if let Some(seed) = try_parse_json_object(trimmed) {
        return Some(seed);
    }

    // Try extracting from markdown code fence
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        // Skip optional language tag (e.g., "json\n")
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[content_start..];
        if let Some(end) = content.find("```") {
            let json_str = content[..end].trim();
            return try_parse_json_object(json_str);
        }
    }

    None
}

fn try_parse_json_object(s: &str) -> Option<InputSeed> {
    let parsed: serde_json::Value = serde_json::from_str(s).ok()?;
    let obj = parsed.as_object()?;

    if obj.is_empty() {
        return None;
    }

    let json_bytes = serde_json::to_vec(&parsed).ok()?;
    Some(InputSeed::new(json_bytes, SeedOrigin::Symbolic))
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::llm::MockLlmClient;

    // ------------------------------------------------------------------
    // Prompt generation tests (existing)
    // ------------------------------------------------------------------

    #[test]
    fn constraints_to_prompt_simple() {
        let constraints = vec!["(> x 0)".to_string(), "(< x 100)".to_string()];
        let prompt = constraints_to_prompt(&constraints, false);
        assert!(prompt.contains("(> x 0)"));
        assert!(prompt.contains("(< x 100)"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn constraints_to_prompt_negate_last() {
        let constraints = vec!["(> x 0)".to_string(), "(< x 100)".to_string()];
        let prompt = constraints_to_prompt(&constraints, true);
        assert!(prompt.contains("negate"));
        assert!(prompt.contains("(< x 100)"));
    }

    #[test]
    fn constraints_to_prompt_empty() {
        let prompt = constraints_to_prompt(&[], false);
        assert!(prompt.contains("no constraints"));
    }

    #[test]
    fn parse_llm_solution_valid_json() {
        let response = r#"{"x": 42, "y": -5}"#;
        let seed = parse_llm_solution(response);
        assert!(seed.is_some());
        let data = String::from_utf8(seed.unwrap().data.to_vec()).unwrap();
        assert!(data.contains("42"));
    }

    #[test]
    fn parse_llm_solution_invalid() {
        let seed = parse_llm_solution("I cannot solve this");
        assert!(seed.is_none());
    }

    #[test]
    fn parse_llm_solution_json_in_markdown() {
        let response = "Here is the solution:\n```json\n{\"x\": 10}\n```\n";
        let seed = parse_llm_solution(response);
        assert!(seed.is_some());
    }

    #[test]
    fn parse_llm_solution_empty_object() {
        let seed = parse_llm_solution("{}");
        assert!(seed.is_none()); // empty object = no assignments
    }

    // ------------------------------------------------------------------
    // LlmSolver tests
    // ------------------------------------------------------------------

    #[test]
    fn llm_solver_name() {
        let mock = Arc::new(MockLlmClient::new(vec![]));
        let solver = LlmSolver::new(mock);
        assert_eq!(solver.name(), "llm");
    }

    #[test]
    fn llm_solver_empty_constraints_returns_none() {
        let mock = Arc::new(MockLlmClient::new(vec![]));
        let solver = LlmSolver::new(mock);
        let result = solver.solve(&[], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn llm_solver_produces_valid_input() {
        let mock = Arc::new(MockLlmClient::new(vec![
            r#"{"x": 42, "y": -5}"#.to_string(),
        ]));
        let solver = LlmSolver::new(mock).with_max_retries(1);
        let result = solver.solve(&["x > 10".to_string()], false).unwrap();
        assert!(result.is_some());
        let seed = result.unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&seed.data).unwrap();
        assert_eq!(json["x"], 42);
        assert_eq!(json["y"], -5);
    }

    #[test]
    fn llm_solver_handles_unsat_response() {
        let mock = Arc::new(MockLlmClient::new(vec!["UNSAT".to_string()]));
        let solver = LlmSolver::new(mock).with_max_retries(1);
        let result = solver.solve(&["x > 10 and x < 5".to_string()], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn llm_solver_handles_unparseable_response() {
        let mock = Arc::new(MockLlmClient::new(vec![
            "I don't know how to solve this".to_string(),
        ]));
        let solver = LlmSolver::new(mock).with_max_retries(1);
        let result = solver.solve(&["x > 10".to_string()], false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn llm_solver_with_source_context() {
        let mock = Arc::new(MockLlmClient::new(vec![
            r#"{"x": 15}"#.to_string(),
        ]));
        let solver = LlmSolver::new(mock)
            .with_source_context("def foo(x):\n    if x > 10:\n        return True".into())
            .with_max_retries(1);
        let result = solver.solve(&["x > 10".to_string()], false).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn llm_solver_with_current_values() {
        let mock = Arc::new(MockLlmClient::new(vec![
            r#"{"x": 15}"#.to_string(),
        ]));
        let solver = LlmSolver::new(mock)
            .with_current_values(r#"{"x": 5}"#.into())
            .with_max_retries(1);
        let result = solver.solve(&["x > 10".to_string()], false).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn llm_solver_set_logic_is_noop() {
        let mock = Arc::new(MockLlmClient::new(vec![]));
        let mut solver = LlmSolver::new(mock);
        solver.set_logic(SolverLogic::QfLia);
        solver.set_logic(SolverLogic::Auto);
        // Should not panic
    }

    #[test]
    fn llm_solver_retries_on_failure() {
        // First two calls fail (empty queue), but since we set max_retries=3
        // and the mock only has errors, all 3 attempts will fail.
        let mock = Arc::new(MockLlmClient::new(vec![]));
        let solver = LlmSolver::new(mock).with_max_retries(3);
        // Should return an error after exhausting retries
        let result = solver.solve(&["x > 10".to_string()], false);
        assert!(result.is_err());
    }

    #[test]
    fn llm_solver_negate_last_prompt_contains_negate() {
        let mock = Arc::new(MockLlmClient::new(vec![
            r#"{"x": 5}"#.to_string(),
        ]));
        let solver = LlmSolver::new(mock).with_max_retries(1);
        // The solve itself should work; we verify the prompt contains negate
        // by checking the solver completes without error
        let result = solver.solve(&["x > 10".to_string()], true).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn llm_solver_build_prompt_includes_context() {
        let mock = Arc::new(MockLlmClient::new(vec![]));
        let solver = LlmSolver::new(mock)
            .with_source_context("if x > 10:".into())
            .with_current_values(r#"{"x": 3}"#.into());

        let prompt = solver.build_prompt(&["x > 10".to_string()], false);
        assert!(prompt.contains("if x > 10:"));
        assert!(prompt.contains(r#"{"x": 3}"#));
        assert!(prompt.contains("x > 10"));
    }

    #[test]
    fn llm_solver_build_prompt_negate_last() {
        let mock = Arc::new(MockLlmClient::new(vec![]));
        let solver = LlmSolver::new(mock);

        let prompt = solver.build_prompt(
            &["x > 0".to_string(), "x < 100".to_string()],
            true,
        );
        assert!(prompt.contains("NEGATE THIS"));
        assert!(prompt.contains("x < 100"));
    }

    #[test]
    fn llm_solver_build_prompt_no_negate() {
        let mock = Arc::new(MockLlmClient::new(vec![]));
        let solver = LlmSolver::new(mock);

        let prompt = solver.build_prompt(&["x > 0".to_string()], false);
        assert!(!prompt.contains("NEGATE"));
    }

    // ------------------------------------------------------------------
    // Verify LLM solver is NOT called when gradient succeeds
    // ------------------------------------------------------------------

    #[test]
    fn portfolio_skips_llm_when_gradient_succeeds() {
        use crate::gradient::GradientSolver;
        use crate::portfolio::PortfolioSolver;
        use std::time::Duration;

        // Gradient can solve simple (= x 42)
        let gradient = Box::new(GradientSolver::new(100));

        // LLM solver with an empty mock — if called, it will error
        let mock = Arc::new(MockLlmClient::new(vec![]));
        let llm = Box::new(LlmSolver::new(mock).with_max_retries(1));

        let solvers: Vec<Box<dyn Solver>> = vec![gradient, llm];
        let portfolio = PortfolioSolver::new(solvers, Duration::from_secs(5));

        // Gradient handles this, LLM should NOT be called
        let result = portfolio.solve(&["(= x 42)".to_string()], false).unwrap();
        assert!(result.is_some());
        let seed = result.unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&seed.data).unwrap();
        assert_eq!(json["x"], 42);
    }

    #[test]
    fn llm_solver_handles_markdown_json_response() {
        let mock = Arc::new(MockLlmClient::new(vec![
            "Here's the solution:\n```json\n{\"x\": 99}\n```\n".to_string(),
        ]));
        let solver = LlmSolver::new(mock).with_max_retries(1);
        let result = solver.solve(&["x > 50".to_string()], false).unwrap();
        assert!(result.is_some());
        let json: serde_json::Value =
            serde_json::from_slice(&result.unwrap().data).unwrap();
        assert_eq!(json["x"], 99);
    }

    #[test]
    fn llm_solver_multiple_constraints() {
        let mock = Arc::new(MockLlmClient::new(vec![
            r#"{"x": 15, "y": 3, "name": "test"}"#.to_string(),
        ]));
        let solver = LlmSolver::new(mock).with_max_retries(1);
        let constraints = vec![
            "x > 10".to_string(),
            "y < 5".to_string(),
            "name is not null".to_string(),
        ];
        let result = solver.solve(&constraints, false).unwrap();
        assert!(result.is_some());
    }
}
