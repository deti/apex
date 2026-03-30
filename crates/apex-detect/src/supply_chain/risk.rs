use serde::{Deserialize, Serialize};

use crate::supply_chain::diff::{ChangeKind, TreeChange, TreeDiff};

/// A specific risk signal detected on a change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RiskSignal {
    /// Checksum mismatch without version change.
    ChecksumMismatch,
    /// New transitive dependency added at depth > 2.
    DeepTransitiveAdd { depth: u32 },
    /// Major semver version jump.
    MajorVersionJump { from: String, to: String },
    /// Source URL changed (registry migration).
    SourceMigration,
    /// Git branch dep with mutated commit.
    BranchMutation,
    /// Package has no checksum in lockfile.
    MissingChecksum,
    /// License changed or became unknown.
    LicenseChange,
    /// Multiple unrelated transitive deps changed together.
    CoordinatedUpdate { count: usize },
}

/// Severity classification derived from risk score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskSeverity {
    Critical,
    High,
    Medium,
    Low,
    None,
}

impl RiskSeverity {
    pub fn from_score(score: f64) -> Self {
        if score >= 8.0 {
            Self::Critical
        } else if score >= 6.0 {
            Self::High
        } else if score >= 3.0 {
            Self::Medium
        } else if score >= 1.0 {
            Self::Low
        } else {
            Self::None
        }
    }
}

/// Composite risk assessment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub score: f64,
    pub severity: RiskSeverity,
    pub signals: Vec<RiskSignal>,
    pub explanation: String,
}

/// Score a single TreeChange and return its individual risk assessment.
pub fn score_change(change: &TreeChange) -> RiskAssessment {
    let mut score = 0.0_f64;
    let mut signals = Vec::new();

    match &change.kind {
        ChangeKind::ChecksumChanged { from, to } => {
            // Same version, different checksum = possible registry compromise
            if from.is_some() && to.is_some() {
                score += 4.0;
                signals.push(RiskSignal::ChecksumMismatch);
            } else if from.is_none() || to.is_none() {
                score += 1.5;
                signals.push(RiskSignal::MissingChecksum);
            }
        }
        ChangeKind::Added => {
            if change.depth > 2 {
                let add_score = (change.depth as f64 * 0.5).min(3.0);
                score += add_score;
                signals.push(RiskSignal::DeepTransitiveAdd {
                    depth: change.depth,
                });
            }
        }
        ChangeKind::VersionChanged { from, to } => {
            if is_major_jump(from, to) {
                score += 1.0;
                signals.push(RiskSignal::MajorVersionJump {
                    from: from.clone(),
                    to: to.clone(),
                });
            }
        }
        ChangeKind::SourceChanged { .. } => {
            score += 2.0;
            signals.push(RiskSignal::SourceMigration);
        }
        ChangeKind::BranchMutated { .. } => {
            score += 2.5;
            signals.push(RiskSignal::BranchMutation);
        }
        ChangeKind::LicenseChanged { .. } => {
            score += 0.5;
            signals.push(RiskSignal::LicenseChange);
        }
        ChangeKind::Removed | ChangeKind::DepthChanged { .. } => {
            // Low risk by themselves
        }
    }

    let severity = RiskSeverity::from_score(score);
    let explanation = build_change_explanation(change, &signals);

    RiskAssessment {
        score,
        severity,
        signals,
        explanation,
    }
}

/// Score an entire TreeDiff, mutating each change's risk_score/risk_signals,
/// and return aggregate assessment.
pub fn score_diff(diff: &mut TreeDiff) -> RiskAssessment {
    let mut all_signals = Vec::new();
    let mut total_score = 0.0_f64;

    // Score individual changes
    for change in &mut diff.changes {
        let assessment = score_change(change);
        change.risk_score = assessment.score;
        change.risk_signals = assessment.signals.iter().map(|s| format!("{s:?}")).collect();
        total_score += assessment.score;
        all_signals.extend(assessment.signals);
    }

    // Coordinated update detection: 3+ unrelated transitive deps changed version
    let transitive_version_changes: Vec<&TreeChange> = diff
        .changes
        .iter()
        .filter(|c| matches!(c.kind, ChangeKind::VersionChanged { .. }) && c.depth >= 2)
        .collect();

    if transitive_version_changes.len() >= 3 {
        let count = transitive_version_changes.len();
        total_score += 3.0;
        all_signals.push(RiskSignal::CoordinatedUpdate { count });
    }

    // Cap at 10.0
    let capped_score = total_score.min(10.0);
    diff.aggregate_risk = capped_score;

    let severity = RiskSeverity::from_score(capped_score);
    let explanation = build_diff_explanation(diff, &all_signals);

    RiskAssessment {
        score: capped_score,
        severity,
        signals: all_signals,
        explanation,
    }
}

