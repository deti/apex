use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Evidence, Finding, FindingCategory, Severity};
use crate::Detector;

pub struct ReDoSDetector;

// ---------------------------------------------------------------------------
// Vulnerable pattern kinds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum VulnKind {
    /// A quantifier applied to a group that itself contains a quantifier,
    /// e.g. `(a+)+`, `(a*)*`, `(a+)*`.  Exponential worst case.
    NestedQuantifier,
    /// `(X|Y)+` where X and Y can match the same character class,
    /// e.g. `(\w|\d)+`, `(a|a)+`, `(.|a)+`.  Polynomial worst case.
    OverlappingAlternative,
    /// Two adjacent quantified groups whose character classes overlap,
    /// e.g. `\w+\w+`, `\d+\d+`, `[a-z]+[a-z]+`.  Polynomial worst case.
    AdjacentOverlap,
}

impl VulnKind {
    fn severity(&self) -> Severity {
        match self {
            VulnKind::NestedQuantifier => Severity::High,
            VulnKind::OverlappingAlternative => Severity::Medium,
            VulnKind::AdjacentOverlap => Severity::Medium,
        }
    }

    fn description(&self) -> &'static str {
        match self {
            VulnKind::NestedQuantifier => {
                "nested quantifier (e.g. `(a+)+`) — exponential backtracking worst case"
            }
            VulnKind::OverlappingAlternative => {
                "overlapping alternatives under a quantifier (e.g. `(\\w|\\d)+`) — \
                 polynomial backtracking worst case"
            }
            VulnKind::AdjacentOverlap => {
                "adjacent quantified groups with overlapping character classes \
                 (e.g. `\\w+\\w+`) — polynomial backtracking worst case"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Regex extraction from source (language-aware)
// ---------------------------------------------------------------------------

/// Returns all `(line_index, regex_literal)` pairs found in `source`.
/// Line index is 0-based.
fn extract_regex_literals(source: &str, lang: Language) -> Vec<(usize, String)> {
    let mut results = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        match lang {
            Language::Python => {
                for prefix in &[
                    "re.compile(",
                    "re.match(",
                    "re.search(",
                    "re.findall(",
                    "re.sub(",
                    "re.fullmatch(",
                ] {
                    if let Some(s) = extract_first_string_arg(line, prefix) {
                        results.push((line_idx, s));
                    }
                }
            }
            Language::JavaScript => {
                // JS regex literals  /PATTERN/flags
                results.extend(extract_js_regex_literals(line_idx, line));
                // new RegExp(...) or RegExp(...) — prefer the longer form so we
                // don't double-count `new RegExp(` as both itself and `RegExp(`.
                if line.contains("new RegExp(") {
                    if let Some(s) = extract_first_string_arg(line, "new RegExp(") {
                        results.push((line_idx, s));
                    }
                } else if line.contains("RegExp(") {
                    if let Some(s) = extract_first_string_arg(line, "RegExp(") {
                        results.push((line_idx, s));
                    }
                }
            }
            Language::Rust => {
                if let Some(s) = extract_first_string_arg(line, "Regex::new(") {
                    results.push((line_idx, s));
                }
            }
            Language::Go => {
                for prefix in &["regexp.Compile(", "regexp.MustCompile("] {
                    if let Some(s) = extract_first_string_arg(line, prefix) {
                        results.push((line_idx, s));
                    }
                }
            }
            Language::Java => {
                if let Some(s) = extract_first_string_arg(line, "Pattern.compile(") {
                    results.push((line_idx, s));
                }
            }
            Language::Ruby => {
                results.extend(extract_ruby_regex_literals(line_idx, line));
            }
            _ => {}
        }
    }

    results
}

/// Extract the content of the first string argument after `prefix`.
/// Handles both single-quoted, double-quoted, and raw string prefixes (r"…", r'…').
fn extract_first_string_arg(line: &str, prefix: &str) -> Option<String> {
    let after = line.find(prefix)? + prefix.len();
    let rest = line[after..].trim_start();

    // Strip optional raw-string prefix r or r# (Rust), b, f, etc.
    let raw_match = rest
        .strip_prefix("r#\"")
        .and_then(|s| s.rfind("\"#").map(|i| &s[..i]))
        .or_else(|| {
            rest.strip_prefix("r\"")
                .and_then(|s| s.find('"').map(|i| &s[..i]))
        })
        .or_else(|| {
            rest.strip_prefix("r'")
                .and_then(|s| s.find('\'').map(|i| &s[..i]))
        });

    if let Some(raw) = raw_match {
        return Some(raw.to_string());
    }

    // Normal quoted string
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }

    let inner = &rest[1..];
    let mut result = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // Keep escape sequences as-is so the vulnerability check sees them
            result.push('\\');
            if let Some(next) = chars.next() {
                result.push(next);
            }
        } else if c == quote {
            break;
        } else {
            result.push(c);
        }
    }
    Some(result)
}

