//! Configurable taint rules for CPG taint analysis.
//!
//! Provides built-in rule sets for Python and JavaScript, plus a merge
//! mechanism for user-defined custom rules.

use serde::{Deserialize, Serialize};

/// A set of taint analysis rules: sources, sinks, and sanitizers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintRuleSet {
    pub sources: Vec<String>,
    pub sinks: Vec<String>,
    pub sanitizers: Vec<String>,
}

impl TaintRuleSet {
    /// Create an empty rule set.
    pub fn empty() -> Self {
        TaintRuleSet {
            sources: Vec::new(),
            sinks: Vec::new(),
            sanitizers: Vec::new(),
        }
    }

    /// Default Python taint rules.
    pub fn python_defaults() -> Self {
        TaintRuleSet {
            sources: vec![
                "request.args".into(),
                "request.form".into(),
                "request.data".into(),
                "request.json".into(),
                "request.get_json".into(),
                "sys.argv".into(),
                "input".into(),
                "os.environ".into(),
            ],
            sinks: vec![
                "cursor.execute".into(),
                "conn.execute".into(),
                "db.execute".into(),
                "session.execute".into(),
                "cursor.executemany".into(),
                "conn.executemany".into(),
                "os.system".into(),
                "os.popen".into(),
                "subprocess.call".into(),
                "subprocess.run".into(),
                "eval".into(),
                "exec".into(),
                "open".into(),
                "render_template_string".into(),
            ],
            sanitizers: vec![
                "html.escape".into(),
                "markupsafe.escape".into(),
                "shlex.quote".into(),
                "bleach.clean".into(),
                "bleach.sanitize".into(),
                "parameterize".into(),
            ],
        }
    }

    /// Default JavaScript taint rules.
    pub fn javascript_defaults() -> Self {
        TaintRuleSet {
            sources: vec![
                "req.body".into(),
                "req.params".into(),
                "req.query".into(),
                "req.headers".into(),
                "document.location".into(),
                "window.location".into(),
                "process.argv".into(),
                "process.env".into(),
            ],
            sinks: vec![
                "eval".into(),
                "exec".into(),
                "execSync".into(),
                "innerHTML".into(),
                "document.write".into(),
                "child_process.exec".into(),
                "db.query".into(),
                "pool.query".into(),
                "fs.readFile".into(),
                "fs.writeFile".into(),
            ],
            sanitizers: vec![
                "escape".into(),
                "sanitize".into(),
                "encodeURIComponent".into(),
                "DOMPurify.sanitize".into(),
                "validator.escape".into(),
            ],
        }
    }

    /// Merge another rule set into this one (additive, no duplicates).
    pub fn merge(&mut self, other: &TaintRuleSet) {
        for src in &other.sources {
            if !self.sources.contains(src) {
                self.sources.push(src.clone());
            }
        }
        for sink in &other.sinks {
            if !self.sinks.contains(sink) {
                self.sinks.push(sink.clone());
            }
        }
        for san in &other.sanitizers {
            if !self.sanitizers.contains(san) {
                self.sanitizers.push(san.clone());
            }
        }
    }

    /// Check if a function name matches any source pattern.
    ///
    /// Uses exact or dotted-suffix matching: `name` must equal a rule exactly,
    /// or end with `.<rule>`.  This avoids false positives where a substring
    /// like `"input"` would match `"input_sanitized"`.
    pub fn is_source(&self, name: &str) -> bool {
        self.sources
            .iter()
            .any(|s| name == s.as_str() || name.ends_with(&format!(".{s}")))
    }

    /// Check if a function name matches any sink pattern.
    ///
    /// Uses exact or dotted-suffix matching to prevent false positives such as
    /// `"executor"` matching the `"exec"` rule.
    pub fn is_sink(&self, name: &str) -> bool {
        self.sinks
            .iter()
            .any(|s| name == s.as_str() || name.ends_with(&format!(".{s}")))
    }

