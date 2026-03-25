//! Hash Collision Risk Detector
//!
//! Identifies hash table operations with user-controlled keys that could enable
//! hash collision DoS attacks (CWE-400). Based on Crosby & Wallach 2003.
//!
//! An attacker who controls hash-map keys can craft inputs that all land in the
//! same bucket, degrading O(1) lookups to O(n) and exhausting CPU under load.
//! This is only a real risk when:
//!   1. The hasher is deterministic / non-randomised (so the attacker can predict
//!      which inputs collide), AND
//!   2. The keys originate from untrusted user input.
//!
//! Language notes:
//!   * Python  — `dict` uses SipHash-1-3 since 3.6 (PYTHONHASHSEED randomised by
//!     default), so plain dict usage is *not* flagged. Only patterns that
//!     explicitly bypass randomisation would be, though those are rare enough
//!     that we keep the detector focused on the request-key pattern.
//!   * JavaScript — V8's object property hashing is deterministic; `Map` uses
//!     identity / value-based keys without randomisation for strings.
//!   * Java — `HashMap` (pre-Java-8 *and* very long chains) is vulnerable;
//!     Java 8+ converts to balanced trees at depth 8, but ConcurrentHashMap and
//!     TreeMap are explicitly safe.
//!   * Rust — `HashMap` uses `RandomState` (SipHash-1-3) by default and is safe.
//!     We only flag when a non-randomised hasher like `FxHasher`,
//!     `BuildHasherDefault`, or `AHasher` is used with user-controlled keys,
//!     because those provide no DoS protection.

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct HashCollisionRiskDetector;

// ---------------------------------------------------------------------------
// Per-language patterns
// ---------------------------------------------------------------------------

/// Python: dict key access/membership with user input as the key.
/// e.g. `data[request.args["k"]]` or `if x in my_dict`.
static PYTHON_HASH_PATTERNS: &[&str] = &["[", " in "];

/// Python user-input source indicators.
static PYTHON_USER_INPUT: &[&str] = &[
    "request.", "args.", "params.", "form.", "json.", "query.", "headers.", "POST[", "GET[",
];

/// Python safe key types that indicate the key is not user-controlled.
static PYTHON_SAFE_PATTERNS: &[&str] = &["OrderedDict"];

// ---------------------------------------------------------------------------

/// JavaScript: object bracket access or Map.set with user input as key.
static JS_HASH_PATTERNS: &[&str] = &["[", "Map.set(", ".set("];

/// JavaScript user-input source indicators.
static JS_USER_INPUT: &[&str] = &[
    "req.body.",
    "req.query.",
    "req.params.",
    "req.headers.",
    "request.body",
    "request.query",
    "request.params",
    "request.headers",
];

// ---------------------------------------------------------------------------

/// Java: HashMap.put / HashMap.get with user input as key.
static JAVA_HASH_PATTERNS: &[&str] = &[".put(", ".get(", ".containsKey("];

/// Java user-input source indicators.
static JAVA_USER_INPUT: &[&str] = &["getParameter(", "getHeader(", "request.get"];

/// Java safe map types (collision-resistant or ordered).
/// ConcurrentHashMap uses tree bins in Java 8+; TreeMap uses comparisons.
static JAVA_SAFE_PATTERNS: &[&str] = &["TreeMap", "ConcurrentHashMap", "LinkedHashMap"];

// ---------------------------------------------------------------------------

/// Rust: HashMap::insert / HashMap::get with a non-randomised hasher AND user
/// input. The default `RandomState` is safe, so we only flag custom hashers.
static RUST_HASH_PATTERNS: &[&str] = &[".insert(", ".get(", ".contains_key("];

/// Rust non-randomised hasher indicators.
static RUST_UNSAFE_HASHERS: &[&str] = &[
    "FxHasher",
    "FxHashMap",
    "FxHashSet",
    "BuildHasherDefault",
    "AHasher",
    "fnv::",
    "FnvHashMap",
    "FnvHashSet",
];

/// Rust user-input indicators (typically actix-web / axum extractors).
static RUST_USER_INPUT: &[&str] = &[
    "Query<",
    "Path<",
    "Form<",
    "Json<",
    "req.body",
    "request.body",
    ".param(",
    ".query_string(",
];

// ---------------------------------------------------------------------------
// Core analysis
// ---------------------------------------------------------------------------

/// Returns true when `line` contains any of the provided needle strings.
fn line_contains_any(line: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| line.contains(n))
}

