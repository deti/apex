//! Automated STRIDE threat matrix generation.
//!
//! Analyzes source code to detect which STRIDE threat categories have
//! mitigations present and which are missing, producing a risk-scored matrix.

use regex::Regex;

/// The six STRIDE threat categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StrideCategory {
    Spoofing,
    Tampering,
    Repudiation,
    InformationDisclosure,
    DenialOfService,
    ElevationOfPrivilege,
}

impl std::fmt::Display for StrideCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spoofing => write!(f, "Spoofing"),
            Self::Tampering => write!(f, "Tampering"),
            Self::Repudiation => write!(f, "Repudiation"),
            Self::InformationDisclosure => write!(f, "Information Disclosure"),
            Self::DenialOfService => write!(f, "Denial of Service"),
            Self::ElevationOfPrivilege => write!(f, "Elevation of Privilege"),
        }
    }
}

/// Risk level for a STRIDE category based on mitigation coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// A mitigation that can be detected via regex pattern matching.
pub struct StrideMitigation {
    pub name: String,
    pub description: String,
    pub detection_pattern: String,
    pub present: bool,
}

/// A single entry in the STRIDE threat matrix.
pub struct StrideEntry {
    pub category: StrideCategory,
    pub risk_level: RiskLevel,
    pub mitigations_found: Vec<String>,
    pub mitigations_missing: Vec<String>,
    pub recommendations: Vec<String>,
}

/// The complete STRIDE threat matrix for a codebase.
pub struct StrideMatrix {
    pub entries: Vec<StrideEntry>,
}

/// Define expected mitigations per STRIDE category.
fn stride_mitigations() -> Vec<(StrideCategory, Vec<StrideMitigation>)> {
    vec![
        (
            StrideCategory::Spoofing,
            vec![
                mitigation(
                    "Authentication middleware",
                    r"login_required|@auth|authenticate|jwt_required|@requires_auth",
                    "auth decorator/middleware on routes",
                ),
                mitigation(
                    "Session management",
                    r"session|cookie|token_verify",
                    "session handling",
                ),
                mitigation(
                    "Multi-factor auth",
                    r"mfa|two_factor|totp|2fa",
                    "MFA implementation",
                ),
            ],
        ),
        (
            StrideCategory::Tampering,
            vec![
                mitigation(
                    "CSRF protection",
                    r"csrf|CSRFProtect|@csrf_exempt|csrf_token",
                    "CSRF token usage",
                ),
                mitigation(
                    "Input validation",
                    r"validate|sanitize|clean|escape|bleach",
                    "input validation/sanitization",
                ),
                mitigation(
                    "Integrity checks",
                    r"hmac|signature|checksum|hash_verify",
                    "data integrity verification",
                ),
            ],
        ),
        (
            StrideCategory::Repudiation,
            vec![
                mitigation(
                    "Audit logging",
                    r"audit_log|logger\.info|logging\.getLogger|AuditTrail",
                    "audit log calls",
                ),
                mitigation(
                    "Transaction logging",
                    r"log_transaction|record_event|emit_event",
                    "transaction recording",
                ),
            ],
        ),
        (
            StrideCategory::InformationDisclosure,
            vec![
                mitigation(
                    "Error handling",
                    r"(?s)try.*except|catch|error_handler|@app\.errorhandler",
                    "error handling",
                ),
                mitigation(
                    "Sensitive data protection",
                    r"mask|redact|encrypt|hash_password|bcrypt",
                    "data masking/encryption",
                ),
                mitigation(
                    "Debug mode disabled",
                    r"DEBUG.*=.*False|debug.*=.*false",
                    "debug mode off in prod config",
                ),
            ],
        ),
        (
            StrideCategory::DenialOfService,
            vec![
                mitigation(
                    "Rate limiting",
                    r"rate_limit|throttle|RateLimit|slowapi",
                    "rate limiting",
                ),
                mitigation(
                    "Timeout configuration",
                    r"timeout|connect_timeout|read_timeout",
                    "timeout settings",
                ),
                mitigation(
                    "Resource limits",
                    r"max_content_length|MAX_UPLOAD|limit.*size",
                    "resource bounds",
                ),
            ],
        ),
        (
            StrideCategory::ElevationOfPrivilege,
            vec![
                mitigation(
                    "Authorization checks",
                    r"permission|role_required|has_permission|@admin_required|rbac",
                    "authorization/RBAC",
                ),
                mitigation(
                    "Least privilege",
                    r"readonly|read_only|minimum_privilege",
                    "least privilege patterns",
                ),
            ],
        ),
    ]
}

