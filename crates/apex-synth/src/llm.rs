//! LLM-guided test synthesizer implementing CoverUp-style closed-loop refinement.
//!
//! The loop: generate test → run → check coverage → if miss, refine with feedback.
//! Callbacks for LLM calls and test execution keep this fully testable without
//! real LLM or execution dependencies.

use apex_core::{error::Result, types::BranchId};

// ---------------------------------------------------------------------------
// Test result
// ---------------------------------------------------------------------------

/// Result of a single test generation attempt.
#[derive(Debug, Clone)]
pub enum TestResult {
    /// Test executed but errored.
    Error(String),
    /// Test ran cleanly but didn't cover the target branch.
    NoCoverageGain,
    /// Test covered new branches.
    Success(Vec<BranchId>),
}

// ---------------------------------------------------------------------------
// Coverage gap
// ---------------------------------------------------------------------------

/// A coverage gap that needs a test.
#[derive(Debug, Clone)]
pub struct CoverageGap {
    pub file_path: String,
    pub target_line: u32,
    pub function_name: Option<String>,
    /// Code around the uncovered line.
    pub source_segment: String,
    pub uncovered_lines: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Synth attempt
// ---------------------------------------------------------------------------

/// A single attempt at generating a test.
#[derive(Debug, Clone)]
pub struct SynthAttempt {
    pub test_code: String,
    pub coverage_delta: Vec<BranchId>,
    pub error: Option<String>,
    pub attempt_number: u32,
}

// ---------------------------------------------------------------------------
// LLM message types
// ---------------------------------------------------------------------------

/// LLM message for conversation history.
#[derive(Debug, Clone)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmRole {
    System,
    User,
    Assistant,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for LLM-guided synthesis.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// Maximum refinement attempts per gap (default 3, from CoverUp).
    pub max_attempts: u32,
    /// LLM model identifier.
    pub model: String,
    /// Sampling temperature.
    pub temperature: f64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            max_attempts: 3,
            model: "claude-sonnet-4-20250514".to_string(),
            temperature: 0.7,
        }
    }
}

// ---------------------------------------------------------------------------
// LlmSynthesizer
// ---------------------------------------------------------------------------

/// LLM-guided test synthesizer implementing CoverUp's closed-loop refinement.
pub struct LlmSynthesizer {
    config: LlmConfig,
}

impl LlmSynthesizer {
    pub fn new(config: LlmConfig) -> Self {
        LlmSynthesizer { config }
    }

    /// Format the uncovered lines description for a coverage gap.
    ///
    /// Returns either "line X" (single line) or "lines X, Y, Z" (multiple lines).
    fn format_uncovered_lines(gap: &CoverageGap) -> String {
        if gap.uncovered_lines.is_empty() {
            format!("line {}", gap.target_line)
        } else {
            let parts: Vec<String> = gap.uncovered_lines.iter().map(|l| l.to_string()).collect();
            format!("lines {}", parts.join(", "))
        }
    }

    /// Build the initial prompt messages for a coverage gap.
    ///
    /// Follows CoverUp's structure:
    /// - System message establishing the expert test-developer role.
    /// - User message with the source segment (tagged lines) and gap description.
    pub fn initial_prompt(&self, gap: &CoverageGap) -> Vec<LlmMessage> {
        let system = LlmMessage {
            role: LlmRole::System,
            content: "You are an expert test developer. \
                      Your task is to write a test that covers the specified \
                      uncovered lines in the provided source code. \
                      Respond with only the test code, no explanation."
                .to_string(),
        };

        let fn_hint = gap
            .function_name
            .as_deref()
            .map(|n| format!(" (function `{n}`)"))
            .unwrap_or_default();

        let uncovered = Self::format_uncovered_lines(gap);

        let user_content = format!(
            "File: {file}{fn_hint}\n\
             Uncovered: {uncovered}\n\n\
             Source segment:\n```\n{segment}\n```\n\n\
             Write a test that exercises {uncovered} in {file}.",
            file = gap.file_path,
            fn_hint = fn_hint,
            uncovered = uncovered,
            segment = gap.source_segment,
        );

        let user = LlmMessage {
            role: LlmRole::User,
            content: user_content,
        };

        vec![system, user]
    }

    /// Build an error-feedback message.
    ///
    /// Mirrors CoverUp: "The test yielded an error: {error}. Modify the test to fix it."
    pub fn error_prompt(&self, error: &str) -> LlmMessage {
        LlmMessage {
            role: LlmRole::User,
            content: format!(
                "The test yielded an error: {error}. \
                 Modify the test to fix it."
            ),
        }
    }

