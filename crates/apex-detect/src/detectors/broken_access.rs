//! Broken access control detector — identifies missing auth/CSRF patterns (CWE-862).

use crate::finding::{Finding, FindingCategory, Severity};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;
use uuid::Uuid;

static ROUTE_DECORATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"@(app|router)\.(get|post|put|delete|route|patch)\("#).unwrap());
static DJANGO_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"def\s+\w+\(request"#).unwrap());

/// Auth decorators that indicate protected endpoints.
const AUTH_DECORATORS: &[&str] = &[
    "login_required",
    "requires_auth",
    "permission_required",
    "jwt_required",
    "auth_required",
    "authenticated",
    "IsAuthenticated",
    "permissions_classes",
];

/// Scan source code for broken access control vulnerabilities.
pub fn scan_broken_access(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    let route_decorator = &*ROUTE_DECORATOR;
    let django_url = &*DJANGO_URL;

    let lines: Vec<&str> = source.lines().collect();

    for (line_num, line) in lines.iter().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip comments.
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        // Check for route handlers without auth decorators.
        if route_decorator.is_match(trimmed) {
            // Look at surrounding lines (up to 5 above) for auth decorators.
            let start = line_num.saturating_sub(5);
            let context: String = lines[start..=line_num]
                .iter()
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join("\n");

            let has_auth = AUTH_DECORATORS.iter().any(|d| context.contains(d));
            if !has_auth {
                findings.push(make_finding(
                    file_path,
                    line_1based,
                    "Route handler without authentication decorator",
                    &format!(
                        "Route at line {line_1based} has no auth decorator. \
                         Ensure authentication is enforced."
                    ),
                    "Add @login_required, @requires_auth, or equivalent auth decorator.",
                    862,
                ));
            }
        }

        // Check for direct object reference without permission check.
        if trimmed.contains("objects.get(") && trimmed.contains("request.") {
            // Look for permission check in nearby lines.
            let start = line_num.saturating_sub(5);
            let end = (line_num + 5).min(lines.len().saturating_sub(1));
            let context: String = lines[start..=end]
                .iter()
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join("\n");

            let has_perm_check = context.contains("has_perm")
                || context.contains("check_object_permissions")
                || context.contains("get_object_or_404");

            if !has_perm_check {
                findings.push(make_finding(
                    file_path,
                    line_1based,
                    "Insecure direct object reference (IDOR)",
                    &format!(
                        "Direct object lookup with user input at line {line_1based} \
                         without permission check."
                    ),
                    "Add permission checks before accessing objects by user-supplied ID.",
                    862,
                ));
            }
        }

        // Check for user-controlled role/privilege escalation.
        if (trimmed.contains("is_admin = request.") || trimmed.contains("role = request."))
            && !trimmed.starts_with('#')
        {
            findings.push(make_finding(
                file_path,
                line_1based,
                "User-controlled privilege assignment",
                &format!(
                    "Privilege level set from user input at line {line_1based}. \
                     Never trust client-supplied role values."
                ),
                "Derive roles from server-side session/token, not from request parameters.",
                862,
            ));
        }

        // Check for missing CSRF protection in forms (only state-changing methods).
        if trimmed.contains("<form") && trimmed.contains("method") {
            // Only flag POST, PUT, DELETE, PATCH — GET forms don't need CSRF.
            let lower = trimmed.to_lowercase();
            let is_state_changing = ["post", "put", "delete", "patch"].iter().any(|m| {
                lower.contains(&format!("method=\"{m}\""))
                    || lower.contains(&format!("method='{m}'"))
            });
            if is_state_changing {
                // Look ahead for csrf token.
                let end = (line_num + 10).min(lines.len().saturating_sub(1));
                let form_context: String = lines[line_num..=end]
                    .iter()
                    .map(|l| l.trim())
                    .collect::<Vec<_>>()
                    .join("\n");

                if !form_context.contains("csrf")
                    && !form_context.contains("_token")
                    && !form_context.contains("antiforgery")
                {
                    findings.push(make_finding(
                        file_path,
                        line_1based,
                        "Form without CSRF protection",
                        &format!(
                            "Form at line {line_1based} appears to lack CSRF token. \
                             This may allow cross-site request forgery."
                        ),
                        "Add {% csrf_token %} or equivalent CSRF protection to forms.",
                        352,
                    ));
                }
            }
        }

        // Detect Django view function without auth.
        if django_url.is_match(trimmed) {
            let start = line_num.saturating_sub(3);
            let context: String = lines[start..=line_num]
                .iter()
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join("\n");

            let has_auth = AUTH_DECORATORS.iter().any(|d| context.contains(d));
            if !has_auth && !trimmed.contains("def login") && !trimmed.contains("def register") {
                findings.push(make_finding(
                    file_path,
                    line_1based,
                    "Django view without authentication decorator",
                    &format!(
                        "View function at line {line_1based} accepts request but has no \
                         auth decorator."
                    ),
                    "Add @login_required or appropriate permission decorator.",
                    862,
                ));
            }
        }
    }

    findings
}

