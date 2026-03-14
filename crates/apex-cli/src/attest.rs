//! In-toto attestation generation for APEX analysis results.
//!
//! Generates SLSA-compatible attestation statements that provide auditable
//! evidence of security analysis in the software supply chain.

use serde::{Deserialize, Serialize};
use sha2::Digest;

/// In-toto Statement v1 (simplified).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InTotoStatement {
    #[serde(rename = "_type")]
    pub type_: String,
    pub subject: Vec<Subject>,
    #[serde(rename = "predicateType")]
    pub predicate_type: String,
    pub predicate: ApexPredicate,
}

/// A subject identified by name and cryptographic digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    pub name: String,
    pub digest: DigestSet,
}

/// Set of digests keyed by algorithm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestSet {
    pub sha256: String,
}

/// APEX-specific predicate attached to the attestation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApexPredicate {
    pub scanner: ScannerInfo,
    pub scan_date: String, // ISO 8601
    pub findings_summary: FindingsSummary,
    pub coverage_summary: CoverageSummary,
}

/// Information about the scanner that produced the results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerInfo {
    pub name: String,
    pub version: String,
}

/// Aggregated finding counts by severity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingsSummary {
    pub total: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
}

/// Coverage metrics from the analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageSummary {
    pub line_pct: f64,
    pub branch_pct: f64,
    pub mutation_score: Option<f64>,
}

/// Compute the SHA-256 hex digest of a string.
pub fn compute_sha256(input: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in result {
        use std::fmt::Write;
        write!(hex, "{:02x}", byte).unwrap();
    }
    hex
}

/// Format the current UTC time as an ISO 8601 string.
fn now_iso8601() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Simple UTC formatting without pulling in chrono
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Convert days since epoch to year-month-day
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's civil_from_days
    days += 719_468;
    let era = days / 146_097;
    let doe = days % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Generate an in-toto attestation for APEX analysis results.
pub fn generate_attestation(
    subject_name: &str,
    subject_digest: &str,
    findings_summary: FindingsSummary,
    coverage_summary: CoverageSummary,
    apex_version: &str,
) -> InTotoStatement {
    InTotoStatement {
        type_: "https://in-toto.io/Statement/v1".into(),
        subject: vec![Subject {
            name: subject_name.into(),
            digest: DigestSet {
                sha256: subject_digest.into(),
            },
        }],
        predicate_type: "https://apex.dev/attestation/v1".into(),
        predicate: ApexPredicate {
            scanner: ScannerInfo {
                name: "APEX".into(),
                version: apex_version.into(),
            },
            scan_date: now_iso8601(),
            findings_summary,
            coverage_summary,
        },
    }
}

/// Serialize an attestation statement to pretty-printed JSON.
pub fn attestation_to_json(stmt: &InTotoStatement) -> serde_json::Result<String> {
    serde_json::to_string_pretty(stmt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_findings() -> FindingsSummary {
        FindingsSummary {
            total: 0,
            critical: 0,
            high: 0,
            medium: 0,
            low: 0,
        }
    }

    fn empty_coverage() -> CoverageSummary {
        CoverageSummary {
            line_pct: 0.0,
            branch_pct: 0.0,
            mutation_score: None,
        }
    }

    #[test]
    fn attestation_follows_in_toto_spec() {
        let stmt = generate_attestation(
            "myproject",
            "abc123",
            FindingsSummary {
                total: 5,
                critical: 1,
                high: 2,
                medium: 1,
                low: 1,
            },
            CoverageSummary {
                line_pct: 85.0,
                branch_pct: 72.0,
                mutation_score: Some(0.65),
            },
            "0.1.0",
        );
        assert_eq!(stmt.type_, "https://in-toto.io/Statement/v1");
        assert_eq!(stmt.predicate_type, "https://apex.dev/attestation/v1");
    }

    #[test]
    fn attestation_includes_subject() {
        let stmt = generate_attestation(
            "myproject",
            "deadbeef",
            empty_findings(),
            CoverageSummary {
                line_pct: 100.0,
                branch_pct: 100.0,
                mutation_score: None,
            },
            "0.1.0",
        );
        assert_eq!(stmt.subject.len(), 1);
        assert_eq!(stmt.subject[0].name, "myproject");
        assert_eq!(stmt.subject[0].digest.sha256, "deadbeef");
    }

    #[test]
    fn attestation_findings_summary_correct() {
        let summary = FindingsSummary {
            total: 3,
            critical: 1,
            high: 1,
            medium: 0,
            low: 1,
        };
        let stmt = generate_attestation("p", "h", summary, empty_coverage(), "1.0.0");
        assert_eq!(stmt.predicate.findings_summary.critical, 1);
        assert_eq!(stmt.predicate.findings_summary.total, 3);
    }

    #[test]
    fn attestation_serializes_to_valid_json() {
        let stmt = generate_attestation(
            "test",
            "abc",
            empty_findings(),
            CoverageSummary {
                line_pct: 50.0,
                branch_pct: 30.0,
                mutation_score: None,
            },
            "0.1.0",
        );
        let json = attestation_to_json(&stmt).unwrap();
        assert!(json.contains("https://in-toto.io/Statement/v1"));
        assert!(json.contains("APEX"));
        // Verify it roundtrips
        let parsed: InTotoStatement = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.subject[0].name, "test");
    }

    #[test]
    fn compute_sha256_deterministic() {
        let hash1 = compute_sha256("hello world");
        let hash2 = compute_sha256("hello world");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // 32 bytes hex
    }

    #[test]
    fn compute_sha256_known_value() {
        // SHA-256("hello world") = b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
        let hash = compute_sha256("hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn scanner_info_correct() {
        let stmt = generate_attestation("p", "h", empty_findings(), empty_coverage(), "2.0.0");
        assert_eq!(stmt.predicate.scanner.name, "APEX");
        assert_eq!(stmt.predicate.scanner.version, "2.0.0");
    }

    #[test]
    fn scan_date_is_iso8601() {
        let stmt = generate_attestation("p", "h", empty_findings(), empty_coverage(), "0.1.0");
        // Should match YYYY-MM-DDTHH:MM:SSZ pattern
        assert!(stmt.predicate.scan_date.ends_with('Z'));
        assert!(stmt.predicate.scan_date.contains('T'));
        assert_eq!(stmt.predicate.scan_date.len(), 20);
    }

    #[test]
    fn coverage_with_mutation_score() {
        let stmt = generate_attestation(
            "p",
            "h",
            empty_findings(),
            CoverageSummary {
                line_pct: 90.0,
                branch_pct: 80.0,
                mutation_score: Some(0.75),
            },
            "0.1.0",
        );
        assert_eq!(stmt.predicate.coverage_summary.mutation_score, Some(0.75));
        assert!((stmt.predicate.coverage_summary.line_pct - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn coverage_without_mutation_score_serializes_null() {
        let stmt = generate_attestation("p", "h", empty_findings(), empty_coverage(), "0.1.0");
        let json = attestation_to_json(&stmt).unwrap();
        assert!(json.contains("\"mutation_score\": null"));
    }
}
