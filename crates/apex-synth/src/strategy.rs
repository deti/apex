use std::collections::HashMap;
use async_trait::async_trait;
use apex_core::types::BranchId;
use apex_core::error::Result;

/// A pluggable prompt construction strategy for LLM test synthesis.
#[async_trait]
pub trait PromptStrategy: Send + Sync {
    fn name(&self) -> &str;
    async fn build_prompt(&self, gap: &BranchId, history: &GapHistory, source: &str) -> Result<String>;
}

#[derive(Debug, Default)]
pub struct GapHistory {
    attempts: HashMap<String, Vec<bool>>,
}

impl GapHistory {
    pub fn new() -> Self { Default::default() }
    pub fn record_attempt(&mut self, key: &str, succeeded: bool) {
        self.attempts.entry(key.to_string()).or_default().push(succeeded);
    }
    pub fn attempt_count(&self, key: &str) -> usize {
        self.attempts.get(key).map_or(0, |v| v.len())
    }
    pub fn last_succeeded(&self, key: &str) -> bool {
        self.attempts.get(key).and_then(|v| v.last()).copied().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_history_records_attempts() {
        let mut h = GapHistory::new();
        h.record_attempt("branch:1:10:0", false);
        h.record_attempt("branch:1:10:0", true);
        assert_eq!(h.attempt_count("branch:1:10:0"), 2);
        assert!(h.last_succeeded("branch:1:10:0"));
    }

    #[test]
    fn gap_history_unknown_key_returns_defaults() {
        let h = GapHistory::new();
        assert_eq!(h.attempt_count("x"), 0);
        assert!(!h.last_succeeded("x"));
    }
}
