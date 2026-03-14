//! OWASP ASVS (Application Security Verification Standard) compliance reporting.
//!
//! Maps APEX detector findings to ASVS requirements and generates compliance reports.

/// ASVS verification level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsvsLevel {
    L1,
    L2,
    L3,
}

/// Status of an individual ASVS requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsvsStatus {
    /// Requirement verified — no findings from the mapped detector.
    Verified,
    /// Requirement failed — detector found issues.
    Failed,
    /// Requirement cannot be verified automatically.
    NotAutomated,
    /// Requirement not applicable to the target.
    NotApplicable,
}

/// An ASVS requirement mapped to an APEX detector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsvsRequirement {
    /// ASVS requirement ID, e.g. "V2.4.1".
    pub id: String,
    /// ASVS level (1, 2, or 3).
    pub level: u8,
    /// Human-readable description.
    pub description: String,
    /// APEX detector ID that covers this requirement.
    pub detector_id: String,
    /// Whether APEX can verify this requirement automatically.
    pub automated: bool,
}

/// Status of a single ASVS requirement after evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsvsRequirementStatus {
    pub requirement: AsvsRequirement,
    pub status: AsvsStatus,
    pub findings: Vec<String>,
}

/// Aggregate coverage statistics for an ASVS report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsvsCoverage {
    pub total: usize,
    pub automated: usize,
    pub verified: usize,
    pub failed: usize,
    pub manual_required: usize,
}

/// Full ASVS compliance report.
#[derive(Debug)]
pub struct AsvsReport {
    pub level: AsvsLevel,
    pub requirements: Vec<AsvsRequirementStatus>,
    pub coverage: AsvsCoverage,
}

/// Built-in ASVS requirement database.
///
/// Returns L1 and L2 requirements mapped to APEX detectors.
/// Requirements with `automated: false` need manual review.
pub fn asvs_requirements() -> Vec<AsvsRequirement> {
    vec![
        // L1 automated requirements
        AsvsRequirement {
            id: "V2.4.1".into(),
            level: 1,
            description: "Verify passwords stored with resistance to offline attacks".into(),
            detector_id: "crypto_failure".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V5.2.2".into(),
            level: 1,
            description: "Verify unstructured data is sanitized".into(),
            detector_id: "sql_injection".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V5.3.4".into(),
            level: 1,
            description: "Verify output encoding against SQL injection".into(),
            detector_id: "sql_injection".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V5.3.7".into(),
            level: 1,
            description: "Verify against command injection".into(),
            detector_id: "command_injection".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V5.3.8".into(),
            level: 1,
            description: "Verify against SSRF".into(),
            detector_id: "ssrf".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V6.2.1".into(),
            level: 1,
            description: "Verify approved cryptographic algorithms".into(),
            detector_id: "crypto_failure".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V8.3.4".into(),
            level: 1,
            description: "Verify no sensitive data in logs".into(),
            detector_id: "hardcoded_secret".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V12.3.1".into(),
            level: 1,
            description: "Verify path traversal protection".into(),
            detector_id: "path_traversal".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V14.2.1".into(),
            level: 1,
            description: "Verify dependencies are up to date".into(),
            detector_id: "dep_audit".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V3.2.1".into(),
            level: 1,
            description: "Verify session tokens generated with approved CSPRNG".into(),
            detector_id: "session_security".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V3.4.1".into(),
            level: 1,
            description: "Verify cookie-based tokens have Secure attribute".into(),
            detector_id: "session_security".into(),
            automated: true,
        },
        // L1 manual-review requirements
        AsvsRequirement {
            id: "V2.1.1".into(),
            level: 1,
            description: "Verify user passwords at least 12 chars".into(),
            detector_id: "password_length".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V2.1.7".into(),
            level: 1,
            description: "Verify passwords are checked against breached sets".into(),
            detector_id: "password_breach_check".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V2.2.1".into(),
            level: 1,
            description: "Verify anti-automation controls for credential stuffing".into(),
            detector_id: "rate_limiting".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V3.1.1".into(),
            level: 1,
            description: "Verify application never reveals session tokens in URLs".into(),
            detector_id: "session_url_leak".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V4.1.1".into(),
            level: 1,
            description: "Verify access controls enforced on trusted service layer".into(),
            detector_id: "access_control_layer".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V7.1.1".into(),
            level: 1,
            description: "Verify application does not log credentials or payment details".into(),
            detector_id: "log_credentials".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V8.1.1".into(),
            level: 1,
            description: "Verify application protects sensitive data from server-side caching".into(),
            detector_id: "cache_control".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V9.1.1".into(),
            level: 1,
            description: "Verify TLS is used for all client connectivity".into(),
            detector_id: "tls_enforcement".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V10.3.1".into(),
            level: 1,
            description: "Verify application source code and libraries do not contain backdoors".into(),
            detector_id: "backdoor_detection".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V13.1.1".into(),
            level: 1,
            description: "Verify all input using positive validation".into(),
            detector_id: "input_validation".into(),
            automated: false,
        },
        // L2 requirements
        AsvsRequirement {
            id: "V1.2.1".into(),
            level: 2,
            description: "Verify unique service accounts for components".into(),
            detector_id: "".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V1.5.4".into(),
            level: 2,
            description: "Verify access controls fail securely".into(),
            detector_id: "broken_access".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V5.1.5".into(),
            level: 2,
            description: "Verify URL redirects to allowed destinations".into(),
            detector_id: "ssrf".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V1.1.6".into(),
            level: 2,
            description: "Verify centralized security controls".into(),
            detector_id: "security_controls".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V1.4.4".into(),
            level: 2,
            description: "Verify application uses a single vetted access control mechanism".into(),
            detector_id: "access_control_mechanism".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V2.5.2".into(),
            level: 2,
            description: "Verify no OTP or time-based token weaknesses".into(),
            detector_id: "otp_weakness".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V6.1.1".into(),
            level: 2,
            description: "Verify regulated private data encrypted at rest".into(),
            detector_id: "encryption_at_rest".into(),
            automated: false,
        },
        AsvsRequirement {
            id: "V6.2.2".into(),
            level: 2,
            description: "Verify approved authenticated encryption modes".into(),
            detector_id: "crypto_failure".into(),
            automated: true,
        },
        AsvsRequirement {
            id: "V11.1.1".into(),
            level: 2,
            description: "Verify business logic flows are sequential and in order".into(),
            detector_id: "logic_flow".into(),
            automated: false,
        },
    ]
}

