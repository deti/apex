//! HUNT+INTEL integration helpers.
//!
//! Converts security findings into priority hints for the hunt phase.
//! Uncovered branches within a configurable line window of a security finding
//! receive a boost to their `heuristic` score so the orchestrator explores them
//! before less suspicious code.
//!
//! # Usage
//!
//! ```ignore
//! let hints = HuntHints::from_findings(&report.findings, HuntHintConfig::default());
//! // Pass hints to the hunt orchestrator when scoring uncovered branches.
//! let boost = hints.security_boost_for(&file_path, branch_line);
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::finding::{Finding, Severity};

/// Configuration for how findings are translated into hunt priority boosts.
#[derive(Debug, Clone)]
pub struct HuntHintConfig {
    /// Lines within this window of a finding get the boost applied.
    /// Default: 10 lines before/after.
    pub proximity_window: u32,
    /// Boost added to the heuristic score for critical findings.
    pub critical_boost: f64,
    /// Boost added to the heuristic score for high findings.
    pub high_boost: f64,
    /// Boost added to the heuristic score for medium findings.
    pub medium_boost: f64,
    /// Boost added to the heuristic score for low / info findings.
    pub low_boost: f64,
}

impl Default for HuntHintConfig {
    fn default() -> Self {
        Self {
            proximity_window: 10,
            critical_boost: 0.8,
            high_boost: 0.5,
            medium_boost: 0.25,
            low_boost: 0.1,
        }
    }
}

/// A single priority hint derived from a security finding.
#[derive(Debug, Clone)]
pub struct HuntHint {
    /// Relative or absolute path to the affected source file.
    pub file: PathBuf,
    /// Center line of the finding (if known).
    pub line: Option<u32>,
    /// Priority boost to add to the heuristic score for nearby branches.
    pub boost: f64,
}

/// Collection of priority hints for the hunt phase.
#[derive(Debug, Default, Clone)]
pub struct HuntHints {
    /// Map from canonicalized file path to list of (center_line, boost).
    hints: HashMap<PathBuf, Vec<(Option<u32>, f64)>>,
    config: HuntHintConfig,
}

impl HuntHints {
    /// Build hints from a slice of findings using the provided config.
    pub fn from_findings(findings: &[Finding], config: HuntHintConfig) -> Self {
        let mut hints: HashMap<PathBuf, Vec<(Option<u32>, f64)>> = HashMap::new();

        for f in findings {
            let boost = match f.severity {
                Severity::Critical => config.critical_boost,
                Severity::High => config.high_boost,
                Severity::Medium => config.medium_boost,
                Severity::Low | Severity::Info => config.low_boost,
            };
            hints
                .entry(f.file.clone())
                .or_default()
                .push((f.line, boost));
        }

        Self { hints, config }
    }

    /// Returns the aggregate security boost for a branch at `file:line`.
    ///
    /// Sums boosts for all findings whose proximity window overlaps with `branch_line`.
    /// Returns 0.0 if the branch is not near any security finding.
    pub fn security_boost_for(&self, file: &Path, branch_line: u32) -> f64 {
        let window = self.config.proximity_window;

        // Try both exact match and filename-only match so that relative paths
        // like `src/auth.rs` match absolute paths like `/project/src/auth.rs`.
        let file_hints = self.hints.get(file).or_else(|| {
            let file_name = file.file_name()?;
            self.hints.iter().find_map(|(k, v)| {
                if k.file_name() == Some(file_name) {
                    Some(v)
                } else {
                    None
                }
            })
        });

        let Some(entries) = file_hints else {
            return 0.0;
        };

        entries
            .iter()
            .map(|(center, boost)| {
                let in_window = match center {
                    Some(c) => {
                        let lo = c.saturating_sub(window);
                        let hi = c.saturating_add(window);
                        branch_line >= lo && branch_line <= hi
                    }
                    // If the finding has no line, treat the whole file as in-window.
                    None => true,
                };
                if in_window {
                    *boost
                } else {
                    0.0
                }
            })
            .fold(0.0_f64, |acc, b| acc + b)
    }

    /// Returns true if any finding affects the given file.
    pub fn has_findings_in(&self, file: &Path) -> bool {
        self.hints.contains_key(file)
            || self.hints.keys().any(|k| k.file_name() == file.file_name())
    }

    /// Returns all hints as a flat `Vec<HuntHint>` for serialization.
    pub fn to_vec(&self) -> Vec<HuntHint> {
        self.hints
            .iter()
            .flat_map(|(file, entries)| {
                entries.iter().map(move |(line, boost)| HuntHint {
                    file: file.clone(),
                    line: *line,
                    boost: *boost,
                })
            })
            .collect()
    }

    /// Returns the number of distinct files with security hints.
    pub fn file_count(&self) -> usize {
        self.hints.len()
    }

    /// Returns the total number of hints (one per finding).
    pub fn hint_count(&self) -> usize {
        self.hints.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Finding, FindingCategory, Severity};
    use std::path::PathBuf;

    fn make_finding(file: &str, line: Option<u32>, severity: Severity) -> Finding {
        Finding {
            id: uuid::Uuid::nil(),
            file: PathBuf::from(file),
            line,
            severity,
            title: "test".into(),
            description: "desc".into(),
            suggestion: "fix it".into(),
            detector: "test-detector".into(),
            category: FindingCategory::Injection,
            evidence: vec![],
            covered: false,
            fix: None,
            explanation: None,
            cwe_ids: vec![],
        }
    }

    #[test]
    fn empty_findings_yield_zero_boost() {
        let hints = HuntHints::from_findings(&[], HuntHintConfig::default());
        assert_eq!(hints.security_boost_for(Path::new("src/auth.rs"), 42), 0.0);
    }

