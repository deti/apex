//! Shared static concolic strategy — reads source, extracts conditions
//! via a pluggable parser, and generates boundary seeds without runtime
//! tracing.

use crate::boundary::boundary_values;
use crate::condition_tree::ConditionTree;
use apex_core::{
    error::Result,
    traits::Strategy,
    types::{ExecutionResult, ExplorationContext, InputSeed, SeedOrigin},
};
use async_trait::async_trait;
use std::sync::Mutex;

/// A condition parser: takes source text, returns `(line_number, condition)` pairs.
pub type ConditionParser = fn(&str) -> Vec<(u32, ConditionTree)>;

/// Cached condition entries per file.
type ConditionCache = Vec<(String, Vec<(u32, ConditionTree)>)>;

/// Static concolic strategy that extracts conditions from source code and
/// generates boundary seeds without runtime tracing.
pub struct StaticConcolicStrategy {
    name: String,
    parser: ConditionParser,
    cache: Mutex<ConditionCache>,
}

impl StaticConcolicStrategy {
    pub fn new(name: impl Into<String>, parser: ConditionParser) -> Self {
        Self {
            name: name.into(),
            parser,
            cache: Mutex::new(Vec::new()),
        }
    }

    /// Parse source and return boundary seeds for all extracted conditions.
    pub fn seeds_from_source(&self, source: &str) -> Vec<String> {
        let conditions = (self.parser)(source);
        let mut seeds = Vec::new();
        for (_line, tree) in &conditions {
            seeds.extend(boundary_values(tree));
        }
        seeds.dedup();
        seeds
    }

    /// Parse and cache conditions for a file.
    fn parse_file(&self, path: &str, source: &str) -> Vec<(u32, ConditionTree)> {
        let conditions = (self.parser)(source);
        if let Ok(mut cache) = self.cache.lock() {
            if !cache.iter().any(|(p, _)| p == path) {
                cache.push((path.to_string(), conditions.clone()));
            }
        }
        conditions
    }
}

#[async_trait]
impl Strategy for StaticConcolicStrategy {
    fn name(&self) -> &str {
        &self.name
    }

    async fn suggest_inputs(&self, ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        let source_path = ctx.target.root.to_string_lossy().to_string();
        let source = match std::fs::read_to_string(&source_path) {
            Ok(s) => s,
            Err(_) => return Ok(vec![]),
        };

        let conditions = self.parse_file(&source_path, &source);
        let mut inputs = Vec::new();

        for (_line, tree) in &conditions {
            let values = boundary_values(tree);
            for val in values {
                let seed = InputSeed::new(val.into_bytes(), SeedOrigin::Concolic);
                inputs.push(seed);
            }
        }

        Ok(inputs)
    }

    async fn observe(&self, _result: &ExecutionResult) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition_tree::{CompareOp, Expr};

    fn dummy_parser(source: &str) -> Vec<(u32, ConditionTree)> {
        let mut results = Vec::new();
        for (i, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.contains("x > 10") {
                results.push((
                    (i + 1) as u32,
                    ConditionTree::Compare {
                        left: Box::new(Expr::Variable("x".into())),
                        op: CompareOp::Gt,
                        right: Box::new(Expr::IntLiteral(10)),
                    },
                ));
            }
        }
        results
    }

    #[test]
    fn strategy_name() {
        let s = StaticConcolicStrategy::new("test-concolic", dummy_parser);
        assert_eq!(s.name(), "test-concolic");
    }

    #[test]
    fn seeds_from_source_basic() {
        let s = StaticConcolicStrategy::new("test", dummy_parser);
        let seeds = s.seeds_from_source("if x > 10 {\n    do_thing();\n}");
        assert!(!seeds.is_empty());
        assert!(seeds.contains(&"10".to_string()));
    }

    #[test]
    fn seeds_from_source_no_conditions() {
        let s = StaticConcolicStrategy::new("test", dummy_parser);
        let seeds = s.seeds_from_source("let y = 42;");
        assert!(seeds.is_empty());
    }

    #[test]
    fn parse_file_caches() {
        let s = StaticConcolicStrategy::new("test", dummy_parser);
        let c1 = s.parse_file("test.rs", "if x > 10 {}");
        assert_eq!(c1.len(), 1);
        let _c2 = s.parse_file("test.rs", "if x > 10 {}");
        let cache = s.cache.lock().unwrap();
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn empty_parser_returns_no_seeds() {
        fn empty_parser(_source: &str) -> Vec<(u32, ConditionTree)> {
            vec![]
        }
        let s = StaticConcolicStrategy::new("empty", empty_parser);
        let seeds = s.seeds_from_source("any source code");
        assert!(seeds.is_empty());
    }
}
