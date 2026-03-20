//! CVSS v3.1 base scoring for security findings.
//!
//! Provides CWE-to-CVSS mapping and the standard base score formula.

use crate::finding::Finding;

/// Attack Vector metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackVector {
    Network,
    Adjacent,
    Local,
    Physical,
}

/// Attack Complexity metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackComplexity {
    Low,
    High,
}

/// Privileges Required metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegesRequired {
    None,
    Low,
    High,
}

/// User Interaction metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserInteraction {
    None,
    Required,
}

/// Scope metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Unchanged,
    Changed,
}

/// Impact metric (used for Confidentiality, Integrity, and Availability).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impact {
    None,
    Low,
    High,
}

/// CVSS v3.1 base metric group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CvssBase {
    pub attack_vector: AttackVector,
    pub attack_complexity: AttackComplexity,
    pub privileges_required: PrivilegesRequired,
    pub user_interaction: UserInteraction,
    pub scope: Scope,
    pub confidentiality: Impact,
    pub integrity: Impact,
    pub availability: Impact,
}

impl AttackVector {
    fn weight(self) -> f64 {
        match self {
            AttackVector::Network => 0.85,
            AttackVector::Adjacent => 0.62,
            AttackVector::Local => 0.55,
            AttackVector::Physical => 0.20,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            AttackVector::Network => "N",
            AttackVector::Adjacent => "A",
            AttackVector::Local => "L",
            AttackVector::Physical => "P",
        }
    }
}

impl AttackComplexity {
    fn weight(self) -> f64 {
        match self {
            AttackComplexity::Low => 0.77,
            AttackComplexity::High => 0.44,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            AttackComplexity::Low => "L",
            AttackComplexity::High => "H",
        }
    }
}

impl PrivilegesRequired {
    fn weight(self, scope: Scope) -> f64 {
        match (self, scope) {
            (PrivilegesRequired::None, _) => 0.85,
            (PrivilegesRequired::Low, Scope::Unchanged) => 0.62,
            (PrivilegesRequired::Low, Scope::Changed) => 0.68,
            (PrivilegesRequired::High, Scope::Unchanged) => 0.27,
            (PrivilegesRequired::High, Scope::Changed) => 0.50,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            PrivilegesRequired::None => "N",
            PrivilegesRequired::Low => "L",
            PrivilegesRequired::High => "H",
        }
    }
}

impl UserInteraction {
    fn weight(self) -> f64 {
        match self {
            UserInteraction::None => 0.85,
            UserInteraction::Required => 0.62,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            UserInteraction::None => "N",
            UserInteraction::Required => "R",
        }
    }
}

impl Scope {
    fn abbrev(self) -> &'static str {
        match self {
            Scope::Unchanged => "U",
            Scope::Changed => "C",
        }
    }
}

impl Impact {
    fn weight(self) -> f64 {
        match self {
            Impact::None => 0.0,
            Impact::Low => 0.22,
            Impact::High => 0.56,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            Impact::None => "N",
            Impact::Low => "L",
            Impact::High => "H",
        }
    }
}

/// Return default CVSS base metrics for a given CWE ID.
///
/// Maps common CWEs to their typical base metric profiles. Unknown CWEs
/// receive a medium-severity default (~5.3).
pub fn cwe_default_cvss(cwe_id: u32) -> CvssBase {
    match cwe_id {
        // CWE-78: OS Command Injection → 9.8
        78 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-79: Cross-site Scripting (XSS) → 6.1
        79 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::Required,
            scope: Scope::Changed,
            confidentiality: Impact::Low,
            integrity: Impact::Low,
            availability: Impact::None,
        },
        // CWE-89: SQL Injection → 9.8
        89 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-94: Code Injection → 9.8
        94 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-22: Path Traversal → 7.5
        22 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::None,
            availability: Impact::None,
        },
        // CWE-502: Deserialization of Untrusted Data → 9.8
        502 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-798: Hardcoded Credentials → 9.8
        798 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-918: Server-Side Request Forgery (SSRF) → 8.6
        918 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Changed,
            confidentiality: Impact::High,
            integrity: Impact::Low,
            availability: Impact::None,
        },
        // CWE-328: Weak Hash → 7.5
        328 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::None,
            availability: Impact::None,
        },
        // CWE-295: Improper Certificate Validation → 7.4
        295 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::High,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::None,
        },
        // Unknown CWE → medium default (~5.3)
        _ => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::Low,
            integrity: Impact::None,
            availability: Impact::None,
        },
    }
}