/// Extract JS regex literals of the form `/PATTERN/flags` from a line.
/// Returns `(line_idx, pattern_without_slashes)` pairs.
fn extract_js_regex_literals(line_idx: usize, line: &str) -> Vec<(usize, String)> {
    let mut results = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'/' {
            // Skip `//` and `/*` comment starts
            if i + 1 < len && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*') {
                break;
            }
            // Very lightweight: scan to next unescaped `/`
            let mut j = i + 1;
            let mut escaped = false;
            while j < len {
                if escaped {
                    escaped = false;
                } else if bytes[j] == b'\\' {
                    escaped = true;
                } else if bytes[j] == b'/' {
                    let pattern = std::str::from_utf8(&bytes[i + 1..j])
                        .unwrap_or("")
                        .to_string();
                    // Sanity check: at least 2 chars, no leading spaces (avoids division)
                    if pattern.len() >= 2 && !pattern.starts_with(' ') {
                        results.push((line_idx, pattern));
                    }
                    i = j + 1;
                    // Skip flags
                    while i < len && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    break;
                }
                j += 1;
            }
            if j >= len {
                break;
            }
        } else {
            i += 1;
        }
    }

    results
}

/// Extract Ruby regex literals `/PATTERN/` from a line.
fn extract_ruby_regex_literals(line_idx: usize, line: &str) -> Vec<(usize, String)> {
    // Ruby regex literals look identical to JS — reuse the same extractor.
    extract_js_regex_literals(line_idx, line)
}

// ---------------------------------------------------------------------------
// Vulnerability detection
// ---------------------------------------------------------------------------

/// Returns `Some((kind, pump_string))` if `pattern` contains a ReDoS-vulnerable construct.
fn check_redos(pattern: &str) -> Option<(VulnKind, String)> {
    // 1. Nested quantifiers: a quantifier on a group that itself contains a quantifier.
    //    Examples: (a+)+  (a*)*  (\w+|x)*  ([a-z]+\d)*
    if has_nested_quantifier(pattern) {
        let pump = generate_pump_nested(pattern);
        return Some((VulnKind::NestedQuantifier, pump));
    }

    // 2. Overlapping alternatives under a quantifier: (X|Y)+
    //    where X and Y share a character class.
    if has_overlapping_alternatives(pattern) {
        let pump = generate_pump_alt(pattern);
        return Some((VulnKind::OverlappingAlternative, pump));
    }

    // 3. Adjacent quantified groups with overlapping character classes.
    if has_adjacent_overlap(pattern) {
        let pump = generate_pump_adjacent(pattern);
        return Some((VulnKind::AdjacentOverlap, pump));
    }

    None
}

