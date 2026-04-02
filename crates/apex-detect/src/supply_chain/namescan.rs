use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Levenshtein distance
// ---------------------------------------------------------------------------

/// Compute the Levenshtein (edit) distance between two strings.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    // Single-row DP: prev[j] holds dist(a[..i], b[..j]).
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

// ---------------------------------------------------------------------------
// Homoglyph detection
// ---------------------------------------------------------------------------

/// Confusable character pairs: (look-alike, ASCII original).
const CONFUSABLES: &[(char, char)] = &[
    ('\u{0430}', 'a'), // Cyrillic а
    ('\u{0435}', 'e'), // Cyrillic е
    ('\u{043E}', 'o'), // Cyrillic о
    ('\u{0440}', 'p'), // Cyrillic р
    ('\u{0441}', 'c'), // Cyrillic с
    ('\u{0443}', 'y'), // Cyrillic у
    ('\u{0456}', 'i'), // Cyrillic і
    ('\u{03BD}', 'v'), // Greek nu
    ('\u{03BF}', 'o'), // Greek omicron
    ('\u{03B1}', 'a'), // Greek alpha
    ('\u{03C4}', 't'), // Greek tau
    ('\u{210E}', 'h'), // Mathematical h
    ('\u{2170}', 'i'), // Roman numeral i
    ('\u{217C}', 'l'), // Roman numeral l
    ('\u{FF10}', '0'), // Fullwidth 0
    ('\u{FF11}', '1'), // Fullwidth 1
    ('\u{30FC}', '-'), // Katakana long vowel (looks like hyphen)
];

/// Normalize a package name by replacing confusable characters with ASCII.
pub fn normalize_homoglyphs(name: &str) -> String {
    name.chars()
        .map(|ch| {
            CONFUSABLES
                .iter()
                .find(|(look_alike, _)| *look_alike == ch)
                .map(|(_, ascii)| *ascii)
                .unwrap_or(ch)
        })
        .collect()
}

/// Check if a name contains any non-ASCII confusable characters.
pub fn has_homoglyphs(name: &str) -> bool {
    name.chars().any(|ch| {
        CONFUSABLES
            .iter()
            .any(|(look_alike, _)| *look_alike == ch)
    })
}

