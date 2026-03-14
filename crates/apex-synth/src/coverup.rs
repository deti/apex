use async_trait::async_trait;
use apex_core::types::BranchId;
use apex_core::error::Result;
use crate::strategy::{GapHistory, PromptStrategy};

pub struct CoverUpStrategy;

impl CoverUpStrategy {
    pub fn new() -> Self { Self }
}

impl Default for CoverUpStrategy {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl PromptStrategy for CoverUpStrategy {
    fn name(&self) -> &str { "coverup" }

    async fn build_prompt(&self, gap: &BranchId, history: &GapHistory, source: &str) -> Result<String> {
        let key = format!("{}:{}:{}:{}", gap.file_id, gap.line, gap.col, gap.direction);
        let attempts = history.attempt_count(&key);
        let retry_hint = if attempts > 0 {
            format!("\nNote: {} previous attempt(s) failed. Try a different approach.", attempts)
        } else {
            String::new()
        };
        Ok(format!(
            "Write a test that covers line {} (direction {}) of the following source code.{}\n\n```\n{}\n```",
            gap.line, gap.direction, retry_hint, source
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::GapHistory;
    use apex_core::types::BranchId;

    #[test]
    fn strategy_name() {
        assert_eq!(CoverUpStrategy::new().name(), "coverup");
    }

    #[tokio::test]
    async fn prompt_contains_branch_info() {
        let s = CoverUpStrategy::new();
        let branch = BranchId::new(1, 42, 0, 0);
        let history = GapHistory::new();
        let prompt = s.build_prompt(&branch, &history, "def foo(x):\n    if x > 0:\n        return x").await.unwrap();
        assert!(prompt.contains("42")); // line number
        assert!(prompt.contains("def foo"));
    }

    #[tokio::test]
    async fn prompt_includes_retry_hint_on_prior_failure() {
        let s = CoverUpStrategy::new();
        let branch = BranchId::new(1, 5, 0, 0);
        let mut history = GapHistory::new();
        history.record_attempt("1:5:0:0", false);
        let prompt = s.build_prompt(&branch, &history, "pass").await.unwrap();
        assert!(prompt.to_lowercase().contains("previous") || prompt.to_lowercase().contains("attempt") || prompt.to_lowercase().contains("retry"));
    }
}