fn make_finding(
    file_path: &str,
    line: u32,
    title: &str,
    description: &str,
    suggestion: &str,
    cwe: u32,
) -> Finding {
    Finding {
        id: Uuid::new_v4(),
        detector: "broken_access".into(),
        severity: Severity::High,
        category: FindingCategory::SecuritySmell,
        file: PathBuf::from(file_path),
        line: Some(line),
        title: title.into(),
        description: description.into(),
        evidence: vec![],
        covered: false,
        suggestion: suggestion.into(),
        explanation: None,
        fix: None,
        cwe_ids: vec![cwe],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_route_without_auth() {
        let source = "@app.route(\"/admin\")\ndef admin_panel():\n    return render()\n";
        let findings = scan_broken_access(source, "views.py");
        assert!(!findings.is_empty());
        assert!(findings[0].title.contains("authentication"));
    }

    #[test]
    fn detect_direct_object_reference() {
        let source = "user = User.objects.get(id=request.GET['id'])\n";
        let findings = scan_broken_access(source, "views.py");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&862));
    }

    #[test]
    fn skip_with_login_required() {
        let source = "@login_required\n@app.route(\"/admin\")\ndef admin():\n    pass\n";
        let findings = scan_broken_access(source, "views.py");
        let route_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Route handler"))
            .collect();
        assert!(route_findings.is_empty());
    }

    #[test]
    fn detect_user_controlled_role() {
        let source = "is_admin = request.POST['is_admin']\n";
        let findings = scan_broken_access(source, "auth.py");
        assert!(!findings.is_empty());
        assert!(findings[0].title.contains("privilege"));
    }

    #[test]
    fn detect_missing_csrf() {
        let source = "<form method=\"POST\" action=\"/update\">\n<input name=\"val\">\n</form>\n";
        let findings = scan_broken_access(source, "template.html");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&352));
    }

    #[test]
    fn no_false_positive_on_authenticated_endpoint() {
        let source = "@requires_auth\n@app.post(\"/data\")\ndef post_data():\n    pass\n";
        let findings = scan_broken_access(source, "api.py");
        let route_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.title.contains("Route handler"))
            .collect();
        assert!(route_findings.is_empty());
    }

    #[test]
    fn detect_in_django_views() {
        let source = "def user_profile(request):\n    return render(request, 'profile.html')\n";
        let findings = scan_broken_access(source, "views.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn detect_in_flask_routes() {
        let source = "@app.get(\"/users\")\ndef list_users():\n    return jsonify(users)\n";
        let findings = scan_broken_access(source, "routes.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn no_csrf_flag_on_get_form() {
        let source = "<form method=\"get\" action=\"/search\">\n<input name=\"q\">\n</form>\n";
        let findings = scan_broken_access(source, "search.html");
        let csrf_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.cwe_ids.contains(&352))
            .collect();
        assert!(
            csrf_findings.is_empty(),
            "GET form should not trigger CSRF warning"
        );
    }

    #[test]
    fn csrf_flag_on_post_form() {
        let source = "<form method=\"POST\" action=\"/update\">\n<input name=\"val\">\n</form>\n";
        let findings = scan_broken_access(source, "update.html");
        let csrf_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.cwe_ids.contains(&352))
            .collect();
        assert!(
            !csrf_findings.is_empty(),
            "POST form without CSRF should be flagged"
        );
    }
}
