use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::is_comment;
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct WallClockMisuseDetector;

/// Variable name fragments that indicate duration-measurement intent.
static DURATION_NAMES: &[&str] = &[
    "start", "begin", "before", "t0", "t1", "t_start", "t_begin", "start_time", "begin_time",
];

/// Wall-clock APIs per language that are wrong for duration measurement.
static RUST_WALL_CLOCK: &[&str] = &["SystemTime::now()", "SystemTime::now"];
static PYTHON_WALL_CLOCK: &[&str] = &["time.time()", "time.time"];
static JS_WALL_CLOCK: &[&str] = &["Date.now()", "Date.now"];

fn suggestion(lang: Language) -> &'static str {
    match lang {
        Language::Rust => {
            "Use `std::time::Instant::now()` for elapsed-time measurement. \
             `SystemTime` is affected by clock adjustments and not monotonic."
        }
        Language::Python => {
            "Use `time.monotonic()` for elapsed-time measurement. \
             `time.time()` can go backwards due to NTP or DST changes."
        }
        _ => {
            "Use `performance.now()` for elapsed-time measurement. \
             `Date.now()` is a wall clock and can go backwards."
        }
    }
}

fn wall_clock_patterns(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::Rust => RUST_WALL_CLOCK,
        Language::Python => PYTHON_WALL_CLOCK,
        Language::JavaScript => JS_WALL_CLOCK,
        _ => &[],
    }
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let patterns = wall_clock_patterns(lang);
    if patterns.is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment(trimmed, lang) {
            continue;
        }

        // Check if this line uses a wall-clock API
        let wall_clock_hit = patterns.iter().find(|&&p| line.contains(p));
        let Some(&clock_api) = wall_clock_hit else {
            continue;
        };

        // Check if the variable being assigned has a duration-measurement name
        let lower_line = line.to_lowercase();
        let has_duration_name = DURATION_NAMES
            .iter()
            .any(|name| lower_line.contains(name));

        if !has_duration_name {
            continue;
        }

        let line_1based = (line_idx + 1) as u32;
        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "wall-clock-misuse".into(),
            severity: Severity::Low,
            category: FindingCategory::SecuritySmell,
            file: path.to_path_buf(),
            line: Some(line_1based),
            title: "Wall clock used for duration measurement".into(),
            description: format!(
                "`{clock_api}` is a wall clock and is not suitable for measuring elapsed time. \
                 Wall clocks can jump forward or backward (NTP, DST, leap seconds), \
                 causing negative or incorrect durations."
            ),
            evidence: vec![],
            covered: false,
            suggestion: suggestion(lang).into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![682],
        });
    }

    findings
}

#[async_trait]
impl Detector for WallClockMisuseDetector {
    fn name(&self) -> &str {
        "wall-clock-misuse"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => Language::Rust,
                Some("py") => Language::Python,
                Some("js") => Language::JavaScript,
                Some("ts") | Some("tsx") => Language::JavaScript,
                _ => continue,
            };
            findings.extend(analyze_source(path, source, lang));
        }
        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn detect_rust(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/lib.rs"), source, Language::Rust)
    }

    fn detect_python(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.py"), source, Language::Python)
    }

    fn detect_js(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/app.js"), source, Language::JavaScript)
    }

    #[test]
    fn detects_system_time_now_as_start() {
        let src = "let start = SystemTime::now();\n";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
        assert_eq!(findings[0].cwe_ids, vec![682]);
    }

    #[test]
    fn detects_time_time_as_begin_python() {
        let src = "begin = time.time()\n";
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("time.time()"));
    }

    #[test]
    fn detects_date_now_as_t0_js() {
        let src = "const t0 = Date.now();\n";
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn no_finding_when_not_duration_name() {
        let src = "let current_time = SystemTime::now();\n";
        let findings = detect_rust(src);
        // "current_time" doesn't match duration names
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn no_finding_for_instant_now() {
        let src = "let start = Instant::now();\n";
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn detects_before_wall_clock() {
        let src = "let before = SystemTime::now();\ndo_work();\nlet after = SystemTime::now();\n";
        let findings = detect_rust(src);
        // "before" matches the duration name list
        assert!(!findings.is_empty());
    }
}