    /// Check if a function name matches any sanitizer pattern.
    ///
    /// Uses exact or dotted-suffix matching to prevent false positives such as
    /// `"cleanup_data"` matching the `"clean"` rule.
    pub fn is_sanitizer(&self, name: &str) -> bool {
        self.sanitizers
            .iter()
            .any(|s| name == s.as_str() || name.ends_with(&format!(".{s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_python_rules_have_sources() {
        let rules = TaintRuleSet::python_defaults();
        assert!(!rules.sources.is_empty());
        assert!(rules.sources.iter().any(|s| s.contains("request")));
    }

    #[test]
    fn default_python_rules_have_sinks() {
        let rules = TaintRuleSet::python_defaults();
        assert!(!rules.sinks.is_empty());
        assert!(rules.sinks.iter().any(|s| s.ends_with("execute")));
    }

    #[test]
    fn default_python_rules_have_sanitizers() {
        let rules = TaintRuleSet::python_defaults();
        assert!(!rules.sanitizers.is_empty());
    }

    #[test]
    fn javascript_rules_have_sources() {
        let rules = TaintRuleSet::javascript_defaults();
        assert!(!rules.sources.is_empty());
        assert!(rules.sources.iter().any(|s| s.contains("req")));
    }

    #[test]
    fn javascript_rules_have_sinks() {
        let rules = TaintRuleSet::javascript_defaults();
        assert!(!rules.sinks.is_empty());
    }

    #[test]
    fn custom_rules_merge() {
        let mut rules = TaintRuleSet::python_defaults();
        let custom = TaintRuleSet {
            sources: vec!["custom_source".into()],
            sinks: vec!["custom_sink".into()],
            sanitizers: vec![],
        };
        rules.merge(&custom);
        assert!(rules.sources.contains(&"custom_source".to_string()));
        assert!(rules.sinks.contains(&"custom_sink".to_string()));
    }

    #[test]
    fn is_source_checks_membership() {
        let rules = TaintRuleSet {
            sources: vec!["request.args".into()],
            sinks: vec![],
            sanitizers: vec![],
        };
        assert!(rules.is_source("request.args"));
        assert!(!rules.is_source("safe_func"));
    }

    #[test]
    fn is_sink_checks_membership() {
        let rules = TaintRuleSet {
            sources: vec![],
            sinks: vec!["execute".into()],
            sanitizers: vec![],
        };
        assert!(rules.is_sink("execute"));
        assert!(!rules.is_sink("safe_func"));
    }

    #[test]
    fn is_sanitizer_checks_membership() {
        let rules = TaintRuleSet {
            sources: vec![],
            sinks: vec![],
            sanitizers: vec!["escape".into()],
        };
        assert!(rules.is_sanitizer("escape"));
        assert!(!rules.is_sanitizer("noop"));
    }

    #[test]
    fn is_sink_rejects_substring_match() {
        let rules = TaintRuleSet::python_defaults();
        assert!(rules.is_sink("exec"));
        assert!(!rules.is_sink("executor"));
        assert!(!rules.is_sink("execute_callback"));
    }

    #[test]
    fn is_sanitizer_rejects_substring_match() {
        let rules = TaintRuleSet::python_defaults();
        assert!(rules.is_sanitizer("shlex.quote"));
        assert!(!rules.is_sanitizer("cleanup_data"));
        assert!(!rules.is_sanitizer("my_escape_plan"));
    }

    #[test]
    fn is_source_rejects_substring_match() {
        let rules = TaintRuleSet::python_defaults();
        assert!(rules.is_source("request.args"));
        assert!(!rules.is_source("my_request_args_parser"));
        assert!(!rules.is_source("input_handler"));
    }

    #[test]
    fn dotted_suffix_matching_works() {
        let rules = TaintRuleSet::python_defaults();
        // "cursor.execute" is a sink rule; "db.cursor.execute" should match via .suffix
        assert!(rules.is_sink("cursor.execute"));
        assert!(rules.is_sink("db.cursor.execute"));
        // "eval" exact match
        assert!(rules.is_sink("eval"));
        // dotted suffix for sanitizer
        assert!(rules.is_sanitizer("markupsafe.escape"));
        assert!(rules.is_sanitizer("jinja2.markupsafe.escape"));
    }

    #[test]
    fn empty_rules() {
        let rules = TaintRuleSet::empty();
        assert!(rules.sources.is_empty());
        assert!(rules.sinks.is_empty());
        assert!(rules.sanitizers.is_empty());
    }
}