/// CVSS v3.1 roundup: smallest number >= x that is a multiple of 0.1.
fn roundup(x: f64) -> f32 {
    let int_x = (x * 100_000.0) as u64;
    #[allow(unknown_lints, clippy::manual_is_multiple_of)]
    if int_x % 10_000 == 0 {
        (int_x as f64 / 100_000.0) as f32
    } else {
        ((int_x / 10_000 + 1) as f64 * 10_000.0 / 100_000.0) as f32
    }
}

/// Calculate the CVSS v3.1 base score from a set of base metrics.
pub fn calculate_cvss_score(base: &CvssBase) -> f32 {
    let isc_raw = 1.0
        - (1.0 - base.confidentiality.weight())
            * (1.0 - base.integrity.weight())
            * (1.0 - base.availability.weight());

    let impact = match base.scope {
        Scope::Unchanged => 6.42 * isc_raw,
        Scope::Changed => 7.52 * (isc_raw - 0.029) - 3.25 * (isc_raw - 0.02).powf(15.0),
    };

    if impact <= 0.0 {
        return 0.0;
    }

    let exploitability = 8.22
        * base.attack_vector.weight()
        * base.attack_complexity.weight()
        * base.privileges_required.weight(base.scope)
        * base.user_interaction.weight();

    let raw = match base.scope {
        Scope::Unchanged => {
            let s = impact + exploitability;
            if s > 10.0 {
                10.0
            } else {
                s
            }
        }
        Scope::Changed => {
            let s = 1.08 * (impact + exploitability);
            if s > 10.0 {
                10.0
            } else {
                s
            }
        }
    };

    roundup(raw)
}

/// Produce the CVSS v3.1 vector string for the given base metrics.
pub fn cvss_vector_string(base: &CvssBase) -> String {
    format!(
        "CVSS:3.1/AV:{}/AC:{}/PR:{}/UI:{}/S:{}/C:{}/I:{}/A:{}",
        base.attack_vector.abbrev(),
        base.attack_complexity.abbrev(),
        base.privileges_required.abbrev(),
        base.user_interaction.abbrev(),
        base.scope.abbrev(),
        base.confidentiality.abbrev(),
        base.integrity.abbrev(),
        base.availability.abbrev(),
    )
}

