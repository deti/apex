//! Type annotation–based taint propagation.
//!
//! Propagates taint through type annotations (e.g., `str` parameters from HTTP
//! handlers are always tainted), catching injection paths that call-graph taint
//! analysis alone misses. Inspired by arXiv:2504.18529.

use std::collections::HashMap;

/// A rule that marks specific fields of a type annotation as tainted.
#[derive(Debug, Clone)]
pub struct TypeTaintRule {
    pub annotation: String,
    pub taint_fields: Vec<String>,
}

/// Analyzer that maps type annotations to their tainted fields.
#[derive(Debug, Default)]
pub struct TypeTaintAnalyzer {
    rules: HashMap<String, Vec<String>>,
}

impl TypeTaintAnalyzer {
    pub fn new() -> Self {
        Default::default()
    }

    /// Register a taint rule for the given type annotation.
    pub fn add_rule(&mut self, rule: TypeTaintRule) {
        self.rules.insert(rule.annotation, rule.taint_fields);
    }

    /// Return `true` if `field` on `type_name` is tainted by a registered rule.
    pub fn is_tainted(&self, type_name: &str, field: &str) -> bool {
        self.rules
            .get(type_name)
            .map_or(false, |fields| fields.iter().any(|f| f == field))
    }

    /// Number of rules currently registered.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_string_param_is_tainted() {
        let mut analyzer = TypeTaintAnalyzer::new();
        analyzer.add_rule(TypeTaintRule {
            annotation: "HttpRequest".into(),
            taint_fields: vec!["body".into(), "query_params".into()],
        });
        assert!(analyzer.is_tainted("HttpRequest", "body"));
        assert!(!analyzer.is_tainted("HttpRequest", "headers"));
    }

    #[test]
    fn unknown_type_not_tainted() {
        let analyzer = TypeTaintAnalyzer::new();
        assert!(!analyzer.is_tainted("MyClass", "field"));
    }

    #[test]
    fn rule_count_matches_added() {
        let mut analyzer = TypeTaintAnalyzer::new();
        analyzer.add_rule(TypeTaintRule {
            annotation: "A".into(),
            taint_fields: vec!["f".into()],
        });
        analyzer.add_rule(TypeTaintRule {
            annotation: "B".into(),
            taint_fields: vec!["g".into()],
        });
        assert_eq!(analyzer.rule_count(), 2);
    }
}