fn mitigation(name: &str, pattern: &str, desc: &str) -> StrideMitigation {
    StrideMitigation {
        name: name.into(),
        description: desc.into(),
        detection_pattern: pattern.into(),
        present: false,
    }
}

/// Analyze source code for STRIDE mitigations and produce a threat matrix.
pub fn analyze_stride(source: &str) -> StrideMatrix {
    let categories = stride_mitigations();
    let mut entries = Vec::new();

    // Pre-compile all regex patterns to avoid compiling inside loops.
    let compiled: Vec<(StrideCategory, Vec<(&StrideMitigation, Regex)>)> = categories
        .iter()
        .map(|(cat, mits)| {
            let patterns = mits
                .iter()
                .map(|mit| {
                    let re = Regex::new(&mit.detection_pattern)
                        .unwrap_or_else(|_| Regex::new(r"$^").unwrap());
                    (mit, re)
                })
                .collect();
            (*cat, patterns)
        })
        .collect();

    for (category, mitigations) in &compiled {
        let mut found = Vec::new();
        let mut missing = Vec::new();

        for (mit, re) in mitigations {
            if re.is_match(source) {
                found.push(mit.name.clone());
            } else {
                missing.push(mit.name.clone());
            }
        }

        let risk_level = if missing.len() > found.len() {
            RiskLevel::High
        } else if missing.is_empty() {
            RiskLevel::Low
        } else {
            RiskLevel::Medium
        };

        let recommendations: Vec<String> = missing
            .iter()
            .map(|m| format!("Add {} to mitigate {} threats", m, category))
            .collect();

        entries.push(StrideEntry {
            category: *category,
            risk_level,
            mitigations_found: found,
            mitigations_missing: missing,
            recommendations,
        });
    }

    StrideMatrix { entries }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_detects_auth_middleware() {
        let source = r#"
@login_required
def dashboard(request):
    return render(request, "dashboard.html")
"#;
        let matrix = analyze_stride(source);
        let spoofing = matrix
            .entries
            .iter()
            .find(|e| e.category == StrideCategory::Spoofing)
            .unwrap();
        assert!(spoofing
            .mitigations_found
            .contains(&"Authentication middleware".to_string()));
    }

    #[test]
    fn stride_detects_missing_auth() {
        let source = r#"
def dashboard(request):
    return render(request, "dashboard.html")
"#;
        let matrix = analyze_stride(source);
        let spoofing = matrix
            .entries
            .iter()
            .find(|e| e.category == StrideCategory::Spoofing)
            .unwrap();
        assert!(spoofing
            .mitigations_missing
            .contains(&"Authentication middleware".to_string()));
        assert!(spoofing
            .mitigations_missing
            .contains(&"Multi-factor auth".to_string()));
    }

    #[test]
    fn stride_detects_csrf_protection() {
        let source = r#"
from flask_wtf.csrf import CSRFProtect
csrf = CSRFProtect(app)
"#;
        let matrix = analyze_stride(source);
        let tampering = matrix
            .entries
            .iter()
            .find(|e| e.category == StrideCategory::Tampering)
            .unwrap();
        assert!(tampering
            .mitigations_found
            .contains(&"CSRF protection".to_string()));
    }

    #[test]
    fn stride_detects_missing_csrf() {
        let source = r#"
app = Flask(__name__)
"#;
        let matrix = analyze_stride(source);
        let tampering = matrix
            .entries
            .iter()
            .find(|e| e.category == StrideCategory::Tampering)
            .unwrap();
        assert!(tampering
            .mitigations_missing
            .contains(&"CSRF protection".to_string()));
    }

    #[test]
    fn stride_detects_audit_logging() {
        let source = r#"
import logging
logger = logging.getLogger(__name__)
logger.info("User logged in: %s", user.email)
"#;
        let matrix = analyze_stride(source);
        let repudiation = matrix
            .entries
            .iter()
            .find(|e| e.category == StrideCategory::Repudiation)
            .unwrap();
        assert!(repudiation
            .mitigations_found
            .contains(&"Audit logging".to_string()));
    }

    #[test]
    fn stride_detects_missing_logging() {
        let source = r#"
def process_payment(amount):
    charge(amount)
"#;
        let matrix = analyze_stride(source);
        let repudiation = matrix
            .entries
            .iter()
            .find(|e| e.category == StrideCategory::Repudiation)
            .unwrap();
        assert!(repudiation
            .mitigations_missing
            .contains(&"Audit logging".to_string()));
        assert!(repudiation
            .mitigations_missing
            .contains(&"Transaction logging".to_string()));
    }

    #[test]
    fn stride_detects_rate_limiting() {
        let source = r#"
from slowapi import Limiter
limiter = Limiter(key_func=get_remote_address)
"#;
        let matrix = analyze_stride(source);
        let dos = matrix
            .entries
            .iter()
            .find(|e| e.category == StrideCategory::DenialOfService)
            .unwrap();
        assert!(dos
            .mitigations_found
            .contains(&"Rate limiting".to_string()));
    }

    #[test]
    fn stride_detects_authorization() {
        let source = r#"
@role_required("admin")
def admin_panel(request):
    pass
"#;
        let matrix = analyze_stride(source);
        let eop = matrix
            .entries
            .iter()
            .find(|e| e.category == StrideCategory::ElevationOfPrivilege)
            .unwrap();
        assert!(eop
            .mitigations_found
            .contains(&"Authorization checks".to_string()));
    }

    #[test]
    fn stride_risk_high_when_most_missing() {
        // Empty source — all mitigations missing
        let matrix = analyze_stride("");
        for entry in &matrix.entries {
            assert_eq!(
                entry.risk_level,
                RiskLevel::High,
                "Expected High risk for {:?} with no mitigations",
                entry.category
            );
        }
    }

    #[test]
    fn stride_risk_low_when_all_present() {
        let source = r#"
@login_required
session.start()
totp_verify(code)
csrf_token = generate_csrf()
validate(input)
hmac.verify(sig)
audit_log("action", user)
log_transaction(tx_id)
try:
    something()
except Exception:
    pass
encrypt(data)
DEBUG = False
rate_limit(100)
timeout = 30
max_content_length = 1024
@role_required("admin")
readonly = True
"#;
        let matrix = analyze_stride(source);
        for entry in &matrix.entries {
            assert_eq!(
                entry.risk_level,
                RiskLevel::Low,
                "Expected Low risk for {:?} with all mitigations present, found: {:?}, missing: {:?}",
                entry.category,
                entry.mitigations_found,
                entry.mitigations_missing
            );
        }
    }

    #[test]
    fn stride_full_analysis_covers_all_categories() {
        let matrix = analyze_stride("");
        assert_eq!(matrix.entries.len(), 6);
        let categories: Vec<_> = matrix.entries.iter().map(|e| e.category).collect();
        assert!(categories.contains(&StrideCategory::Spoofing));
        assert!(categories.contains(&StrideCategory::Tampering));
        assert!(categories.contains(&StrideCategory::Repudiation));
        assert!(categories.contains(&StrideCategory::InformationDisclosure));
        assert!(categories.contains(&StrideCategory::DenialOfService));
        assert!(categories.contains(&StrideCategory::ElevationOfPrivilege));
    }

    #[test]
    fn stride_recommendations_generated() {
        let matrix = analyze_stride("");
        for entry in &matrix.entries {
            assert_eq!(
                entry.recommendations.len(),
                entry.mitigations_missing.len(),
                "Should have one recommendation per missing mitigation for {:?}",
                entry.category
            );
            for rec in &entry.recommendations {
                assert!(
                    rec.starts_with("Add "),
                    "Recommendation should start with 'Add ': {}",
                    rec
                );
            }
        }
    }
}