/// Scan a window of `context_lines` lines around `center` (inclusive) for any
/// of the given indicators. The window is clamped to [0, lines.len()).
fn window_contains_any(
    lines: &[&str],
    center: usize,
    context_lines: usize,
    needles: &[&str],
) -> bool {
    let start = center.saturating_sub(context_lines);
    let end = (center + context_lines + 1).min(lines.len());
    lines[start..end]
        .iter()
        .any(|l| line_contains_any(l, needles))
}

/// Check whether a line (or nearby context) indicates a safe/suppressed pattern.
fn is_suppressed(line: &str, safe_patterns: &[&str]) -> bool {
    line_contains_any(line, safe_patterns)
}

fn analyze_python(path: &std::path::Path, source: &str) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // The line must perform a dict operation (key lookup or membership test).
        if !line_contains_any(line, PYTHON_HASH_PATTERNS) {
            continue;
        }

        // Check if user input appears on the same line or within ±3 lines.
        if !window_contains_any(&lines, idx, 3, PYTHON_USER_INPUT) {
            continue;
        }

        // Suppress known-safe patterns.
        if is_suppressed(line, PYTHON_SAFE_PATTERNS) {
            continue;
        }

        let line_1based = (idx + 1) as u32;
        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "hash-collision-risk".into(),
            severity: Severity::Medium,
            category: FindingCategory::PerformanceRisk,
            file: path.to_path_buf(),
            line: Some(line_1based),
            title: "User-controlled input used as hash map key — potential hash collision DoS"
                .into(),
            description: format!(
                "Line {}: A value derived from user input is used as a dictionary key. \
                 An attacker can craft inputs that all hash to the same bucket, \
                 degrading O(1) lookups to O(n) and exhausting CPU (CWE-400). \
                 Reference: Crosby & Wallach, USENIX Security 2003.",
                line_1based
            ),
            evidence: vec![],
            covered: false,
            suggestion: "Consider applying a size limit on the dictionary, rate-limiting \
                         per-request key insertions, or hashing keys with HMAC before \
                         insertion to prevent adversarial preimage selection."
                .into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![400],
            noisy: false,
            base_severity: None,
            coverage_confidence: None,
        });
    }

    findings
}

fn analyze_javascript(path: &std::path::Path, source: &str) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("*") {
            continue;
        }

        if !line_contains_any(line, JS_HASH_PATTERNS) {
            continue;
        }

        if !window_contains_any(&lines, idx, 3, JS_USER_INPUT) {
            continue;
        }

        let line_1based = (idx + 1) as u32;
        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "hash-collision-risk".into(),
            severity: Severity::Medium,
            category: FindingCategory::PerformanceRisk,
            file: path.to_path_buf(),
            line: Some(line_1based),
            title: "User-controlled input used as hash map key — potential hash collision DoS"
                .into(),
            description: format!(
                "Line {}: A value derived from user input is used as an object property key \
                 or Map key. V8's object hashing is deterministic for string keys; an attacker \
                 can craft inputs that collide, causing O(n) lookup degradation (CWE-400). \
                 Reference: Crosby & Wallach, USENIX Security 2003.",
                line_1based
            ),
            evidence: vec![],
            covered: false,
            suggestion: "Use a Map with a size cap, validate and canonicalise keys before \
                         insertion, or apply rate limiting per request to bound the number of \
                         unique keys inserted per map instance."
                .into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![400],
            noisy: false,
            base_severity: None,
            coverage_confidence: None,
        });
    }

    findings
}

fn analyze_java(path: &std::path::Path, source: &str) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('*') {
            continue;
        }

        if !line_contains_any(line, JAVA_HASH_PATTERNS) {
            continue;
        }

        // Suppress safe map types declared anywhere in the visible context.
        if is_suppressed(line, JAVA_SAFE_PATTERNS) {
            continue;
        }
        if window_contains_any(&lines, idx, 5, JAVA_SAFE_PATTERNS) {
            continue;
        }

        if !window_contains_any(&lines, idx, 3, JAVA_USER_INPUT) {
            continue;
        }

        let line_1based = (idx + 1) as u32;
        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "hash-collision-risk".into(),
            severity: Severity::Medium,
            category: FindingCategory::PerformanceRisk,
            file: path.to_path_buf(),
            line: Some(line_1based),
            title: "User-controlled input used as hash map key — potential hash collision DoS"
                .into(),
            description: format!(
                "Line {}: A value from user input is used as a HashMap key. \
                 Although Java 8+ converts long collision chains to balanced trees, \
                 the tree conversion has its own overhead and does not fully mitigate \
                 a targeted DoS. Use TreeMap or ConcurrentHashMap, or limit key cardinality \
                 (CWE-400). Reference: Crosby & Wallach, USENIX Security 2003.",
                line_1based
            ),
            evidence: vec![],
            covered: false,
            suggestion: "Replace HashMap with TreeMap for untrusted keys (O(log n) worst-case \
                         comparisons instead of hash-bucket chains), or use ConcurrentHashMap \
                         which applies Java 8+ tree-bin mitigation. Also consider rate-limiting \
                         the number of distinct keys per request."
                .into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![400],
            noisy: false,
            base_severity: None,
            coverage_confidence: None,
        });
    }

    findings
}