/// Return the list of confusable (original, replacement) pairs found in `name`.
fn find_confusable_chars(name: &str) -> Vec<(char, char)> {
    name.chars()
        .filter_map(|ch| {
            CONFUSABLES
                .iter()
                .find(|(look_alike, _)| *look_alike == ch)
                .copied()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Finding types
// ---------------------------------------------------------------------------

/// Aggregated result for a single package name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameScanResult {
    pub package: String,
    pub findings: Vec<NameFinding>,
}

/// Individual finding about a suspicious package name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NameFinding {
    Typosquat {
        similar_to: String,
        distance: usize,
        risk_score: f64,
    },
    Homoglyph {
        normalized: String,
        confusable_chars: Vec<(char, char)>,
        risk_score: f64,
    },
    SuspiciousName {
        reason: String,
        risk_score: f64,
    },
}

// ---------------------------------------------------------------------------
// Main scan function
// ---------------------------------------------------------------------------

/// Scan a list of dependency names against known popular packages.
///
/// Returns only packages that have at least one finding.
pub fn scan_names(dep_names: &[String], popular_packages: &[String]) -> Vec<NameScanResult> {
    dep_names
        .iter()
        .filter_map(|dep| {
            let findings = scan_single(dep, popular_packages);
            if findings.is_empty() {
                None
            } else {
                Some(NameScanResult {
                    package: dep.clone(),
                    findings,
                })
            }
        })
        .collect()
}

/// Produce findings for a single dependency name.
fn scan_single(dep: &str, popular_packages: &[String]) -> Vec<NameFinding> {
    let mut findings = Vec::new();

    // Homoglyph check — independent of the popular list.
    if has_homoglyphs(dep) {
        let normalized = normalize_homoglyphs(dep);
        let confusable_chars = find_confusable_chars(dep);
        // If the normalized form matches a popular package, that is very high risk.
        let risk = if popular_packages.iter().any(|p| p == &normalized) {
            9.5
        } else {
            7.0
        };
        findings.push(NameFinding::Homoglyph {
            normalized,
            confusable_chars,
            risk_score: risk,
        });
    }

    // Levenshtein typosquat check.
    let normalized_dep = normalize_homoglyphs(dep);
    for popular in popular_packages {
        // Skip exact matches — they are not suspicious.
        if dep == popular.as_str() || normalized_dep == *popular {
            continue;
        }

        let dist = levenshtein(&normalized_dep, popular);
        if dist > 0 && dist <= 2 {
            let risk = match dist {
                1 => 8.0,
                2 => 5.5,
                _ => 3.0,
            };
            findings.push(NameFinding::Typosquat {
                similar_to: popular.clone(),
                distance: dist,
                risk_score: risk,
            });
        }
    }

    findings
}

// ---------------------------------------------------------------------------
// Popular package lists (top ~50 per ecosystem)
// ---------------------------------------------------------------------------

pub const POPULAR_NPM: &[&str] = &[
    "lodash",
    "express",
    "react",
    "axios",
    "chalk",
    "debug",
    "commander",
    "moment",
    "webpack",
    "typescript",
    "jest",
    "mocha",
    "eslint",
    "prettier",
    "next",
    "vue",
    "angular",
    "svelte",
    "jquery",
    "underscore",
    "uuid",
    "dotenv",
    "cors",
    "bluebird",
    "async",
    "glob",
    "yargs",
    "inquirer",
    "mkdirp",
    "semver",
    "rimraf",
    "fs-extra",
    "minimist",
    "body-parser",
    "cookie-parser",
    "passport",
    "mongoose",
    "sequelize",
    "knex",
    "graphql",
    "socket.io",
    "redis",
    "node-fetch",
    "cross-env",
    "nodemon",
    "concurrently",
    "husky",
    "lint-staged",
    "tslib",
    "rxjs",
];

pub const POPULAR_PYPI: &[&str] = &[
    "requests",
    "numpy",
    "pandas",
    "flask",
    "django",
    "boto3",
    "pydantic",
    "fastapi",
    "httpx",
    "aiohttp",
    "cryptography",
    "pillow",
    "scipy",
    "tensorflow",
    "torch",
    "openai",
    "anthropic",
    "langchain",
    "pytest",
    "setuptools",
    "pip",
    "wheel",
    "six",
    "urllib3",
    "certifi",
    "charset-normalizer",
    "idna",
    "packaging",
    "pyyaml",
    "jinja2",
    "markupsafe",
    "click",
    "colorama",
    "tqdm",
    "beautifulsoup4",
    "lxml",
    "scrapy",
    "celery",
    "gunicorn",
    "uvicorn",
    "sqlalchemy",
    "alembic",
    "psycopg2",
    "redis",
    "black",
    "ruff",
    "mypy",
    "isort",
    "flake8",
    "coverage",
];

pub const POPULAR_CARGO: &[&str] = &[
    "serde",
    "tokio",
    "clap",
    "reqwest",
    "rand",
    "regex",
    "log",
    "hyper",
    "actix-web",
    "axum",
    "tracing",
    "anyhow",
    "thiserror",
    "chrono",
    "serde_json",
    "syn",
    "quote",
    "proc-macro2",
    "futures",
    "bytes",
    "once_cell",
    "lazy_static",
    "itertools",
    "rayon",
    "crossbeam",
    "parking_lot",
    "dashmap",
    "uuid",
    "url",
    "http",
    "tower",
    "tonic",
    "prost",
    "diesel",
    "sqlx",
    "rusqlite",
    "tempfile",
    "walkdir",
    "glob",
    "env_logger",
    "tracing-subscriber",
    "config",
    "toml",
    "csv",
    "sha2",
    "ring",
    "rustls",
    "num",
    "bitflags",
    "memmap2",
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Levenshtein ----------------------------------------------------------

    #[test]
    fn levenshtein_identical() {
        assert_eq!(levenshtein("lodash", "lodash"), 0);
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("lodash", "1odash"), 1); // substitution
        assert_eq!(levenshtein("lodash", "lodasb"), 1);
        assert_eq!(levenshtein("requests", "reqeusts"), 2); // transposition (2 edits)
    }

    #[test]
    fn levenshtein_empty() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", ""), 0);
    }

    #[test]
    fn levenshtein_insertion_deletion() {
        assert_eq!(levenshtein("abc", "abcd"), 1);
        assert_eq!(levenshtein("abcd", "abc"), 1);
    }

    // -- Homoglyphs -----------------------------------------------------------

    #[test]
    fn homoglyph_cyrillic_e() {
        // "rеquests" with Cyrillic е instead of Latin e
        let name = "r\u{0435}quests";
        assert!(has_homoglyphs(name));
        let norm = normalize_homoglyphs(name);
        assert_eq!(norm, "requests");
    }

    #[test]
    fn homoglyph_clean_name() {
        assert!(!has_homoglyphs("requests"));
        assert!(!has_homoglyphs("my-package-123"));
    }

    #[test]
    fn homoglyph_multiple_substitutions() {
        // "lоdаsh" with Cyrillic о and а
        let name = "l\u{043E}d\u{0430}sh";
        assert!(has_homoglyphs(name));
        assert_eq!(normalize_homoglyphs(name), "lodash");
    }

    #[test]
    fn homoglyph_greek_nu() {
        // "νue" with Greek nu instead of v
        let name = "\u{03BD}ue";
        assert!(has_homoglyphs(name));
        assert_eq!(normalize_homoglyphs(name), "vue");
    }

    // -- scan_names -----------------------------------------------------------

    #[test]
    fn scan_finds_typosquat() {
        let deps = vec!["req-uests".to_string()]; // distance 1 from "requests" (insertion of -)
        let popular = vec!["requests".to_string()];
        let results = scan_names(&deps, &popular);
        assert!(!results.is_empty());
        assert!(results[0]
            .findings
            .iter()
            .any(|f| matches!(f, NameFinding::Typosquat { distance: 1, .. })));
    }

    #[test]
    fn scan_finds_homoglyph() {
        let deps = vec!["r\u{0435}quests".to_string()]; // Cyrillic е
        let popular = vec!["requests".to_string()];
        let results = scan_names(&deps, &popular);
        assert!(results[0]
            .findings
            .iter()
            .any(|f| matches!(f, NameFinding::Homoglyph { .. })));
    }

    #[test]
    fn scan_ignores_exact_match() {
        let deps = vec!["requests".to_string()];
        let popular = vec!["requests".to_string()];
        let results = scan_names(&deps, &popular);
        assert!(results.is_empty() || results[0].findings.is_empty());
    }

    #[test]
    fn scan_with_popular_lists() {
        let deps = vec!["requ3sts".to_string()]; // distance 1 from "requests"
        let results = scan_names(
            &deps,
            &POPULAR_PYPI.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
        assert!(results[0]
            .findings
            .iter()
            .any(|f| matches!(f, NameFinding::Typosquat { .. })));
    }

    #[test]
    fn scan_homoglyph_high_risk_when_matches_popular() {
        let deps = vec!["r\u{0435}quests".to_string()];
        let popular = vec!["requests".to_string()];
        let results = scan_names(&deps, &popular);
        let hg = results[0]
            .findings
            .iter()
            .find(|f| matches!(f, NameFinding::Homoglyph { .. }));
        assert!(hg.is_some());
        match hg.unwrap() {
            NameFinding::Homoglyph { risk_score, .. } => assert!(*risk_score >= 9.0),
            _ => unreachable!(),
        }
    }

    #[test]
    fn scan_distance_2_typosquat() {
        let deps = vec!["reqeusts".to_string()]; // transposition, distance 2
        let popular = vec!["requests".to_string()];
        let results = scan_names(&deps, &popular);
        assert!(results[0]
            .findings
            .iter()
            .any(|f| matches!(f, NameFinding::Typosquat { distance: 2, .. })));
    }

    #[test]
    fn scan_no_flag_for_distance_3() {
        let deps = vec!["abcdefgh".to_string()];
        let popular = vec!["requests".to_string()];
        let results = scan_names(&deps, &popular);
        assert!(results.is_empty());
    }

    #[test]
    fn scan_npm_popular() {
        let deps = vec!["1odash".to_string()]; // distance 1 from lodash
        let results = scan_names(
            &deps,
            &POPULAR_NPM.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
        assert!(results[0]
            .findings
            .iter()
            .any(|f| matches!(f, NameFinding::Typosquat { similar_to, .. } if similar_to == "lodash")));
    }

    #[test]
    fn scan_cargo_popular() {
        let deps = vec!["toklo".to_string()]; // distance 1 from tokio
        let results = scan_names(
            &deps,
            &POPULAR_CARGO
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        );
        assert!(results[0]
            .findings
            .iter()
            .any(|f| matches!(f, NameFinding::Typosquat { similar_to, .. } if similar_to == "tokio")));
    }
}