    /// Build a missing-coverage feedback message.
    ///
    /// Mirrors CoverUp: "The test runs but lines X still don't execute."
    pub fn missing_coverage_prompt(&self, gap: &CoverageGap) -> LlmMessage {
        let lines_desc = Self::format_uncovered_lines(gap);

        LlmMessage {
            role: LlmRole::User,
            content: format!(
                "The test runs but {lines_desc} still don't execute. \
                 Modify the test to cover them."
            ),
        }
    }

    /// CoverUp-style refinement loop for a single coverage gap.
    ///
    /// Orchestrates the generate → run → measure → refine loop using callbacks
    /// so it is fully testable without real LLM or execution dependencies.
    ///
    /// # Arguments
    /// * `gap`      - The coverage gap to fill.
    /// * `llm_call` - `fn(messages) -> Result<String>` — returns generated test code.
    /// * `run_test` - `fn(test_code) -> TestResult` — executes the test and reports outcome.
    ///
    /// # Returns
    /// `Ok(Some(attempt))` if a successful test was found within `max_attempts`.
    /// `Ok(None)` if all attempts were exhausted without success.
    pub fn fill_gap<F, G>(
        &self,
        gap: &CoverageGap,
        llm_call: F,
        run_test: G,
    ) -> Result<Option<SynthAttempt>>
    where
        F: Fn(&[LlmMessage]) -> Result<String>,
        G: Fn(&str) -> TestResult,
    {
        let mut messages = self.initial_prompt(gap);

        for attempt_number in 1..=self.config.max_attempts {
            // 1. Ask the LLM for test code.
            let test_code = llm_call(&messages)?;

            // Append the LLM's response to conversation history.
            messages.push(LlmMessage {
                role: LlmRole::Assistant,
                content: test_code.clone(),
            });

            // 2. Execute the test.
            match run_test(&test_code) {
                TestResult::Success(branches) => {
                    return Ok(Some(SynthAttempt {
                        test_code,
                        coverage_delta: branches,
                        error: None,
                        attempt_number,
                    }));
                }
                TestResult::Error(err) => {
                    // Add error feedback and retry.
                    messages.push(self.error_prompt(&err));
                }
                TestResult::NoCoverageGain => {
                    // Add missing-coverage feedback and retry.
                    messages.push(self.missing_coverage_prompt(gap));
                }
            }
        }

        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_gap() -> CoverageGap {
        CoverageGap {
            file_path: "test.py".into(),
            target_line: 5,
            function_name: None,
            source_segment: "x = 1\n".into(),
            uncovered_lines: vec![5],
        }
    }

    #[test]
    fn initial_prompt_includes_source_and_gap() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = CoverageGap {
            file_path: "app.py".into(),
            target_line: 10,
            function_name: Some("process".into()),
            source_segment: "def process(x):\n    if x > 0:\n        return x\n".into(),
            uncovered_lines: vec![12],
        };
        let messages = synth.initial_prompt(&gap);
        assert!(
            messages.len() >= 2,
            "expected at least system + user message"
        );
        assert_eq!(messages[0].role, LlmRole::System);
        assert!(messages[1].content.contains("app.py"));
    }

