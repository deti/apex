//! Accessibility Rule Checking — scans JSX/HTML for WCAG violations.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub struct A11yIssue {
    pub rule: String,
    pub wcag: String,
    pub severity: A11ySeverity,
    pub file: PathBuf,
    pub line: u32,
    pub evidence: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum A11ySeverity {
    Critical,
    Serious,
    Moderate,
    Minor,
}

#[derive(Debug, Clone, Serialize)]
pub struct A11yReport {
    pub issues: Vec<A11yIssue>,
    pub critical_count: usize,
    pub serious_count: usize,
    pub files_scanned: usize,
}

// Match patterns (without look-ahead — Rust regex crate doesn't support it).
// We match the tag then check for absence of required attributes in code.
static IMG_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<img\b[^>]*>"#).unwrap());

static ALT_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\balt\s*="#).unwrap());

static INPUT_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<input\b[^>]*>"#).unwrap());

static ARIA_LABEL_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\baria-label"#).unwrap());

static BUTTON_EMPTY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<button[^>]*>\s*</button>"#).unwrap());

static ONCLICK_DIV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<div\b[^>]*\bonClick\b[^>]*>"#).unwrap());

static HTML_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<html\b[^>]*>"#).unwrap());

static LANG_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\blang\s*="#).unwrap());

static AUTOFOCUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bautofocus\b"#).unwrap());

/// A rule that matches when a tag is present but a required attribute is absent.
struct AbsenceRule {
    tag_re: &'static LazyLock<Regex>,
    absent_re: &'static LazyLock<Regex>,
    rule: &'static str,
    wcag: &'static str,
    severity: A11ySeverity,
    suggestion: &'static str,
}

/// A rule that matches when a regex matches (simple presence check).
struct PresenceRule {
    re: &'static LazyLock<Regex>,
    rule: &'static str,
    wcag: &'static str,
    severity: A11ySeverity,
    suggestion: &'static str,
}

pub fn scan_accessibility(source_cache: &HashMap<PathBuf, String>) -> A11yReport {
    let mut issues = Vec::new();
    let mut files_scanned = 0;

    let absence_rules = vec![
        AbsenceRule {
            tag_re: &IMG_TAG,
            absent_re: &ALT_ATTR,
            rule: "img-alt",
            wcag: "WCAG 1.1.1",
            severity: A11ySeverity::Critical,
            suggestion: "Add alt attribute to <img> element",
        },
        AbsenceRule {
            tag_re: &INPUT_TAG,
            absent_re: &ARIA_LABEL_ATTR,
            rule: "input-label",
            wcag: "WCAG 1.3.1",
            severity: A11ySeverity::Serious,
            suggestion: "Add aria-label or associated <label> to <input>",
        },
        AbsenceRule {
            tag_re: &HTML_TAG,
            absent_re: &LANG_ATTR,
            rule: "html-has-lang",
            wcag: "WCAG 3.1.1",
            severity: A11ySeverity::Serious,
            suggestion: "Add lang attribute to <html> element",
        },
    ];

    let presence_rules = vec![
        PresenceRule {
            re: &BUTTON_EMPTY,
            rule: "button-content",
            wcag: "WCAG 4.1.2",
            severity: A11ySeverity::Serious,
            suggestion: "Add text content or aria-label to <button>",
        },
        PresenceRule {
            re: &ONCLICK_DIV,
            rule: "click-events-have-key-events",
            wcag: "WCAG 2.1.1",
            severity: A11ySeverity::Critical,
            suggestion: "Use <button> instead of <div onClick> for interactive elements",
        },
        PresenceRule {
            re: &AUTOFOCUS,
            rule: "no-autofocus",
            wcag: "WCAG 2.4.3",
            severity: A11ySeverity::Moderate,
            suggestion: "Avoid autofocus — it can disorient screen reader users",
        },
    ];

    for (path, source) in source_cache {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "jsx" | "tsx" | "html" | "htm" | "vue" | "svelte") {
            continue;
        }
        files_scanned += 1;

        for (line_num, line) in source.lines().enumerate() {
            let ln = (line_num + 1) as u32;

            // Check absence rules: tag present but required attr missing
            for rule in &absence_rules {
                if let Some(m) = rule.tag_re.find(line) {
                    let tag_text = m.as_str();
                    if !rule.absent_re.is_match(tag_text) {
                        issues.push(A11yIssue {
                            rule: rule.rule.to_string(),
                            wcag: rule.wcag.to_string(),
                            severity: rule.severity,
                            file: path.clone(),
                            line: ln,
                            evidence: line.trim().to_string(),
                            suggestion: rule.suggestion.to_string(),
                        });
                    }
                }
            }

            // Check presence rules: pattern match = violation
            for rule in &presence_rules {
                if rule.re.is_match(line) {
                    issues.push(A11yIssue {
                        rule: rule.rule.to_string(),
                        wcag: rule.wcag.to_string(),
                        severity: rule.severity,
                        file: path.clone(),
                        line: ln,
                        evidence: line.trim().to_string(),
                        suggestion: rule.suggestion.to_string(),
                    });
                }
            }
        }
    }

    let critical_count = issues
        .iter()
        .filter(|i| matches!(i.severity, A11ySeverity::Critical))
        .count();
    let serious_count = issues
        .iter()
        .filter(|i| matches!(i.severity, A11ySeverity::Serious))
        .count();

    A11yReport {
        issues,
        critical_count,
        serious_count,
        files_scanned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_img_without_alt() {
        let mut src = HashMap::new();
        src.insert(
            PathBuf::from("page.jsx"),
            r#"<img src="photo.jpg" />"#.into(),
        );
        let r = scan_accessibility(&src);
        assert_eq!(r.critical_count, 1);
        assert!(r.issues[0].rule.contains("img-alt"));
    }

    #[test]
    fn img_with_alt_passes() {
        let mut src = HashMap::new();
        src.insert(
            PathBuf::from("page.jsx"),
            r#"<img src="photo.jpg" alt="A photo" />"#.into(),
        );
        let r = scan_accessibility(&src);
        assert_eq!(r.critical_count, 0);
    }

    #[test]
    fn detects_empty_button() {
        let mut src = HashMap::new();
        src.insert(PathBuf::from("btn.tsx"), r#"<button></button>"#.into());
        let r = scan_accessibility(&src);
        assert!(r.issues.iter().any(|i| i.rule == "button-content"));
    }

    #[test]
    fn detects_div_onclick() {
        let mut src = HashMap::new();
        src.insert(
            PathBuf::from("menu.jsx"),
            r#"<div onClick={toggle}>Menu</div>"#.into(),
        );
        let r = scan_accessibility(&src);
        assert!(r
            .issues
            .iter()
            .any(|i| i.rule == "click-events-have-key-events"));
    }

    #[test]
    fn skips_non_jsx_files() {
        let mut src = HashMap::new();
        src.insert(
            PathBuf::from("app.py"),
            r#"<img src="photo.jpg" />"#.into(),
        );
        let r = scan_accessibility(&src);
        assert_eq!(r.files_scanned, 0);
    }

    #[test]
    fn empty_source_no_issues() {
        let r = scan_accessibility(&HashMap::new());
        assert!(r.issues.is_empty());
    }

    #[test]
    fn detects_html_no_lang() {
        let mut src = HashMap::new();
        src.insert(
            PathBuf::from("index.html"),
            r#"<html><head></head></html>"#.into(),
        );
        let r = scan_accessibility(&src);
        assert!(r.issues.iter().any(|i| i.rule == "html-has-lang"));
    }
}