/// Score a [`Finding`] by its first CWE ID.
///
/// Returns `(Some(score), Some(vector))` if the finding has at least one CWE ID,
/// or `(None, None)` otherwise.
pub fn score_finding(finding: &Finding) -> (Option<f32>, Option<String>) {
    match finding.cwe_ids.first() {
        Some(&cwe_id) => {
            let base = cwe_default_cvss(cwe_id);
            let score = calculate_cvss_score(&base);
            let vector = cvss_vector_string(&base);
            (Some(score), Some(vector))
        }
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwe_78_scores_critical() {
        let base = cwe_default_cvss(78);
        let score = calculate_cvss_score(&base);
        assert!(
            score >= 9.0,
            "CWE-78 score {score} should be >= 9.0 (critical)"
        );
    }

    #[test]
    fn cwe_79_scores_medium() {
        let base = cwe_default_cvss(79);
        let score = calculate_cvss_score(&base);
        assert!(
            (5.0..=7.0).contains(&score),
            "CWE-79 score {score} should be between 5.0 and 7.0 (medium)"
        );
    }

    #[test]
    fn cwe_89_scores_critical() {
        let base = cwe_default_cvss(89);
        let score = calculate_cvss_score(&base);
        assert!(
            score >= 9.0,
            "CWE-89 score {score} should be >= 9.0 (critical)"
        );
    }

    #[test]
    fn unknown_cwe_scores_medium() {
        let base = cwe_default_cvss(99999);
        let score = calculate_cvss_score(&base);
        let diff = (score - 5.3_f32).abs();
        assert!(diff < 0.2, "Unknown CWE score {score} should be ~5.3");
    }

    #[test]
    fn cvss_vector_format() {
        let base = cwe_default_cvss(78);
        let vec = cvss_vector_string(&base);
        assert!(
            vec.starts_with("CVSS:3.1/"),
            "Vector string should start with CVSS:3.1/, got: {vec}"
        );
        assert_eq!(vec, "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H");
    }

    #[test]
    fn score_zero_impact() {
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::None,
            integrity: Impact::None,
            availability: Impact::None,
        };
        let score = calculate_cvss_score(&base);
        assert!(
            score == 0.0,
            "All-None impact should yield score 0.0, got {score}"
        );
    }

    #[test]
    fn roundup_to_nearest_tenth() {
        // 4.0 should stay 4.0
        assert_eq!(roundup(4.0), 4.0);
        // 4.02 should round up to 4.1
        assert_eq!(roundup(4.02), 4.1);
        // 4.1 should stay 4.1
        assert_eq!(roundup(4.1), 4.1);
        // 4.91 should round up to 5.0
        assert_eq!(roundup(4.91), 5.0);
    }

    // --- CWE mapping coverage: exercise every match arm in cwe_default_cvss ---

    #[test]
    fn cwe_94_code_injection_scores_critical() {
        let base = cwe_default_cvss(94);
        let score = calculate_cvss_score(&base);
        assert!(score >= 9.0, "CWE-94 score {score} should be >= 9.0");
        assert_eq!(base.confidentiality, Impact::High);
        assert_eq!(base.integrity, Impact::High);
        assert_eq!(base.availability, Impact::High);
    }

    #[test]
    fn cwe_22_path_traversal_scores_high() {
        let base = cwe_default_cvss(22);
        let score = calculate_cvss_score(&base);
        assert!(
            (7.0..=8.0).contains(&score),
            "CWE-22 score {score} should be ~7.5"
        );
        assert_eq!(base.confidentiality, Impact::High);
        assert_eq!(base.integrity, Impact::None);
        assert_eq!(base.availability, Impact::None);
    }

    #[test]
    fn cwe_502_deserialization_scores_critical() {
        let base = cwe_default_cvss(502);
        let score = calculate_cvss_score(&base);
        assert!(score >= 9.0, "CWE-502 score {score} should be >= 9.0");
    }

    #[test]
    fn cwe_798_hardcoded_creds_scores_critical() {
        let base = cwe_default_cvss(798);
        let score = calculate_cvss_score(&base);
        assert!(score >= 9.0, "CWE-798 score {score} should be >= 9.0");
    }

    #[test]
    fn cwe_918_ssrf_scope_changed() {
        let base = cwe_default_cvss(918);
        assert_eq!(base.scope, Scope::Changed);
        assert_eq!(base.confidentiality, Impact::High);
        assert_eq!(base.integrity, Impact::Low);
        assert_eq!(base.availability, Impact::None);
        let score = calculate_cvss_score(&base);
        assert!(
            (8.0..=10.0).contains(&score),
            "CWE-918 score {score} should be high"
        );
        let vec = cvss_vector_string(&base);
        assert!(vec.contains("S:C"), "SSRF should have scope Changed");
    }

    #[test]
    fn cwe_328_weak_hash_scores_high() {
        let base = cwe_default_cvss(328);
        let score = calculate_cvss_score(&base);
        assert!(
            (7.0..=8.0).contains(&score),
            "CWE-328 score {score} should be ~7.5"
        );
    }

    #[test]
    fn cwe_295_cert_validation_high_complexity() {
        let base = cwe_default_cvss(295);
        assert_eq!(base.attack_complexity, AttackComplexity::High);
        let score = calculate_cvss_score(&base);
        assert!(
            (7.0..=8.0).contains(&score),
            "CWE-295 score {score} should be ~7.4"
        );
        let vec = cvss_vector_string(&base);
        assert!(vec.contains("AC:H"), "Should have high attack complexity");
    }

    // --- Exercise all enum variant weights and abbreviations ---

    #[test]
    fn attack_vector_all_weights() {
        assert_eq!(AttackVector::Network.weight(), 0.85);
        assert_eq!(AttackVector::Adjacent.weight(), 0.62);
        assert_eq!(AttackVector::Local.weight(), 0.55);
        assert_eq!(AttackVector::Physical.weight(), 0.20);
    }

    #[test]
    fn attack_vector_all_abbrevs() {
        assert_eq!(AttackVector::Network.abbrev(), "N");
        assert_eq!(AttackVector::Adjacent.abbrev(), "A");
        assert_eq!(AttackVector::Local.abbrev(), "L");
        assert_eq!(AttackVector::Physical.abbrev(), "P");
    }

    #[test]
    fn attack_complexity_all_weights() {
        assert_eq!(AttackComplexity::Low.weight(), 0.77);
        assert_eq!(AttackComplexity::High.weight(), 0.44);
    }

    #[test]
    fn attack_complexity_all_abbrevs() {
        assert_eq!(AttackComplexity::Low.abbrev(), "L");
        assert_eq!(AttackComplexity::High.abbrev(), "H");
    }

    #[test]
    fn privileges_required_all_weights() {
        assert_eq!(PrivilegesRequired::None.weight(Scope::Unchanged), 0.85);
        assert_eq!(PrivilegesRequired::None.weight(Scope::Changed), 0.85);
        assert_eq!(PrivilegesRequired::Low.weight(Scope::Unchanged), 0.62);
        assert_eq!(PrivilegesRequired::Low.weight(Scope::Changed), 0.68);
        assert_eq!(PrivilegesRequired::High.weight(Scope::Unchanged), 0.27);
        assert_eq!(PrivilegesRequired::High.weight(Scope::Changed), 0.50);
    }

    #[test]
    fn privileges_required_all_abbrevs() {
        assert_eq!(PrivilegesRequired::None.abbrev(), "N");
        assert_eq!(PrivilegesRequired::Low.abbrev(), "L");
        assert_eq!(PrivilegesRequired::High.abbrev(), "H");
    }

    #[test]
    fn user_interaction_all_weights() {
        assert_eq!(UserInteraction::None.weight(), 0.85);
        assert_eq!(UserInteraction::Required.weight(), 0.62);
    }

    #[test]
    fn user_interaction_all_abbrevs() {
        assert_eq!(UserInteraction::None.abbrev(), "N");
        assert_eq!(UserInteraction::Required.abbrev(), "R");
    }

    #[test]
    fn scope_all_abbrevs() {
        assert_eq!(Scope::Unchanged.abbrev(), "U");
        assert_eq!(Scope::Changed.abbrev(), "C");
    }

    #[test]
    fn impact_all_weights() {
        assert_eq!(Impact::None.weight(), 0.0);
        assert_eq!(Impact::Low.weight(), 0.22);
        assert_eq!(Impact::High.weight(), 0.56);
    }

    #[test]
    fn impact_all_abbrevs() {
        assert_eq!(Impact::None.abbrev(), "N");
        assert_eq!(Impact::Low.abbrev(), "L");
        assert_eq!(Impact::High.abbrev(), "H");
    }

    // --- Score calculation edge cases ---

    #[test]
    fn score_zero_impact_scope_changed() {
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Changed,
            confidentiality: Impact::None,
            integrity: Impact::None,
            availability: Impact::None,
        };
        let score = calculate_cvss_score(&base);
        assert_eq!(score, 0.0, "Zero impact with Changed scope should be 0.0");
    }

    #[test]
    fn score_with_scope_changed_and_high_impact() {
        // Scope::Changed triggers different formula in calculate_cvss_score
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Changed,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        };
        let score = calculate_cvss_score(&base);
        assert!(score > 0.0, "Should produce non-zero score");
        assert!(score <= 10.0, "Score should not exceed 10.0");
    }

    #[test]
    fn score_clamped_at_10_scope_unchanged() {
        // Very high exploitability + impact should clamp at 10.0
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        };
        let score = calculate_cvss_score(&base);
        // impact + exploitability for this config exceeds 10.0, so clamped
        assert!(score <= 10.0, "Score must be clamped at 10.0");
    }

    #[test]
    fn score_clamped_at_10_scope_changed() {
        // Scope::Changed with 1.08 multiplier can also exceed 10.0
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Changed,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        };
        let score = calculate_cvss_score(&base);
        assert_eq!(score, 10.0, "Max score with Changed scope should be 10.0");
    }

    #[test]
    fn score_low_impact_scope_unchanged() {
        // Only Low confidentiality, no integrity/availability
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::Low,
            integrity: Impact::None,
            availability: Impact::None,
        };
        let score = calculate_cvss_score(&base);
        assert!(score > 0.0 && score < 10.0);
    }

    #[test]
    fn score_with_adjacent_attack_vector() {
        let base = CvssBase {
            attack_vector: AttackVector::Adjacent,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        };
        let score = calculate_cvss_score(&base);
        // Adjacent vector has lower weight than Network, so score should be lower
        let network_base = cwe_default_cvss(78); // Network, same otherwise
        let network_score = calculate_cvss_score(&network_base);
        assert!(
            score < network_score,
            "Adjacent ({score}) should score lower than Network ({network_score})"
        );
    }

    #[test]
    fn score_with_local_attack_vector() {
        let base = CvssBase {
            attack_vector: AttackVector::Local,
            attack_complexity: AttackComplexity::High,
            privileges_required: PrivilegesRequired::High,
            user_interaction: UserInteraction::Required,
            scope: Scope::Unchanged,
            confidentiality: Impact::Low,
            integrity: Impact::Low,
            availability: Impact::None,
        };
        let score = calculate_cvss_score(&base);
        assert!(score > 0.0 && score < 5.0, "Low severity score: {score}");
    }

    #[test]
    fn score_with_physical_attack_vector() {
        let base = CvssBase {
            attack_vector: AttackVector::Physical,
            attack_complexity: AttackComplexity::High,
            privileges_required: PrivilegesRequired::High,
            user_interaction: UserInteraction::Required,
            scope: Scope::Unchanged,
            confidentiality: Impact::Low,
            integrity: Impact::None,
            availability: Impact::None,
        };
        let score = calculate_cvss_score(&base);
        assert!(
            score > 0.0 && score < 3.0,
            "Physical+High should be very low: {score}"
        );
    }

    #[test]
    fn score_privileges_low_scope_changed() {
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::Low,
            user_interaction: UserInteraction::None,
            scope: Scope::Changed,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        };
        let score = calculate_cvss_score(&base);
        assert!(score > 8.0, "PR:L/S:C should still be high: {score}");
    }

    #[test]
    fn score_privileges_high_scope_unchanged() {
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::High,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        };
        let score = calculate_cvss_score(&base);
        assert!(score > 5.0 && score < 9.0, "PR:H/S:U score: {score}");
    }

    #[test]
    fn score_privileges_high_scope_changed() {
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::High,
            user_interaction: UserInteraction::None,
            scope: Scope::Changed,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        };
        let score = calculate_cvss_score(&base);
        assert!(score > 7.0, "PR:H/S:C score: {score}");
    }

    // --- Vector string coverage ---

    #[test]
    fn vector_string_all_variants() {
        let base = CvssBase {
            attack_vector: AttackVector::Physical,
            attack_complexity: AttackComplexity::High,
            privileges_required: PrivilegesRequired::High,
            user_interaction: UserInteraction::Required,
            scope: Scope::Changed,
            confidentiality: Impact::Low,
            integrity: Impact::Low,
            availability: Impact::Low,
        };
        let vec = cvss_vector_string(&base);
        assert_eq!(vec, "CVSS:3.1/AV:P/AC:H/PR:H/UI:R/S:C/C:L/I:L/A:L");
    }

    #[test]
    fn vector_string_adjacent_low_unchanged() {
        let base = CvssBase {
            attack_vector: AttackVector::Adjacent,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::Low,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::None,
            integrity: Impact::High,
            availability: Impact::None,
        };
        let vec = cvss_vector_string(&base);
        assert_eq!(vec, "CVSS:3.1/AV:A/AC:L/PR:L/UI:N/S:U/C:N/I:H/A:N");
    }

    #[test]
    fn vector_string_local() {
        let base = CvssBase {
            attack_vector: AttackVector::Local,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::Required,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::None,
            availability: Impact::High,
        };
        let vec = cvss_vector_string(&base);
        assert_eq!(vec, "CVSS:3.1/AV:L/AC:L/PR:N/UI:R/S:U/C:H/I:N/A:H");
    }

    // --- score_finding coverage ---

    #[test]
    fn score_finding_with_cwe_ids() {
        let finding = Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity: crate::finding::Severity::High,
            category: crate::finding::FindingCategory::Injection,
            file: std::path::PathBuf::from("test.py"),
            line: Some(1),
            title: "test".into(),
            description: "test".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![78],
                    noisy: false,
        };
        let (score, vector) = score_finding(&finding);
        assert!(score.is_some());
        assert!(vector.is_some());
        assert!(score.unwrap() >= 9.0);
        assert!(vector.unwrap().starts_with("CVSS:3.1/"));
    }

    #[test]
    fn score_finding_no_cwe_ids() {
        let finding = Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity: crate::finding::Severity::Low,
            category: crate::finding::FindingCategory::PanicPath,
            file: std::path::PathBuf::from("test.py"),
            line: None,
            title: "test".into(),
            description: "test".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![],
                    noisy: false,
        };
        let (score, vector) = score_finding(&finding);
        assert!(score.is_none());
        assert!(vector.is_none());
    }

    #[test]
    fn score_finding_uses_first_cwe() {
        let finding = Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity: crate::finding::Severity::High,
            category: crate::finding::FindingCategory::Injection,
            file: std::path::PathBuf::from("test.py"),
            line: Some(1),
            title: "test".into(),
            description: "test".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix".into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![79, 78], // Should use 79 (XSS), not 78
                    noisy: false,
        };
        let (score, _vector) = score_finding(&finding);
        let s = score.unwrap();
        // CWE-79 scores ~6.1, not 9.8
        assert!(s < 7.0, "Should use first CWE (79), got score {s}");
    }

    // --- CWE-79 vector string (exercises UserInteraction::Required + Scope::Changed + Impact::Low) ---

    #[test]
    fn cwe_79_vector_string() {
        let base = cwe_default_cvss(79);
        let vec = cvss_vector_string(&base);
        assert_eq!(vec, "CVSS:3.1/AV:N/AC:L/PR:N/UI:R/S:C/C:L/I:L/A:N");
    }

    // --- Scope::Changed with low impact (exercises the Changed branch below 10.0 without clamping) ---

    #[test]
    fn scope_changed_low_impact_no_clamping() {
        let base = CvssBase {
            attack_vector: AttackVector::Physical,
            attack_complexity: AttackComplexity::High,
            privileges_required: PrivilegesRequired::High,
            user_interaction: UserInteraction::Required,
            scope: Scope::Changed,
            confidentiality: Impact::Low,
            integrity: Impact::None,
            availability: Impact::None,
        };
        let score = calculate_cvss_score(&base);
        // Low exploitability + low impact + Changed scope: should not clamp
        assert!(
            score > 0.0 && score < 10.0,
            "Score should not be clamped: {score}"
        );
    }

    // --- Scope::Unchanged with moderate values (exercises the s <= 10.0 else branch) ---

    #[test]
    fn scope_unchanged_moderate_no_clamping() {
        let base = CvssBase {
            attack_vector: AttackVector::Local,
            attack_complexity: AttackComplexity::High,
            privileges_required: PrivilegesRequired::Low,
            user_interaction: UserInteraction::Required,
            scope: Scope::Unchanged,
            confidentiality: Impact::Low,
            integrity: Impact::Low,
            availability: Impact::None,
        };
        let score = calculate_cvss_score(&base);
        assert!(
            score > 0.0 && score < 10.0,
            "Moderate unchanged should not clamp: {score}"
        );
    }

    // --- Roundup edge cases ---

    #[test]
    fn roundup_exact_tenths() {
        assert_eq!(roundup(0.0), 0.0);
        assert_eq!(roundup(1.0), 1.0);
        assert_eq!(roundup(5.5), 5.5);
        assert_eq!(roundup(10.0), 10.0);
    }

    #[test]
    fn roundup_fractional() {
        assert_eq!(roundup(0.01), 0.1);
        assert_eq!(roundup(3.14), 3.2);
        assert_eq!(roundup(9.99), 10.0);
    }

    // --- PrivilegesRequired::Low with Scope::Unchanged in full score calc ---

    #[test]
    fn score_privileges_low_scope_unchanged() {
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::Low,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        };
        let score = calculate_cvss_score(&base);
        assert!(score > 7.0 && score < 10.0, "PR:L/S:U score: {score}");
    }

    // -----------------------------------------------------------------------
    // Bug-hunting tests
    // -----------------------------------------------------------------------

    /// roundup with a negative input: the (x * 100_000.0) as u64 cast
    /// saturates to 0 for negative values, silently producing 0.0.
    /// This is technically incorrect (roundup of -0.5 should be -0.5 or 0.0
    /// depending on definition) but in practice the CVSS formula guards
    /// against negatives. Documenting the behavior.
    #[test]
    fn roundup_negative_input_saturates() {
        // This documents the existing behavior: negative -> 0.0
        let result = roundup(-0.5);
        assert_eq!(
            result, 0.0,
            "roundup(-0.5) should be 0.0 due to u64 saturation"
        );
    }
}
