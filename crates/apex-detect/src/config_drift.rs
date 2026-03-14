//! Config Drift Detection — compares configuration across environments.

use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub enum DriftSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigDrift {
    pub key: String,
    pub env_a_value: Option<String>,
    pub env_b_value: Option<String>,
    pub severity: DriftSeverity,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DriftReport {
    pub drifts: Vec<ConfigDrift>,
    pub total_keys: usize,
    pub matching_keys: usize,
    pub drifted_keys: usize,
}

/// Parse a .env file into key-value pairs.
pub fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = trimmed.split_once('=') {
            map.insert(
                key.trim().to_string(),
                val.trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            );
        }
    }
    map
}

/// Classify drift severity based on key name patterns.
fn classify_drift(key: &str) -> DriftSeverity {
    let key_upper = key.to_uppercase();
    if key_upper.contains("SECRET")
        || key_upper.contains("PASSWORD")
        || key_upper.contains("API_KEY")
        || key_upper.contains("TOKEN")
    {
        DriftSeverity::Critical
    } else if key_upper.contains("TIMEOUT")
        || key_upper.contains("LIMIT")
        || key_upper.contains("PORT")
        || key_upper.contains("HOST")
    {
        DriftSeverity::Warning
    } else {
        DriftSeverity::Info
    }
}

/// Compare two environment configs and report drift.
pub fn detect_drift(
    env_a: &HashMap<String, String>,
    env_b: &HashMap<String, String>,
    env_a_name: &str,
    env_b_name: &str,
) -> DriftReport {
    let mut all_keys: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for k in env_a.keys() {
        all_keys.insert(k);
    }
    for k in env_b.keys() {
        all_keys.insert(k);
    }

    let mut drifts = Vec::new();
    let mut matching = 0usize;

    for key in &all_keys {
        let a_val = env_a.get(*key);
        let b_val = env_b.get(*key);
        match (a_val, b_val) {
            (Some(a), Some(b)) if a == b => {
                matching += 1;
            }
            (Some(a), Some(b)) => {
                drifts.push(ConfigDrift {
                    key: key.to_string(),
                    env_a_value: Some(a.clone()),
                    env_b_value: Some(b.clone()),
                    severity: classify_drift(key),
                    description: format!(
                        "Value differs: {} has '{}', {} has '{}'",
                        env_a_name, a, env_b_name, b
                    ),
                });
            }
            (Some(_), None) => {
                drifts.push(ConfigDrift {
                    key: key.to_string(),
                    env_a_value: env_a.get(*key).cloned(),
                    env_b_value: None,
                    severity: classify_drift(key),
                    description: format!("Key only in {}", env_a_name),
                });
            }
            (None, Some(_)) => {
                drifts.push(ConfigDrift {
                    key: key.to_string(),
                    env_a_value: None,
                    env_b_value: env_b.get(*key).cloned(),
                    severity: classify_drift(key),
                    description: format!("Key only in {}", env_b_name),
                });
            }
            _ => {}
        }
    }

    DriftReport {
        total_keys: all_keys.len(),
        matching_keys: matching,
        drifted_keys: drifts.len(),
        drifts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_configs_no_drift() {
        let a = HashMap::from([("KEY".into(), "val".into())]);
        let r = detect_drift(&a, &a, "staging", "prod");
        assert_eq!(r.drifted_keys, 0);
        assert_eq!(r.matching_keys, 1);
    }

    #[test]
    fn detects_value_difference() {
        let a = HashMap::from([("PORT".into(), "8080".into())]);
        let b = HashMap::from([("PORT".into(), "3000".into())]);
        let r = detect_drift(&a, &b, "staging", "prod");
        assert_eq!(r.drifted_keys, 1);
    }

    #[test]
    fn detects_missing_key() {
        let a = HashMap::from([("KEY".into(), "val".into())]);
        let b = HashMap::new();
        let r = detect_drift(&a, &b, "staging", "prod");
        assert_eq!(r.drifted_keys, 1);
    }

    #[test]
    fn secret_key_is_critical() {
        let a = HashMap::from([("API_KEY".into(), "abc".into())]);
        let b = HashMap::from([("API_KEY".into(), "xyz".into())]);
        let r = detect_drift(&a, &b, "s", "p");
        assert!(matches!(r.drifts[0].severity, DriftSeverity::Critical));
    }

    #[test]
    fn parse_env_file_basic() {
        let content = "KEY=value\n# comment\nDB_HOST=localhost\n";
        let m = parse_env_file(content);
        assert_eq!(m.get("KEY").unwrap(), "value");
        assert_eq!(m.get("DB_HOST").unwrap(), "localhost");
        assert_eq!(m.len(), 2);
    }
}
