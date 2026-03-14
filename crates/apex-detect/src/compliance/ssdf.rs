//! NIST SSDF (Secure Software Development Framework) compliance tags.
//!
//! Maps APEX capabilities to SSDF practices and tasks, generating
//! compliance evidence reports.

/// A single SSDF task with APEX coverage information.
#[derive(Debug, Clone)]
pub struct SsdfTask {
    /// SSDF task ID, e.g. "PW.7.1".
    pub id: String,
    /// SSDF practice name, e.g. "Review Human-Readable Code".
    pub practice: String,
    /// Task description.
    pub description: String,
    /// Whether APEX satisfies this task.
    pub apex_satisfies: bool,
    /// Evidence of how APEX satisfies the task (empty if not satisfied).
    pub evidence: String,
}

/// SSDF compliance summary report.
#[derive(Debug)]
pub struct SsdfReport {
    pub tasks: Vec<SsdfTask>,
    pub satisfied_count: usize,
    pub total_count: usize,
}

/// Return the SSDF tasks that APEX can map to.
///
/// Tasks with `apex_satisfies: true` have evidence describing how APEX covers them.
/// Tasks with `apex_satisfies: false` are included for completeness but APEX does not address them.
pub fn ssdf_tasks_satisfied() -> Vec<SsdfTask> {
    vec![
        SsdfTask {
            id: "PO.3.1".into(),
            practice: "Implement Supporting Toolchains".into(),
            description: "Use security-focused tools in the toolchain".into(),
            apex_satisfies: true,
            evidence: "APEX provides SAST, SCA, and taint analysis".into(),
        },
        SsdfTask {
            id: "PO.3.2".into(),
            practice: "Implement Supporting Toolchains".into(),
            description: "Follow recommended security practices for tool configuration".into(),
            apex_satisfies: true,
            evidence: "APEX enforces secure defaults and provides configuration validation".into(),
        },
        SsdfTask {
            id: "PW.1.1".into(),
            practice: "Design Software to Meet Security Requirements".into(),
            description: "Use threat modeling to inform design decisions".into(),
            apex_satisfies: true,
            evidence: "APEX threat-model generates STRIDE-based threat models from code".into(),
        },
        SsdfTask {
            id: "PW.4.1".into(),
            practice: "Reuse Existing Well-Secured Software".into(),
            description: "Acquire and maintain well-secured components".into(),
            apex_satisfies: true,
            evidence: "APEX dep-audit scans dependencies for known vulnerabilities".into(),
        },
        SsdfTask {
            id: "PW.4.4".into(),
            practice: "Reuse Existing Well-Secured Software".into(),
            description: "Verify integrity of acquired software".into(),
            apex_satisfies: true,
            evidence: "APEX SBOM generation and lockfile auditing verify component integrity".into(),
        },
        SsdfTask {
            id: "PW.7.1".into(),
            practice: "Review Human-Readable Code".into(),
            description: "Perform static analysis to identify vulnerabilities".into(),
            apex_satisfies: true,
            evidence: "APEX detect performs CPG-based static analysis".into(),
        },
        SsdfTask {
            id: "PW.7.2".into(),
            practice: "Review Human-Readable Code".into(),
            description: "Perform peer review of code changes".into(),
            apex_satisfies: false,
            evidence: "".into(),
        },
        SsdfTask {
            id: "PW.8.1".into(),
            practice: "Test Executable Code".into(),
            description: "Perform dynamic analysis including fuzzing".into(),
            apex_satisfies: true,
            evidence: "APEX run performs coverage-guided fuzzing and concolic testing".into(),
        },
        SsdfTask {
            id: "PW.8.2".into(),
            practice: "Test Executable Code".into(),
            description: "Perform penetration testing".into(),
            apex_satisfies: false,
            evidence: "".into(),
        },
        SsdfTask {
            id: "RV.1.1".into(),
            practice: "Identify and Confirm Vulnerabilities".into(),
            description: "Gather information from security tools".into(),
            apex_satisfies: true,
            evidence: "APEX report aggregates all findings in SARIF format".into(),
        },
        SsdfTask {
            id: "RV.1.2".into(),
            practice: "Identify and Confirm Vulnerabilities".into(),
            description: "Review and triage vulnerability reports".into(),
            apex_satisfies: true,
            evidence: "APEX CVSS scoring and severity classification enable triage".into(),
        },
        SsdfTask {
            id: "PS.1.1".into(),
            practice: "Protect All Forms of Code".into(),
            description: "Store all code in a version control system".into(),
            apex_satisfies: false,
            evidence: "".into(),
        },
        SsdfTask {
            id: "PS.2.1".into(),
            practice: "Protect All Forms of Code".into(),
            description: "Protect integrity of code from unauthorized changes".into(),
            apex_satisfies: false,
            evidence: "".into(),
        },
    ]
}

/// Generate an SSDF compliance report.
pub fn generate_ssdf_report() -> SsdfReport {
    let tasks = ssdf_tasks_satisfied();
    let satisfied = tasks.iter().filter(|t| t.apex_satisfies).count();
    let total = tasks.len();
    SsdfReport {
        tasks,
        satisfied_count: satisfied,
        total_count: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssdf_report_has_tasks() {
        let report = generate_ssdf_report();
        assert!(
            !report.tasks.is_empty(),
            "SSDF report should contain tasks"
        );
    }

    #[test]
    fn ssdf_satisfied_count_matches() {
        let report = generate_ssdf_report();
        let manual_count = report.tasks.iter().filter(|t| t.apex_satisfies).count();
        assert_eq!(report.satisfied_count, manual_count);
    }

    #[test]
    fn ssdf_pw_7_1_is_satisfied() {
        let tasks = ssdf_tasks_satisfied();
        let pw71 = tasks.iter().find(|t| t.id == "PW.7.1");
        assert!(pw71.is_some(), "PW.7.1 should exist");
        let pw71 = pw71.unwrap();
        assert!(pw71.apex_satisfies, "PW.7.1 should be satisfied by APEX");
        assert!(!pw71.evidence.is_empty());
    }

    #[test]
    fn ssdf_unsatisfied_task_has_empty_evidence() {
        let tasks = ssdf_tasks_satisfied();
        let unsatisfied: Vec<_> = tasks.iter().filter(|t| !t.apex_satisfies).collect();
        assert!(!unsatisfied.is_empty(), "Should have unsatisfied tasks");
        for task in unsatisfied {
            assert!(
                task.evidence.is_empty(),
                "Unsatisfied task {} should have empty evidence",
                task.id
            );
        }
    }

    #[test]
    fn ssdf_report_total_count() {
        let report = generate_ssdf_report();
        assert_eq!(report.total_count, report.tasks.len());
        assert!(report.satisfied_count <= report.total_count);
    }

    #[test]
    fn ssdf_task_ids_are_valid_format() {
        let tasks = ssdf_tasks_satisfied();
        for task in &tasks {
            // SSDF task IDs follow pattern: XX.N.N
            assert!(
                task.id.contains('.'),
                "Task ID {} should contain dots",
                task.id
            );
            assert!(!task.practice.is_empty());
            assert!(!task.description.is_empty());
        }
    }
}
