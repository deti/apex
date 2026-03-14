//! Selective symbolic execution — determines which functions should be
//! analyzed symbolically vs executed concretely, based on taint information,
//! security sink presence, and branch priority data.

use std::collections::{HashMap, HashSet};

/// Reason a function was included in the symbolic scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeReason {
    HandlesTaintedInput,
    ContainsSecuritySink,
    HighPriorityCoverage,
    ManuallyIncluded,
}

/// Which functions should be analyzed symbolically vs concretely.
#[derive(Debug, Clone)]
pub struct SymbolicScope {
    pub functions: HashSet<String>,
    pub reasons: HashMap<String, Vec<ScopeReason>>,
}

impl SymbolicScope {
    pub fn new() -> Self {
        Self {
            functions: HashSet::new(),
            reasons: HashMap::new(),
        }
    }

    pub fn include(&mut self, function: &str, reason: ScopeReason) {
        self.functions.insert(function.to_string());
        self.reasons
            .entry(function.to_string())
            .or_default()
            .push(reason);
    }

    pub fn is_in_scope(&self, function: &str) -> bool {
        self.functions.contains(function)
    }

    pub fn scope_size(&self) -> usize {
        self.functions.len()
    }
}

impl Default for SymbolicScope {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for scope selection.
#[derive(Debug, Clone)]
pub struct ScopeConfig {
    /// Minimum priority to include (0.0-1.0).
    pub priority_threshold: f64,
    /// Functions to always include regardless of analysis.
    pub manual_includes: Vec<String>,
    /// Maximum number of functions in scope.
    pub max_scope_size: usize,
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self {
            priority_threshold: 0.5,
            manual_includes: vec![],
            max_scope_size: 100,
        }
    }
}

/// A simplified taint summary for scope selection.
#[derive(Debug, Clone)]
pub struct FunctionTaintInfo {
    pub function_name: String,
    pub has_unsanitized_flows: bool,
    pub has_security_sinks: bool,
}

/// A branch priority entry.
#[derive(Debug, Clone)]
pub struct BranchPriority {
    pub function_name: String,
    pub priority: f64,
}

/// Build the symbolic scope from taint info and priority data.
pub fn select_scope(
    taint_info: &[FunctionTaintInfo],
    priorities: &[BranchPriority],
    config: &ScopeConfig,
) -> SymbolicScope {
    let mut scope = SymbolicScope::new();

    // Include functions that handle tainted input
    for info in taint_info {
        if info.has_unsanitized_flows {
            scope.include(&info.function_name, ScopeReason::HandlesTaintedInput);
        }
        if info.has_security_sinks {
            scope.include(&info.function_name, ScopeReason::ContainsSecuritySink);
        }
    }

    // Include functions with high-priority uncovered branches
    for bp in priorities {
        if bp.priority > config.priority_threshold {
            scope.include(&bp.function_name, ScopeReason::HighPriorityCoverage);
        }
    }

    // Include manually specified functions
    for name in &config.manual_includes {
        scope.include(name, ScopeReason::ManuallyIncluded);
    }

    scope
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taint(name: &str, unsanitized: bool, sinks: bool) -> FunctionTaintInfo {
        FunctionTaintInfo {
            function_name: name.to_string(),
            has_unsanitized_flows: unsanitized,
            has_security_sinks: sinks,
        }
    }

    fn priority(name: &str, p: f64) -> BranchPriority {
        BranchPriority {
            function_name: name.to_string(),
            priority: p,
        }
    }

    #[test]
    fn scope_includes_tainted_functions() {
        let taint_info = vec![taint("handle_request", true, false)];
        let scope = select_scope(&taint_info, &[], &ScopeConfig::default());
        assert!(scope.is_in_scope("handle_request"));
    }

    #[test]
    fn scope_excludes_clean_functions() {
        let taint_info = vec![taint("safe_helper", false, false)];
        let scope = select_scope(&taint_info, &[], &ScopeConfig::default());
        assert!(!scope.is_in_scope("safe_helper"));
    }

    #[test]
    fn scope_includes_security_sinks() {
        let taint_info = vec![taint("exec_query", false, true)];
        let scope = select_scope(&taint_info, &[], &ScopeConfig::default());
        assert!(scope.is_in_scope("exec_query"));
    }

    #[test]
    fn scope_includes_high_priority_branches() {
        let priorities = vec![priority("complex_branch", 0.8)];
        let scope = select_scope(&[], &priorities, &ScopeConfig::default());
        assert!(scope.is_in_scope("complex_branch"));
    }

    #[test]
    fn scope_excludes_low_priority_branches() {
        let priorities = vec![priority("boring_branch", 0.3)];
        let scope = select_scope(&[], &priorities, &ScopeConfig::default());
        assert!(!scope.is_in_scope("boring_branch"));
    }

    #[test]
    fn scope_includes_manual_functions() {
        let config = ScopeConfig {
            manual_includes: vec!["my_func".to_string()],
            ..Default::default()
        };
        let scope = select_scope(&[], &[], &config);
        assert!(scope.is_in_scope("my_func"));
    }

    #[test]
    fn scope_priority_threshold_configurable() {
        let priorities = vec![priority("medium_priority", 0.4)];

        // Default threshold 0.5 excludes it
        let scope = select_scope(&[], &priorities, &ScopeConfig::default());
        assert!(!scope.is_in_scope("medium_priority"));

        // Lower threshold 0.3 includes it
        let config = ScopeConfig {
            priority_threshold: 0.3,
            ..Default::default()
        };
        let scope = select_scope(&[], &priorities, &config);
        assert!(scope.is_in_scope("medium_priority"));
    }

    #[test]
    fn scope_reasons_tracked() {
        let taint_info = vec![taint("handler", true, false)];
        let scope = select_scope(&taint_info, &[], &ScopeConfig::default());
        let reasons = scope.reasons.get("handler").unwrap();
        assert_eq!(reasons, &[ScopeReason::HandlesTaintedInput]);
    }

    #[test]
    fn scope_multiple_reasons_for_same_function() {
        let taint_info = vec![taint("dangerous_handler", true, true)];
        let priorities = vec![priority("dangerous_handler", 0.9)];
        let scope = select_scope(&taint_info, &priorities, &ScopeConfig::default());

        let reasons = scope.reasons.get("dangerous_handler").unwrap();
        assert_eq!(reasons.len(), 3);
        assert!(reasons.contains(&ScopeReason::HandlesTaintedInput));
        assert!(reasons.contains(&ScopeReason::ContainsSecuritySink));
        assert!(reasons.contains(&ScopeReason::HighPriorityCoverage));
    }

    #[test]
    fn empty_inputs_empty_scope() {
        let scope = select_scope(&[], &[], &ScopeConfig::default());
        assert_eq!(scope.scope_size(), 0);
        assert!(!scope.is_in_scope("anything"));
    }
}