/// Generate an ASVS compliance report from detector findings.
///
/// `finding_detector_ids` contains the IDs of detectors that produced at least one finding.
/// `level` determines which ASVS requirements are included (L1, L1+L2, or all).
pub fn generate_asvs_report(
    finding_detector_ids: &[String],
    level: AsvsLevel,
) -> AsvsReport {
    let requirements = asvs_requirements();
    let max_level = match level {
        AsvsLevel::L1 => 1,
        AsvsLevel::L2 => 2,
        AsvsLevel::L3 => 3,
    };
    let filtered: Vec<_> = requirements
        .into_iter()
        .filter(|r| r.level <= max_level)
        .collect();

    let mut statuses = Vec::new();
    for req in filtered {
        let status = if !req.automated {
            AsvsStatus::NotAutomated
        } else if finding_detector_ids.contains(&req.detector_id) {
            AsvsStatus::Failed
        } else {
            AsvsStatus::Verified
        };
        let findings = if status == AsvsStatus::Failed {
            vec![format!("Detector {} found issues", req.detector_id)]
        } else {
            vec![]
        };
        statuses.push(AsvsRequirementStatus {
            requirement: req,
            status,
            findings,
        });
    }

    let coverage = AsvsCoverage {
        total: statuses.len(),
        automated: statuses.iter().filter(|s| s.requirement.automated).count(),
        verified: statuses.iter().filter(|s| s.status == AsvsStatus::Verified).count(),
        failed: statuses.iter().filter(|s| s.status == AsvsStatus::Failed).count(),
        manual_required: statuses
            .iter()
            .filter(|s| s.status == AsvsStatus::NotAutomated)
            .count(),
    };

    AsvsReport {
        level,
        requirements: statuses,
        coverage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asvs_l1_report_covers_expected_requirements() {
        let report = generate_asvs_report(&[], AsvsLevel::L1);
        // L1 should have at least 12 requirements (11 automated + 10 manual)
        assert!(
            report.requirements.len() >= 12,
            "L1 should have at least 12 requirements, got {}",
            report.requirements.len()
        );
        // All requirements should be level 1
        for status in &report.requirements {
            assert_eq!(status.requirement.level, 1);
        }
    }

    #[test]
    fn asvs_l2_includes_l1() {
        let l1_report = generate_asvs_report(&[], AsvsLevel::L1);
        let l2_report = generate_asvs_report(&[], AsvsLevel::L2);
        assert!(
            l2_report.requirements.len() > l1_report.requirements.len(),
            "L2 should include more requirements than L1"
        );
        // Every L1 requirement ID should appear in L2
        let l2_ids: Vec<&str> = l2_report
            .requirements
            .iter()
            .map(|s| s.requirement.id.as_str())
            .collect();
        for status in &l1_report.requirements {
            assert!(
                l2_ids.contains(&status.requirement.id.as_str()),
                "L2 report missing L1 requirement {}",
                status.requirement.id
            );
        }
    }

    #[test]
    fn failed_requirement_includes_findings() {
        let findings = vec!["sql_injection".to_string()];
        let report = generate_asvs_report(&findings, AsvsLevel::L1);
        let sql_reqs: Vec<_> = report
            .requirements
            .iter()
            .filter(|s| s.requirement.detector_id == "sql_injection")
            .collect();
        assert!(!sql_reqs.is_empty(), "Should have SQL injection requirements");
        for req in sql_reqs {
            assert_eq!(req.status, AsvsStatus::Failed);
            assert!(!req.findings.is_empty(), "Failed requirement should have findings");
        }
    }

    #[test]
    fn verified_when_no_findings() {
        let report = generate_asvs_report(&[], AsvsLevel::L1);
        let automated: Vec<_> = report
            .requirements
            .iter()
            .filter(|s| s.requirement.automated)
            .collect();
        assert!(!automated.is_empty());
        for req in automated {
            assert_eq!(req.status, AsvsStatus::Verified);
            assert!(req.findings.is_empty());
        }
    }

    #[test]
    fn not_automated_status_for_manual_reqs() {
        let report = generate_asvs_report(&[], AsvsLevel::L1);
        let manual: Vec<_> = report
            .requirements
            .iter()
            .filter(|s| !s.requirement.automated)
            .collect();
        assert!(!manual.is_empty(), "Should have manual requirements");
        for req in manual {
            assert_eq!(req.status, AsvsStatus::NotAutomated);
        }
    }

    #[test]
    fn coverage_counts_correct() {
        let findings = vec!["crypto_failure".to_string()];
        let report = generate_asvs_report(&findings, AsvsLevel::L1);
        let cov = &report.coverage;

        assert_eq!(cov.total, report.requirements.len());
        assert_eq!(
            cov.automated,
            report.requirements.iter().filter(|s| s.requirement.automated).count()
        );
        assert_eq!(
            cov.verified,
            report.requirements.iter().filter(|s| s.status == AsvsStatus::Verified).count()
        );
        assert_eq!(
            cov.failed,
            report.requirements.iter().filter(|s| s.status == AsvsStatus::Failed).count()
        );
        assert_eq!(
            cov.manual_required,
            report.requirements.iter().filter(|s| s.status == AsvsStatus::NotAutomated).count()
        );
        assert_eq!(
            cov.total,
            cov.verified + cov.failed + cov.manual_required
        );
    }

    #[test]
    fn asvs_requirements_have_valid_ids() {
        let reqs = asvs_requirements();
        for req in &reqs {
            assert!(
                req.id.starts_with('V'),
                "Requirement ID should start with 'V', got {}",
                req.id
            );
            assert!(!req.description.is_empty());
            assert!(req.level >= 1 && req.level <= 3);
        }
    }

    #[test]
    fn multiple_detectors_failed() {
        let findings = vec![
            "sql_injection".to_string(),
            "command_injection".to_string(),
            "ssrf".to_string(),
        ];
        let report = generate_asvs_report(&findings, AsvsLevel::L1);
        let failed_count = report
            .requirements
            .iter()
            .filter(|s| s.status == AsvsStatus::Failed)
            .count();
        // sql_injection maps to 2 reqs, command_injection to 1, ssrf to 1 = 4
        assert!(
            failed_count >= 4,
            "Expected at least 4 failed requirements, got {}",
            failed_count
        );
    }

    #[test]
    fn l3_includes_all_requirements() {
        let all_reqs = asvs_requirements();
        let report = generate_asvs_report(&[], AsvsLevel::L3);
        assert_eq!(report.requirements.len(), all_reqs.len());
    }

    #[test]
    fn report_level_matches_requested() {
        let report = generate_asvs_report(&[], AsvsLevel::L2);
        assert_eq!(report.level, AsvsLevel::L2);
    }
}