fn analyze_rust(path: &std::path::Path, source: &str) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();

    // Rust's default HashMap uses RandomState (SipHash-1-3) which is collision-
    // resistant by design. We only flag when the file also uses a known
    // non-randomised hasher — check the whole source for hasher imports/usage.
    let uses_unsafe_hasher = lines
        .iter()
        .any(|l| line_contains_any(l, RUST_UNSAFE_HASHERS));

    if !uses_unsafe_hasher {
        return findings;
    }

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        if !line_contains_any(line, RUST_HASH_PATTERNS) {
            continue;
        }

        if !window_contains_any(&lines, idx, 3, RUST_USER_INPUT) {
            continue;
        }

        let line_1based = (idx + 1) as u32;
        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "hash-collision-risk".into(),
            severity: Severity::Medium,
            category: FindingCategory::PerformanceRisk,
            file: path.to_path_buf(),
            line: Some(line_1based),
            title: "User-controlled input used as hash map key — potential hash collision DoS"
                .into(),
            description: format!(
                "Line {}: A non-randomised hasher (FxHasher / AHasher / FNV) is used and \
                 user-controlled input is inserted as a key. Unlike Rust's default SipHash-1-3 \
                 RandomState, these hashers are deterministic and allow an attacker to craft \
                 colliding keys that degrade O(1) lookups to O(n) (CWE-400). \
                 Reference: Crosby & Wallach, USENIX Security 2003.",
                line_1based
            ),
            evidence: vec![],
            covered: false,
            suggestion: "For user-controlled keys, use the default `HashMap` (RandomState / \
                         SipHash-1-3) instead of FxHasher or other non-randomised hashers. \
                         Non-randomised hashers are only safe when all keys are trusted and \
                         known at compile time."
                .into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![400],
            noisy: false,
            base_severity: None,
            coverage_confidence: None,
        });
    }

    findings
}

fn analyze_source(path: &std::path::Path, source: &str, lang: Language) -> Vec<Finding> {
    match lang {
        Language::Python => analyze_python(path, source),
        Language::JavaScript => analyze_javascript(path, source),
        Language::Java => analyze_java(path, source),
        Language::Rust => analyze_rust(path, source),
        _ => Vec::new(),
    }
}

#[async_trait]
impl Detector for HashCollisionRiskDetector {
    fn name(&self) -> &str {
        "hash-collision-risk"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            let lang = match path.extension().and_then(|e| e.to_str()) {
                Some("py") => Language::Python,
                Some("js") | Some("jsx") | Some("ts") | Some("tsx") => Language::JavaScript,
                Some("java") => Language::Java,
                Some("rs") => Language::Rust,
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

    fn py(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("app/views.py"), source, Language::Python)
    }

    fn js(source: &str) -> Vec<Finding> {
        analyze_source(
            &PathBuf::from("src/handler.js"),
            source,
            Language::JavaScript,
        )
    }

    fn java(source: &str) -> Vec<Finding> {
        analyze_source(
            &PathBuf::from("src/Controller.java"),
            source,
            Language::Java,
        )
    }

    fn rust_src(source: &str) -> Vec<Finding> {
        analyze_source(&PathBuf::from("src/handler.rs"), source, Language::Rust)
    }

    // -------------------------------------------------------------------------
    // 1. Python Flask handler: request.args used as dict key → finding
    // -------------------------------------------------------------------------

    #[test]
    fn python_flask_request_args_as_dict_key_is_flagged() {
        let src = r#"
from flask import Flask, request
app = Flask(__name__)

@app.route('/search')
def search():
    cache = {}
    key = request.args.get('q')
    cache[key] = expensive_lookup(key)
    return cache[key]
"#;
        let findings = py(src);
        assert!(
            !findings.is_empty(),
            "expected a finding for request.args as dict key"
        );
        assert_eq!(findings[0].cwe_ids, vec![400]);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[0].category, FindingCategory::PerformanceRisk);
        assert!(!findings[0].noisy);
    }

    // -------------------------------------------------------------------------
    // 2. Python internal dict (no user input) → no finding
    // -------------------------------------------------------------------------

