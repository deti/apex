//! i18n Completeness Check — finds hardcoded strings and missing translations.

use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub struct I18nIssue {
    pub file: PathBuf,
    pub line: u32,
    pub issue_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct I18nReport {
    pub hardcoded_strings: usize,
    pub missing_keys: Vec<(String, String)>, // (key, locale)
    pub issues: Vec<I18nIssue>,
}

static HARDCODED_UI_STRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:label|title|placeholder|text|message|error|warning)\s*[:=]\s*['"]([^'"]{3,})['"]"#,
    )
    .unwrap()
});

static I18N_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:t|i18n|__|gettext|ngettext|translate)\s*\(\s*['"]([^'"]+)['"]"#).unwrap()
});

/// Scan source for hardcoded UI strings not wrapped in i18n calls.
pub fn check_i18n(source_cache: &HashMap<PathBuf, String>) -> I18nReport {
    let mut issues = Vec::new();
    let mut hardcoded = 0usize;

    for (path, source) in source_cache {
        for (line_num, line) in source.lines().enumerate() {
            let ln = (line_num + 1) as u32;
            // Check for hardcoded strings that should be i18n'd
            if HARDCODED_UI_STRING.is_match(line) && !I18N_CALL.is_match(line) {
                hardcoded += 1;
                issues.push(I18nIssue {
                    file: path.clone(),
                    line: ln,
                    issue_type: "hardcoded-string".into(),
                    description: "UI string not wrapped in i18n function".into(),
                });
            }
        }
    }

    I18nReport {
        hardcoded_strings: hardcoded,
        missing_keys: vec![],
        issues,
    }
}

/// Compare translation files across locales to find missing keys.
pub fn compare_locales(
    locale_files: &HashMap<String, HashMap<String, String>>,
) -> Vec<(String, String)> {
    let all_keys: HashSet<&str> = locale_files
        .values()
        .flat_map(|m| m.keys().map(|k| k.as_str()))
        .collect();

    let mut missing = Vec::new();
    for (locale, translations) in locale_files {
        for key in &all_keys {
            if !translations.contains_key(*key) {
                missing.push((key.to_string(), locale.clone()));
            }
        }
    }
    missing.sort();
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_hardcoded_string() {
        let mut c = HashMap::new();
        c.insert(PathBuf::from("app.jsx"), r#"label = "Submit Form""#.into());
        let r = check_i18n(&c);
        assert_eq!(r.hardcoded_strings, 1);
    }

    #[test]
    fn ignores_i18n_wrapped() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("app.jsx"),
            r#"label = t("submit_form")"#.into(),
        );
        let r = check_i18n(&c);
        assert_eq!(r.hardcoded_strings, 0);
    }

    #[test]
    fn compare_locales_finds_missing() {
        let mut locales = HashMap::new();
        locales.insert(
            "en".into(),
            HashMap::from([
                ("hello".into(), "Hello".into()),
                ("bye".into(), "Goodbye".into()),
            ]),
        );
        locales.insert(
            "fr".into(),
            HashMap::from([("hello".into(), "Bonjour".into())]),
        );
        let missing = compare_locales(&locales);
        assert!(missing.iter().any(|(k, l)| k == "bye" && l == "fr"));
    }

    #[test]
    fn no_missing_when_complete() {
        let mut locales = HashMap::new();
        locales.insert("en".into(), HashMap::from([("hi".into(), "Hi".into())]));
        locales.insert(
            "fr".into(),
            HashMap::from([("hi".into(), "Salut".into())]),
        );
        let missing = compare_locales(&locales);
        assert!(missing.is_empty());
    }
}