    #[test]
    fn initial_prompt_system_role() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = make_gap();
        let messages = synth.initial_prompt(&gap);
        assert_eq!(messages[0].role, LlmRole::System);
    }

    #[test]
    fn initial_prompt_contains_function_name() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = CoverageGap {
            file_path: "foo.py".into(),
            target_line: 3,
            function_name: Some("compute".into()),
            source_segment: "def compute(): pass\n".into(),
            uncovered_lines: vec![3],
        };
        let messages = synth.initial_prompt(&gap);
        assert!(messages[1].content.contains("compute"));
    }

    #[test]
    fn error_prompt_includes_error_message() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let msg = synth.error_prompt("NameError: name 'foo' is not defined");
        assert_eq!(msg.role, LlmRole::User);
        assert!(msg.content.contains("NameError"));
    }

    #[test]
    fn missing_coverage_prompt_includes_lines() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = CoverageGap {
            file_path: "x.py".into(),
            target_line: 7,
            function_name: None,
            source_segment: String::new(),
            uncovered_lines: vec![7, 8, 9],
        };
        let msg = synth.missing_coverage_prompt(&gap);
        assert_eq!(msg.role, LlmRole::User);
        assert!(msg.content.contains("7"));
    }

    #[test]
    fn fill_gap_succeeds_on_first_try() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = make_gap();
        let result = synth
            .fill_gap(
                &gap,
                |_msgs| Ok("def test_foo(): assert True".into()),
                |_code| TestResult::Success(vec![BranchId::new(1, 5, 0, 0)]),
            )
            .unwrap();
        assert!(result.is_some());
        let attempt = result.unwrap();
        assert_eq!(attempt.attempt_number, 1);
        assert_eq!(attempt.coverage_delta.len(), 1);
    }

    #[test]
    fn fill_gap_retries_on_error() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = make_gap();
        let call_count = std::cell::Cell::new(0u32);
        let result = synth
            .fill_gap(
                &gap,
                |_msgs| {
                    call_count.set(call_count.get() + 1);
                    Ok("def test(): pass".into())
                },
                |_code| {
                    if call_count.get() < 2 {
                        TestResult::Error("SyntaxError".into())
                    } else {
                        TestResult::Success(vec![])
                    }
                },
            )
            .unwrap();
        assert!(result.is_some());
        assert!(call_count.get() >= 2, "should have retried at least once");
    }

    #[test]
    fn fill_gap_returns_none_after_max_attempts() {
        let config = LlmConfig {
            max_attempts: 2,
            ..Default::default()
        };
        let synth = LlmSynthesizer::new(config);
        let gap = make_gap();
        let result = synth
            .fill_gap(
                &gap,
                |_| Ok("bad test".into()),
                |_| TestResult::NoCoverageGain,
            )
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn fill_gap_accumulates_messages() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = make_gap();
        let msg_counts = std::cell::RefCell::new(vec![]);
        let call_count = std::cell::Cell::new(0u32);
        synth
            .fill_gap(
                &gap,
                |msgs| {
                    msg_counts.borrow_mut().push(msgs.len());
                    call_count.set(call_count.get() + 1);
                    Ok("test".into())
                },
                |_| {
                    if call_count.get() < 3 {
                        TestResult::Error("err".into())
                    } else {
                        TestResult::Success(vec![])
                    }
                },
            )
            .unwrap();

        let counts = msg_counts.borrow();
        assert!(counts.len() >= 2, "should have made at least 2 LLM calls");
        assert!(
            counts[1] > counts[0],
            "second call should have more context than first; got {:?}",
            *counts
        );
    }

    #[test]
    fn fill_gap_no_coverage_gain_then_success() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = make_gap();
        let call_count = std::cell::Cell::new(0u32);
        let result = synth
            .fill_gap(
                &gap,
                |_msgs| {
                    call_count.set(call_count.get() + 1);
                    Ok("def test(): pass".into())
                },
                |_code| {
                    if call_count.get() < 2 {
                        TestResult::NoCoverageGain
                    } else {
                        TestResult::Success(vec![BranchId::new(1, 5, 0, 0)])
                    }
                },
            )
            .unwrap();
        assert!(result.is_some());
        let attempt = result.unwrap();
        assert_eq!(attempt.attempt_number, 2);
    }

    #[test]
    fn llm_config_defaults() {
        let cfg = LlmConfig::default();
        assert_eq!(cfg.max_attempts, 3);
        assert!(!cfg.model.is_empty());
        assert!(cfg.temperature > 0.0);
    }

    #[test]
    fn fill_gap_error_propagates_from_llm() {
        use apex_core::error::ApexError;
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = make_gap();
        let result = synth.fill_gap(
            &gap,
            |_msgs| Err(ApexError::Agent("network timeout".into())),
            |_code| TestResult::Success(vec![]),
        );
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Gap-filling tests: uncovered branches
    // -----------------------------------------------------------------------

    /// `format_uncovered_lines` with an empty `uncovered_lines` list falls back
    /// to formatting with `target_line` (line 114 branch).
    #[test]
    fn format_uncovered_lines_empty_uses_target_line() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = CoverageGap {
            file_path: "empty.py".into(),
            target_line: 42,
            function_name: None,
            source_segment: String::new(),
            uncovered_lines: vec![], // <-- triggers the is_empty() branch
        };
        let messages = synth.initial_prompt(&gap);
        // The user message must reference "line 42" (the target_line fallback).
        assert!(
            messages[1].content.contains("line 42"),
            "expected 'line 42' in prompt, got: {}",
            messages[1].content
        );
    }

    /// `fill_gap` with `Success(vec![])` returned immediately on the first call.
    /// The `vec![]` branch of `TestResult::Success` (line 465 callback path)
    /// must be reachable when the run_test callback is actually invoked.
    #[test]
    fn fill_gap_success_with_empty_coverage_delta() {
        let synth = LlmSynthesizer::new(LlmConfig::default());
        let gap = make_gap();
        let result = synth
            .fill_gap(
                &gap,
                |_msgs| Ok("def test_nothing(): pass".into()),
                |_code| TestResult::Success(vec![]),
            )
            .unwrap();
        let attempt = result.expect("expected a successful attempt");
        assert_eq!(attempt.attempt_number, 1);
        assert!(
            attempt.coverage_delta.is_empty(),
            "coverage_delta should be empty"
        );
    }
}