    #[test]
    fn python_internal_dict_no_user_input_is_clean() {
        let src = r#"
def build_index(items):
    index = {}
    for item in items:
        index[item.id] = item
    return index
"#;
        let findings = py(src);
        assert_eq!(
            findings.len(),
            0,
            "internal dict with no user input should not be flagged"
        );
    }

    // -------------------------------------------------------------------------
    // 3. JavaScript Express handler: req.body as object key → finding
    // -------------------------------------------------------------------------

    #[test]
    fn js_express_req_body_as_object_key_is_flagged() {
        let src = r#"
const express = require('express');
const app = express();

app.post('/store', (req, res) => {
    const store = {};
    const key = req.body.name;
    store[key] = req.body.value;
    res.json({ ok: true });
});
"#;
        let findings = js(src);
        assert!(
            !findings.is_empty(),
            "expected a finding for req.body as object key"
        );
        assert_eq!(findings[0].cwe_ids, vec![400]);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    // -------------------------------------------------------------------------
    // 4. JavaScript with no user input → no finding
    // -------------------------------------------------------------------------

    #[test]
    fn js_object_with_internal_keys_is_clean() {
        let src = r#"
function buildConfig(settings) {
    const config = {};
    settings.forEach(s => {
        config[s.name] = s.value;
    });
    return config;
}
"#;
        let findings = js(src);
        assert_eq!(
            findings.len(),
            0,
            "no req.body/req.query means no user-input risk"
        );
    }

    // -------------------------------------------------------------------------
    // 5. Java HashMap with request.getParameter() as key → finding
    // -------------------------------------------------------------------------

    #[test]
    fn java_hashmap_get_parameter_as_key_is_flagged() {
        let src = r#"
import java.util.HashMap;
import javax.servlet.http.HttpServletRequest;

public class SearchController {
    private HashMap<String, Object> cache = new HashMap<>();

    public void handleRequest(HttpServletRequest request) {
        String key = request.getParameter("query");
        cache.put(key, performSearch(key));
    }
}
"#;
        let findings = java(src);
        assert!(
            !findings.is_empty(),
            "expected a finding for getParameter used as HashMap key"
        );
        assert_eq!(findings[0].cwe_ids, vec![400]);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    // -------------------------------------------------------------------------
    // 6. Java TreeMap (safe) with user input → no finding
    // -------------------------------------------------------------------------

    #[test]
    fn java_treemap_with_user_input_is_clean() {
        let src = r#"
import java.util.TreeMap;
import javax.servlet.http.HttpServletRequest;

public class SafeController {
    private TreeMap<String, Object> cache = new TreeMap<>();

    public void handleRequest(HttpServletRequest request) {
        String key = request.getParameter("query");
        cache.put(key, performSearch(key));
    }
}
"#;
        let findings = java(src);
        assert_eq!(findings.len(), 0, "TreeMap is safe from hash collision DoS");
    }

    // -------------------------------------------------------------------------
    // 7. Rust HashMap with default RandomState hasher → no finding
    // -------------------------------------------------------------------------

    #[test]
    fn rust_default_hashmap_with_user_input_is_clean() {
        let src = r#"
use std::collections::HashMap;

async fn handler(query: Query<Params>) -> impl Responder {
    let mut map: HashMap<String, String> = HashMap::new();
    map.insert(query.key.clone(), query.value.clone());
    HttpResponse::Ok().json(map)
}
"#;
        // Default HashMap uses SipHash-1-3 (RandomState) — should not be flagged.
        let findings = rust_src(src);
        assert_eq!(
            findings.len(),
            0,
            "default HashMap (SipHash/RandomState) is collision-resistant"
        );
    }

    // -------------------------------------------------------------------------
    // 8. Rust FxHashMap + user-controlled key → finding
    // -------------------------------------------------------------------------

    #[test]
    fn rust_fxhashmap_with_user_input_is_flagged() {
        let src = r#"
use rustc_hash::FxHashMap;

async fn handler(query: Query<Params>) -> impl Responder {
    let mut map: FxHashMap<String, String> = FxHashMap::default();
    map.insert(query.key.clone(), query.value.clone());
    HttpResponse::Ok().json(map)
}
"#;
        let findings = rust_src(src);
        assert!(
            !findings.is_empty(),
            "FxHashMap is non-randomised and should be flagged when keys are user-controlled"
        );
        assert_eq!(findings[0].cwe_ids, vec![400]);
        assert_eq!(findings[0].severity, Severity::Medium);
        assert!(!findings[0].noisy);
    }
}