/// Check if a version change is a major semver jump.
fn is_major_jump(from: &str, to: &str) -> bool {
    let from_major = from.split('.').next().and_then(|s| s.parse::<u32>().ok());
    let to_major = to.split('.').next().and_then(|s| s.parse::<u32>().ok());
    match (from_major, to_major) {
        (Some(f), Some(t)) => t > f,
        _ => false,
    }
}

fn build_change_explanation(change: &TreeChange, signals: &[RiskSignal]) -> String {
    if signals.is_empty() {
        return format!("{}: {:?} (no risk signals)", change.package, change.kind);
    }
    let signal_names: Vec<String> = signals.iter().map(|s| format!("{s:?}")).collect();
    let path_str = change.propagation_path.join(" -> ");
    format!(
        "{} at depth {}: {:?} [path: {}] [signals: {}]",
        change.package,
        change.depth,
        change.kind,
        path_str,
        signal_names.join(", ")
    )
}

fn build_diff_explanation(diff: &TreeDiff, signals: &[RiskSignal]) -> String {
    let s = &diff.summary;
    format!(
        "{} changes ({} added, {} removed, {} version, {} checksum). Risk: {:.1}/10. Signals: {}",
        s.total_changes,
        s.added,
        s.removed,
        s.version_changed,
        s.checksum_changed,
        diff.aggregate_risk,
        signals.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supply_chain::diff::{ChangeKind, TreeChange, TreeDiff, TreeDiffSummary};
    use crate::supply_chain::tree::Ecosystem;
    use chrono::Utc;

    fn make_change(kind: ChangeKind, depth: u32) -> TreeChange {
        TreeChange {
            package: "test-pkg".to_string(),
            ecosystem: Ecosystem::Npm,
            kind,
            propagation_path: vec!["root".to_string(), "test-pkg".to_string()],
            depth,
            risk_score: 0.0,
            risk_signals: vec![],
        }
    }

    fn empty_summary() -> TreeDiffSummary {
        TreeDiffSummary {
            added: 0,
            removed: 0,
            version_changed: 0,
            checksum_changed: 0,
            depth_changed: 0,
            source_changed: 0,
            branch_mutated: 0,
            license_changed: 0,
            total_changes: 0,
        }
    }

    #[test]
    fn checksum_mismatch_scores_4() {
        let change = make_change(
            ChangeKind::ChecksumChanged {
                from: Some("sha256:aaa".to_string()),
                to: Some("sha256:bbb".to_string()),
            },
            2,
        );
        let assessment = score_change(&change);
        assert!((assessment.score - 4.0).abs() < f64::EPSILON);
        assert!(assessment.signals.contains(&RiskSignal::ChecksumMismatch));
    }

    #[test]
    fn missing_checksum_scores_1_5() {
        let change = make_change(
            ChangeKind::ChecksumChanged {
                from: Some("sha256:aaa".to_string()),
                to: None,
            },
            2,
        );
        let assessment = score_change(&change);
        assert!((assessment.score - 1.5).abs() < f64::EPSILON);
        assert!(assessment.signals.contains(&RiskSignal::MissingChecksum));
    }

    #[test]
    fn deep_transitive_add_at_depth_4_scores_2() {
        let change = make_change(ChangeKind::Added, 4);
        let assessment = score_change(&change);
        assert!((assessment.score - 2.0).abs() < f64::EPSILON);
        assert!(assessment
            .signals
            .contains(&RiskSignal::DeepTransitiveAdd { depth: 4 }));
    }

    #[test]
    fn deep_transitive_add_capped_at_3() {
        let change = make_change(ChangeKind::Added, 10);
        let assessment = score_change(&change);
        // 10 * 0.5 = 5.0, capped at 3.0
        assert!((assessment.score - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn shallow_add_no_risk() {
        let change = make_change(ChangeKind::Added, 1);
        let assessment = score_change(&change);
        assert!((assessment.score - 0.0).abs() < f64::EPSILON);
        assert!(assessment.signals.is_empty());
    }

    #[test]
    fn major_version_jump_scores_1() {
        let change = make_change(
            ChangeKind::VersionChanged {
                from: "1.0.0".to_string(),
                to: "2.0.0".to_string(),
            },
            1,
        );
        let assessment = score_change(&change);
        assert!((assessment.score - 1.0).abs() < f64::EPSILON);
        assert!(assessment.signals.contains(&RiskSignal::MajorVersionJump {
            from: "1.0.0".to_string(),
            to: "2.0.0".to_string(),
        }));
    }

    #[test]
    fn minor_version_change_no_risk() {
        let change = make_change(
            ChangeKind::VersionChanged {
                from: "1.0.0".to_string(),
                to: "1.1.0".to_string(),
            },
            1,
        );
        let assessment = score_change(&change);
        assert!((assessment.score - 0.0).abs() < f64::EPSILON);
        assert!(assessment.signals.is_empty());
    }

    #[test]
    fn branch_mutation_scores_2_5() {
        let change = make_change(
            ChangeKind::BranchMutated {
                branch: "main".to_string(),
                from_commit: Some("abc".to_string()),
                to_commit: Some("def".to_string()),
            },
            1,
        );
        let assessment = score_change(&change);
        assert!((assessment.score - 2.5).abs() < f64::EPSILON);
        assert!(assessment.signals.contains(&RiskSignal::BranchMutation));
    }

    #[test]
    fn source_migration_scores_2() {
        let change = make_change(
            ChangeKind::SourceChanged {
                from: Some("https://registry.npmjs.org".to_string()),
                to: Some("https://evil.com".to_string()),
            },
            1,
        );
        let assessment = score_change(&change);
        assert!((assessment.score - 2.0).abs() < f64::EPSILON);
        assert!(assessment.signals.contains(&RiskSignal::SourceMigration));
    }

    #[test]
    fn license_change_scores_0_5() {
        let change = make_change(
            ChangeKind::LicenseChanged {
                from: Some("MIT".to_string()),
                to: Some("GPL-3.0".to_string()),
            },
            1,
        );
        let assessment = score_change(&change);
        assert!((assessment.score - 0.5).abs() < f64::EPSILON);
        assert!(assessment.signals.contains(&RiskSignal::LicenseChange));
    }

    #[test]
    fn coordinated_update_adds_3() {
        // Build a diff with 5 transitive version changes at depth >= 2
        let changes: Vec<TreeChange> = (0..5)
            .map(|i| {
                let mut c = make_change(
                    ChangeKind::VersionChanged {
                        from: "1.0.0".to_string(),
                        to: "1.1.0".to_string(),
                    },
                    3,
                );
                c.package = format!("pkg-{i}");
                c
            })
            .collect();

        let mut diff = TreeDiff {
            from_timestamp: Utc::now(),
            to_timestamp: Utc::now(),
            ecosystem: Ecosystem::Npm,
            changes,
            aggregate_risk: 0.0,
            summary: empty_summary(),
        };

        let assessment = score_diff(&mut diff);
        // 5 minor version changes = 0 individual risk, but coordinated = +3.0
        assert!((assessment.score - 3.0).abs() < f64::EPSILON);
        assert!(assessment
            .signals
            .contains(&RiskSignal::CoordinatedUpdate { count: 5 }));
    }

    #[test]
    fn score_capped_at_10() {
        // Build a diff with many high-risk changes
        let changes: Vec<TreeChange> = (0..10)
            .map(|i| {
                let mut c = make_change(
                    ChangeKind::ChecksumChanged {
                        from: Some("sha256:old".to_string()),
                        to: Some("sha256:new".to_string()),
                    },
                    2,
                );
                c.package = format!("pkg-{i}");
                c
            })
            .collect();

        let mut diff = TreeDiff {
            from_timestamp: Utc::now(),
            to_timestamp: Utc::now(),
            ecosystem: Ecosystem::Npm,
            changes,
            aggregate_risk: 0.0,
            summary: empty_summary(),
        };

        let assessment = score_diff(&mut diff);
        // 10 * 4.0 = 40.0 but capped at 10.0
        assert!((assessment.score - 10.0).abs() < f64::EPSILON);
        assert_eq!(assessment.severity, RiskSeverity::Critical);
    }

    #[test]
    fn risk_severity_boundaries() {
        assert_eq!(RiskSeverity::from_score(10.0), RiskSeverity::Critical);
        assert_eq!(RiskSeverity::from_score(8.0), RiskSeverity::Critical);
        assert_eq!(RiskSeverity::from_score(7.9), RiskSeverity::High);
        assert_eq!(RiskSeverity::from_score(6.0), RiskSeverity::High);
        assert_eq!(RiskSeverity::from_score(5.9), RiskSeverity::Medium);
        assert_eq!(RiskSeverity::from_score(3.0), RiskSeverity::Medium);
        assert_eq!(RiskSeverity::from_score(2.9), RiskSeverity::Low);
        assert_eq!(RiskSeverity::from_score(1.0), RiskSeverity::Low);
        assert_eq!(RiskSeverity::from_score(0.9), RiskSeverity::None);
        assert_eq!(RiskSeverity::from_score(0.0), RiskSeverity::None);
    }

    #[test]
    fn is_major_jump_true() {
        assert!(is_major_jump("1.0.0", "2.0.0"));
        assert!(is_major_jump("0.9.0", "1.0.0"));
    }

    #[test]
    fn is_major_jump_false() {
        assert!(!is_major_jump("1.0.0", "1.1.0"));
        assert!(!is_major_jump("2.0.0", "2.0.1"));
        assert!(!is_major_jump("2.0.0", "1.0.0")); // downgrade
    }

    #[test]
    fn removed_and_depth_changed_no_risk() {
        let removed = make_change(ChangeKind::Removed, 1);
        assert!((score_change(&removed).score - 0.0).abs() < f64::EPSILON);

        let depth = make_change(ChangeKind::DepthChanged { from: 2, to: 4 }, 4);
        assert!((score_change(&depth).score - 0.0).abs() < f64::EPSILON);
    }
}