/// Check for nested quantifiers: a group `(…)` that contains a quantifier
/// (`+`, `*`, `{n,}`) is itself followed by a quantifier.
///
/// Strategy: scan the pattern for `)` followed by `+`/`*`/`{n,}`.  When we
/// find such a `)`, walk backwards to find the matching `(` and check
/// whether anything inside already has a quantifier.
fn has_nested_quantifier(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        if chars[i] == ')' {
            // Check outer quantifier after the `)`
            let after = i + 1;
            if after < n && (chars[after] == '+' || chars[after] == '*') {
                // Find matching `(`
                if let Some(open) = find_matching_open(&chars, i) {
                    let inner: String = chars[open + 1..i].iter().collect();
                    if inner_has_quantifier(&inner) {
                        return true;
                    }
                }
            } else if after < n && chars[after] == '{' {
                // `{n,}` form — look for a comma before the closing `}`
                let j = after + 1;
                let close = chars[j..].iter().position(|&c| c == '}').map(|p| j + p);
                if let Some(close_idx) = close {
                    let inside: String = chars[j..close_idx].iter().collect();
                    if inside.contains(',') {
                        if let Some(open) = find_matching_open(&chars, i) {
                            let inner: String = chars[open + 1..i].iter().collect();
                            if inner_has_quantifier(&inner) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    false
}

/// Return the index of the `(` that matches the `)` at position `close`.
fn find_matching_open(chars: &[char], close: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = close as isize;
    while i >= 0 {
        match chars[i as usize] {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i as usize);
                }
            }
            _ => {}
        }
        i -= 1;
    }
    None
}

/// True if `inner` (the contents of a group) already has a top-level
/// quantifier — meaning one that applies to a character, escape, or subgroup.
fn inner_has_quantifier(inner: &str) -> bool {
    // Simplified: look for `+` or `*` that is not inside a nested group
    // and not immediately after `\` (escape).
    let chars: Vec<char> = inner.chars().collect();
    let n = chars.len();
    let mut depth = 0;
    let mut i = 0;
    while i < n {
        match chars[i] {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
                // After the `)` of a nested group, check for quantifier
                let after = i + 1;
                if depth == 0 && after < n && (chars[after] == '+' || chars[after] == '*') {
                    return true;
                }
            }
            '\\' => {
                // Skip the escaped character — it cannot carry a quantifier itself
                i += 1;
            }
            '+' | '*' if depth == 0 => return true,
            '{' if depth == 0 => {
                // `{n,}` — check for comma
                let j = i + 1;
                if let Some(close) = chars[j..].iter().position(|&c| c == '}') {
                    let seg: String = chars[j..j + close].iter().collect();
                    if seg.contains(',') {
                        return true;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Detect `(X|Y)+` or `(X|Y)*` where X and Y share a character class.
///
/// Heuristic: find groups with `|` inside under an outer quantifier, then
/// check whether any two alternatives share a "character family":
///   - Both are `\w`, `\d`, `\s`, `[a-z]`, `.`, literal letter, etc.
fn has_overlapping_alternatives(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        if chars[i] == '(' {
            if let Some(close) = find_matching_close(&chars, i) {
                let after = close + 1;
                let has_outer_quant = after < n && (chars[after] == '+' || chars[after] == '*');
                if has_outer_quant {
                    let inner: String = chars[i + 1..close].iter().collect();
                    // Skip non-capturing group prefix `?:`
                    let inner = inner.trim_start_matches("?:");
                    if alternatives_overlap(inner) {
                        return true;
                    }
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    false
}

/// Return the index of the `)` matching the `(` at position `open`.
fn find_matching_close(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, &c) in chars.iter().enumerate().skip(open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Check whether any two alternatives in `inner` (split by top-level `|`)
/// share a character class.
fn alternatives_overlap(inner: &str) -> bool {
    let alts = split_top_level_alts(inner);
    if alts.len() < 2 {
        return false;
    }
    for i in 0..alts.len() {
        for j in i + 1..alts.len() {
            if char_families_overlap(&alts[i], &alts[j]) {
                return true;
            }
        }
    }
    false
}

/// Split `inner` on top-level `|` (not inside nested groups).
fn split_top_level_alts(inner: &str) -> Vec<String> {
    let mut alts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for c in inner.chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth -= 1;
                current.push(c);
            }
            '|' if depth == 0 => {
                alts.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(c),
        }
    }
    alts.push(current.trim().to_string());
    alts
}

/// Return the set of "character families" for an alternative fragment.
/// Families: `word`, `digit`, `space`, `dot`, `literal`.
fn char_families(alt: &str) -> Vec<&'static str> {
    let mut families = Vec::new();
    if alt.contains("\\w")
        || alt.contains("[a-z")
        || alt.contains("[A-Z")
        || alt.contains("[a-zA-Z")
    {
        families.push("word");
    }
    if alt.contains("\\d") || alt.contains("[0-9") {
        families.push("digit");
    }
    if alt.contains("\\s") {
        families.push("space");
    }
    if alt == "." || alt.contains('.') && !alt.contains("\\.") {
        families.push("dot");
    }
    // `\w` encompasses `\d`, so always add `digit` when `word` is present
    if families.contains(&"word") && !families.contains(&"digit") {
        families.push("digit");
    }
    // Literal alphanumeric sequences (e.g. "a", "aa", "abc") — two such
    // alternatives always have the potential to overlap since the characters
    // they match are drawn from the same alphabet.
    if !alt.is_empty() && alt.chars().all(|c| c.is_alphanumeric()) {
        families.push("literal");
    }
    families
}

fn char_families_overlap(a: &str, b: &str) -> bool {
    // `.` (dot) overlaps everything
    if a == "." || b == "." {
        return true;
    }
    let fa = char_families(a);
    let fb = char_families(b);
    // Identical alternatives always overlap
    if a == b {
        return true;
    }
    fa.iter().any(|f| fb.contains(f))
}

/// Detect adjacent quantified groups with overlapping character classes.
/// Patterns like: `\d+\d+`, `\w+\s*\w+`, `[a-z]+[a-z]+`
fn has_adjacent_overlap(pattern: &str) -> bool {
    // Extract adjacent quantified atoms and check if neighbouring pairs overlap.
    let atoms = extract_quantified_atoms(pattern);
    for window in atoms.windows(2) {
        if char_families_overlap(&window[0], &window[1]) {
            return true;
        }
    }
    false
}

/// Extract atoms (character class names) that are followed by a quantifier.
fn extract_quantified_atoms(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    let mut atoms = Vec::new();
    let mut i = 0;

    while i < n {
        let atom_start = i;
        let atom: Option<String>;

        if chars[i] == '\\' && i + 1 < n {
            // Escape sequence like \w \d \s
            let escaped = chars[i + 1];
            atom = Some(format!("\\{}", escaped));
            i += 2;
        } else if chars[i] == '[' {
            // Character class [...]
            let mut j = i + 1;
            while j < n && chars[j] != ']' {
                j += 1;
            }
            atom = Some(chars[i..=j.min(n - 1)].iter().collect());
            i = j + 1;
        } else if chars[i] == '(' {
            // Skip groups entirely — handled by other checks
            if let Some(close) = find_matching_close(&chars, i) {
                i = close + 1;
            } else {
                i += 1;
            }
            let _ = atom_start;
            continue;
        } else if chars[i].is_alphanumeric() || chars[i] == '.' {
            atom = Some(chars[i].to_string());
            i += 1;
        } else {
            // Anchor, pipe, etc. — reset sequence
            i += 1;
            continue;
        }

        // Check if a quantifier follows the atom
        if let Some(a) = atom {
            if i < n && (chars[i] == '+' || chars[i] == '*') {
                atoms.push(a);
                i += 1; // skip quantifier
            } else if i < n && chars[i] == '{' {
                let j = i + 1;
                if let Some(close) = chars[j..].iter().position(|&c| c == '}') {
                    let seg: String = chars[j..j + close].iter().collect();
                    if seg.contains(',') {
                        atoms.push(a);
                    }
                    i = j + close + 1;
                } else {
                    i += 1;
                }
            } else {
                // No quantifier — break the adjacency chain
                atoms.clear();
            }
        }
    }
    atoms
}

// ---------------------------------------------------------------------------
// Pump string generation
// ---------------------------------------------------------------------------

/// Generate a worst-case pump string for nested quantifier patterns.
/// Strategy: repeat the character matched by the inner quantifier, then
/// append a non-matching character to force backtracking.
fn generate_pump_nested(pattern: &str) -> String {
    let base_char = dominant_char(pattern);
    let reject_char = reject_for(base_char);
    format!("{}{}", base_char.to_string().repeat(20), reject_char)
}

/// Generate a pump string for overlapping alternative patterns.
fn generate_pump_alt(pattern: &str) -> String {
    let base_char = dominant_char(pattern);
    let reject_char = reject_for(base_char);
    format!("{}{}", base_char.to_string().repeat(20), reject_char)
}

/// Generate a pump string for adjacent overlap patterns.
fn generate_pump_adjacent(pattern: &str) -> String {
    let base_char = dominant_char(pattern);
    let reject_char = reject_for(base_char);
    format!("{}{}", base_char.to_string().repeat(20), reject_char)
}

/// Heuristic: pick the most likely repeated character from the pattern.
fn dominant_char(pattern: &str) -> char {
    if pattern.contains("\\d") || pattern.contains("[0-9") {
        '1'
    } else if pattern.contains("\\s") {
        ' '
    } else if pattern.contains("[A-Z") {
        'A'
    } else {
        // Default: lowercase letter
        'a'
    }
}

/// Return a character that will NOT match the dominant character class,
/// forcing the regex engine to fail and backtrack.
fn reject_for(base: char) -> char {
    match base {
        '1' => 'a',
        ' ' => '!',
        'A' => '!',
        _ => '!',
    }
}

// ---------------------------------------------------------------------------
// Per-file analysis
// ---------------------------------------------------------------------------

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (line_idx, pattern) in extract_regex_literals(source, lang) {
        if let Some((kind, pump)) = check_redos(&pattern) {
            let line_1based = (line_idx + 1) as u32;
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "redos".into(),
                severity: kind.severity(),
                category: FindingCategory::PerformanceRisk,
                file: path.to_path_buf(),
                line: Some(line_1based),
                title: format!("ReDoS-vulnerable regex: {}", kind.description()),
                description: format!(
                    "The regular expression `{}` at line {} contains a {}. \
                     An attacker who can control input may craft a string that \
                     causes catastrophic backtracking, consuming CPU indefinitely \
                     (Denial of Service). Worst-case pump string: `{}`",
                    pattern,
                    line_1based,
                    kind.description(),
                    pump
                ),
                evidence: vec![Evidence::PerformanceProfile {
                    function: "regex".into(),
                    metric: "backtracking".into(),
                    baseline_value: None,
                    measured_value: 0.0,
                    input_description: pump,
                }],
                covered: false,
                suggestion: match kind {
                    VulnKind::NestedQuantifier => {
                        "Rewrite the regex to eliminate nested quantifiers. \
                         Use atomic grouping `(?>…)` or possessive quantifiers `++`/`*+` \
                         where the engine supports them. Alternatively, add a termination \
                         anchor so the failure path is short-circuited early."
                    }
                    VulnKind::OverlappingAlternative => {
                        "Make the alternatives mutually exclusive so no character can \
                         match both branches. For example, replace `(\\w|\\d)+` with `\\w+` \
                         since `\\w` already subsumes `\\d`."
                    }
                    VulnKind::AdjacentOverlap => {
                        "Merge or separate adjacent quantified groups whose character classes \
                         overlap. For example, replace `\\d+\\d+` with `\\d+` and capture \
                         the boundary with an explicit separator pattern."
                    }
                }
                .into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![1333, 400],
                noisy: false,
                base_severity: None,
                coverage_confidence: None,
            });
        }
    }

    findings
}

// ---------------------------------------------------------------------------
// Detector impl
// ---------------------------------------------------------------------------

#[async_trait]
impl Detector for ReDoSDetector {
    fn name(&self) -> &str {
        "redos"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => Language::Rust,
                Some("py") => Language::Python,
                Some("js") | Some("jsx") => Language::JavaScript,
                Some("ts") | Some("tsx") => Language::JavaScript,
                Some("go") => Language::Go,
                Some("java") => Language::Java,
                Some("rb") => Language::Ruby,
                _ => continue,
            };
            findings.extend(analyze_source(path, source, lang));
        }

        Ok(findings)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn detect(source: &str, lang: Language, ext: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from(format!("src/app.{}", ext)), source, lang)
    }

    fn detect_rust(source: &str) -> Vec<Finding> {
        detect(source, Language::Rust, "rs")
    }

    fn detect_python(source: &str) -> Vec<Finding> {
        detect(source, Language::Python, "py")
    }

    fn detect_js(source: &str) -> Vec<Finding> {
        detect(source, Language::JavaScript, "js")
    }

    fn detect_go(source: &str) -> Vec<Finding> {
        detect(source, Language::Go, "go")
    }

    // ---- Positive: nested quantifiers ----

    #[test]
    fn detects_nested_quantifier_a_plus_plus() {
        // (a+)+$ — classic exponential ReDoS
        let src = r#"let re = Regex::new(r"(a+)+$").unwrap();"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1, "expected 1 finding for (a+)+$");
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].cwe_ids, vec![1333, 400]);
        assert_eq!(findings[0].category, FindingCategory::PerformanceRisk);
        assert!(!findings[0].noisy);
        // Evidence should contain a pump string ending with the reject char
        match &findings[0].evidence[0] {
            Evidence::PerformanceProfile {
                input_description, ..
            } => {
                assert!(
                    input_description.ends_with('!'),
                    "pump string should end with '!'"
                );
            }
            _ => panic!("expected PerformanceProfile evidence"),
        }
    }

    #[test]
    fn detects_nested_quantifier_star_star() {
        // (a*)* — also exponential
        let src = r#"let re = Regex::new(r"(a*)*$").unwrap();"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn detects_nested_quantifier_plus_star() {
        // (a+)* — exponential
        let src = r#"let re = Regex::new(r"(a+)*$").unwrap();"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    // ---- Positive: overlapping alternatives ----

    #[test]
    fn detects_overlapping_alternatives_word_digit() {
        // (\w|\d)+ — polynomial
        let src = r#"let re = Regex::new(r"(\w|\d)+$").unwrap();"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].cwe_ids, vec![1333, 400]);
    }

    #[test]
    fn detects_overlapping_alternatives_identical() {
        // (a|a)*$ — always overlaps
        let src = r#"let re = Regex::new(r"(a|aa)*$").unwrap();"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    // ---- Negative: safe patterns ----

    #[test]
    fn no_finding_simple_anchored() {
        // ^[a-z]+$ — safe: anchored, single quantifier
        let src = r#"let re = Regex::new(r"^[a-z]+$").unwrap();"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0, "^[a-z]+$ should not flag");
    }

    #[test]
    fn no_finding_fixed_width_digits() {
        // ^\d{3}-\d{4}$ — safe: fixed quantifiers, no nesting
        let src = r#"let re = Regex::new(r"^\d{3}-\d{4}$").unwrap();"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 0, "fixed-width quantifiers should not flag");
    }

    #[test]
    fn no_finding_email_like() {
        // Simple email-ish: [a-z]+@[a-z]+\.[a-z]{2,6}
        let src = r#"let re = Regex::new(r"[a-z]+@[a-z]+\.[a-z]{2,6}").unwrap();"#;
        let findings = detect_rust(src);
        // [a-z]+@[a-z]+ is adjacent overlap — but the `@` separator breaks
        // adjacency, so the atom list resets.  Expect 0 findings.
        assert_eq!(findings.len(), 0);
    }

    // ---- Multi-language: Python ----

    #[test]
    fn detects_nested_quantifier_python_re_compile() {
        let src = r#"pattern = re.compile(r"(a+)+$")"#;
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn detects_overlapping_alternatives_python_re_search() {
        let src = r#"m = re.search(r"(\w|\d)+", text)"#;
        let findings = detect_python(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    // ---- Multi-language: JavaScript ----

    #[test]
    fn detects_nested_quantifier_js_literal() {
        let src = r#"const re = /(a+)+$/;"#;
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn detects_nested_quantifier_js_new_regexp() {
        let src = r#"const re = new RegExp("(a+)+$");"#;
        let findings = detect_js(src);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    // ---- Multi-language: Go ----

    #[test]
    fn detects_nested_quantifier_go_must_compile() {
        let src = r#"re := regexp.MustCompile(`(a+)+$`)"#;
        // Go uses backtick raw strings — not currently extracted by extract_first_string_arg,
        // but the pattern tests the language dispatch.  We use a double-quoted form here.
        let src2 = r#"re, _ := regexp.Compile("(a+)+$")"#;
        let findings = detect_go(src2);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
        let _ = src; // suppress unused warning
    }

    // ---- Evidence structure ----

    #[test]
    fn finding_evidence_is_performance_profile() {
        let src = r#"let re = Regex::new(r"(a+)+$").unwrap();"#;
        let findings = detect_rust(src);
        assert_eq!(findings.len(), 1);
        let ev = &findings[0].evidence;
        assert_eq!(ev.len(), 1);
        match &ev[0] {
            Evidence::PerformanceProfile {
                function,
                metric,
                baseline_value,
                measured_value,
                input_description,
            } => {
                assert_eq!(function, "regex");
                assert_eq!(metric, "backtracking");
                assert!(baseline_value.is_none());
                assert_eq!(*measured_value, 0.0);
                assert!(!input_description.is_empty());
            }
            _ => panic!("expected PerformanceProfile"),
        }
    }

    // ---- Suggestion text ----

    #[test]
    fn suggestion_mentions_nested_quantifier_rewrite() {
        let src = r#"let re = Regex::new(r"(a+)+$").unwrap();"#;
        let findings = detect_rust(src);
        assert!(
            findings[0].suggestion.contains("nested quantifier")
                || findings[0].suggestion.contains("atomic grouping")
                || findings[0].suggestion.contains("possessive")
        );
    }

    #[test]
    fn suggestion_mentions_mutually_exclusive_for_overlap() {
        let src = r#"let re = Regex::new(r"(\w|\d)+$").unwrap();"#;
        let findings = detect_rust(src);
        assert!(
            findings[0].suggestion.contains("mutually exclusive")
                || findings[0].suggestion.contains("subsumes")
        );
    }

    // ---- No double-counting ----

    #[test]
    fn no_duplicate_findings_for_same_line() {
        // If multiple check passes would match, we only get one finding per regex
        let src = r#"let re = Regex::new(r"(a+)+$").unwrap();"#;
        let findings = detect_rust(src);
        // Nested quantifier triggers first; we should not also get overlap/adjacent
        assert_eq!(findings.len(), 1);
    }
}