    #[test]
    fn boost_within_proximity_window() {
        let cfg = HuntHintConfig {
            proximity_window: 5,
            ..Default::default()
        };
        let findings = vec![make_finding("src/auth.rs", Some(100), Severity::High)];
        let hints = HuntHints::from_findings(&findings, cfg);

        // Exactly at the finding line — should boost
        assert!(hints.security_boost_for(Path::new("src/auth.rs"), 100) > 0.0);
        // Within window (100 + 5 = 105)
        assert!(hints.security_boost_for(Path::new("src/auth.rs"), 105) > 0.0);
        // Outside window (100 + 6 = 106)
        assert_eq!(hints.security_boost_for(Path::new("src/auth.rs"), 106), 0.0);
    }

    #[test]
    fn boost_below_window() {
        let cfg = HuntHintConfig {
            proximity_window: 5,
            ..Default::default()
        };
        let findings = vec![make_finding("src/auth.rs", Some(100), Severity::High)];
        let hints = HuntHints::from_findings(&findings, cfg);

        // Within window below (100 - 5 = 95)
        assert!(hints.security_boost_for(Path::new("src/auth.rs"), 95) > 0.0);
        // Outside window below (100 - 6 = 94)
        assert_eq!(hints.security_boost_for(Path::new("src/auth.rs"), 94), 0.0);
    }

    #[test]
    fn no_line_boosts_all_lines_in_file() {
        let findings = vec![make_finding("src/auth.rs", None, Severity::Critical)];
        let hints = HuntHints::from_findings(&findings, HuntHintConfig::default());
        assert!(hints.security_boost_for(Path::new("src/auth.rs"), 1) > 0.0);
        assert!(hints.security_boost_for(Path::new("src/auth.rs"), 99999) > 0.0);
    }

    #[test]
    fn severity_determines_boost_magnitude() {
        let cfg = HuntHintConfig::default();
        let findings = vec![
            make_finding("a.rs", Some(10), Severity::Critical),
            make_finding("b.rs", Some(10), Severity::High),
            make_finding("c.rs", Some(10), Severity::Medium),
            make_finding("d.rs", Some(10), Severity::Low),
        ];
        let hints = HuntHints::from_findings(&findings, cfg.clone());

        let crit = hints.security_boost_for(Path::new("a.rs"), 10);
        let high = hints.security_boost_for(Path::new("b.rs"), 10);
        let med = hints.security_boost_for(Path::new("c.rs"), 10);
        let low = hints.security_boost_for(Path::new("d.rs"), 10);

        assert!(crit > high, "critical > high: {crit} vs {high}");
        assert!(high > med, "high > medium: {high} vs {med}");
        assert!(med > low, "medium > low: {med} vs {low}");
        assert_eq!(crit, cfg.critical_boost);
        assert_eq!(high, cfg.high_boost);
        assert_eq!(med, cfg.medium_boost);
        assert_eq!(low, cfg.low_boost);
    }

    #[test]
    fn multiple_findings_in_same_file_accumulate_boosts() {
        let findings = vec![
            make_finding("src/vuln.rs", Some(10), Severity::High),
            make_finding("src/vuln.rs", Some(15), Severity::Medium),
        ];
        let cfg = HuntHintConfig {
            proximity_window: 10,
            ..Default::default()
        };
        let hints = HuntHints::from_findings(&findings, cfg.clone());

        // Line 12 is within both finding windows (10±10 and 15±10)
        let boost = hints.security_boost_for(Path::new("src/vuln.rs"), 12);
        assert_eq!(boost, cfg.high_boost + cfg.medium_boost);
    }

    #[test]
    fn different_file_gets_no_boost() {
        let findings = vec![make_finding("src/auth.rs", Some(50), Severity::Critical)];
        let hints = HuntHints::from_findings(&findings, HuntHintConfig::default());
        assert_eq!(hints.security_boost_for(Path::new("src/other.rs"), 50), 0.0);
    }

    #[test]
    fn filename_fallback_match() {
        // Relative path in findings, absolute path queried
        let findings = vec![make_finding("auth.rs", Some(50), Severity::High)];
        let hints = HuntHints::from_findings(&findings, HuntHintConfig::default());
        // Query with a different prefix but same filename
        let boost = hints.security_boost_for(Path::new("/project/src/auth.rs"), 50);
        assert!(boost > 0.0, "expected filename fallback to match");
    }

    #[test]
    fn has_findings_in_returns_true_for_affected_file() {
        let findings = vec![make_finding("src/auth.rs", Some(50), Severity::High)];
        let hints = HuntHints::from_findings(&findings, HuntHintConfig::default());
        assert!(hints.has_findings_in(Path::new("src/auth.rs")));
        assert!(!hints.has_findings_in(Path::new("src/other.rs")));
    }

    #[test]
    fn to_vec_returns_all_hints() {
        let findings = vec![
            make_finding("a.rs", Some(1), Severity::High),
            make_finding("b.rs", Some(2), Severity::Medium),
        ];
        let hints = HuntHints::from_findings(&findings, HuntHintConfig::default());
        assert_eq!(hints.to_vec().len(), 2);
    }

    #[test]
    fn file_count_and_hint_count() {
        let findings = vec![
            make_finding("a.rs", Some(1), Severity::High),
            make_finding("a.rs", Some(20), Severity::Medium),
            make_finding("b.rs", Some(5), Severity::Low),
        ];
        let hints = HuntHints::from_findings(&findings, HuntHintConfig::default());
        assert_eq!(hints.file_count(), 2);
        assert_eq!(hints.hint_count(), 3);
    }
}
